//! Chat Completions：BYOT 请求/分片类型（标准子结构复用 SDK，方言字段自补）与增量累积。

use std::collections::{BTreeMap, HashMap};

use async_openai::types::chat::{
    ChatCompletionMessageToolCall, ChatCompletionMessageToolCalls,
    ChatCompletionRequestAssistantMessage, ChatCompletionRequestAssistantMessageContent,
    ChatCompletionRequestMessage, ChatCompletionRequestMessageContentPartImage,
    ChatCompletionRequestMessageContentPartText, ChatCompletionRequestSystemMessage,
    ChatCompletionRequestSystemMessageContent, ChatCompletionRequestToolMessage,
    ChatCompletionRequestToolMessageContent, ChatCompletionRequestUserMessage,
    ChatCompletionRequestUserMessageContent, ChatCompletionRequestUserMessageContentPart,
    ChatCompletionStreamOptions, ChatCompletionStreamResponseDelta, ChatCompletionTool,
    ChatCompletionTools, CompletionUsage, FunctionCall, FunctionObject, ImageUrl, ReasoningEffort,
};
use mic_core::{ModelEvent, ModelRequest, ModelResponse, ProviderError, StopReason};
use mic_message::{ContentPart, ImageData, ImageId, ModelView, Reasoning, ReplyBlock};
use mic_store::Usage;
use serde::{Deserialize, Serialize};

use crate::config::{Dialect, Resolved};
use crate::content::{
    arguments_text, data_url, has_image, parse_arguments, text_of, unsupported_file,
};
use crate::error::{protocol, transient};
use crate::stream::{Accumulate, Step};

#[derive(Serialize)]
pub(crate) struct ChatRequest {
    model: String,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<ChatCompletionTools>,
    stream: bool,
    stream_options: ChatCompletionStreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Serialize)]
struct Message {
    #[serde(flatten)]
    base: ChatCompletionRequestMessage,
    /// DeepSeek 思考模式下工具往返必须回传。
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_content: Option<String>,
}

impl From<ChatCompletionRequestMessage> for Message {
    fn from(base: ChatCompletionRequestMessage) -> Self {
        Self {
            base,
            reasoning_content: None,
        }
    }
}

pub(crate) fn request(
    cfg: &Resolved,
    dialect: Dialect,
    req: &ModelRequest,
) -> Result<ChatRequest, ProviderError> {
    let mut messages: Vec<Message> = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(
            ChatCompletionRequestMessage::System(ChatCompletionRequestSystemMessage {
                content: ChatCompletionRequestSystemMessageContent::Text(req.system.clone()),
                name: None,
            })
            .into(),
        );
    }
    for m in &req.messages {
        let Some(view) = m.model_view() else {
            continue;
        };
        messages.push(match view {
            ModelView::User(parts) => {
                ChatCompletionRequestMessage::User(ChatCompletionRequestUserMessage {
                    content: user_content(parts, &req.images)?,
                    name: None,
                })
                .into()
            }
            ModelView::Assistant { model, blocks } => {
                assistant(blocks, dialect == Dialect::DeepSeek && model == cfg.model)
            }
            ModelView::Tool {
                tool_call_id,
                output,
            } => ChatCompletionRequestMessage::Tool(ChatCompletionRequestToolMessage {
                content: ChatCompletionRequestToolMessageContent::Text(text_of(output)?),
                tool_call_id: tool_call_id.to_owned(),
            })
            .into(),
        });
    }
    Ok(ChatRequest {
        model: cfg.model.clone(),
        messages,
        tools: req
            .tools
            .iter()
            .map(|t| {
                ChatCompletionTools::Function(ChatCompletionTool {
                    function: FunctionObject {
                        name: t.name.clone(),
                        description: Some(t.description.clone()),
                        parameters: Some(t.parameters.clone()),
                        strict: None,
                    },
                })
            })
            .collect(),
        stream: true,
        stream_options: ChatCompletionStreamOptions {
            include_usage: Some(true),
            include_obfuscation: None,
        },
        max_tokens: cfg.max_tokens,
        reasoning_effort: cfg.reasoning_effort.map(|e| e.sdk()),
    })
}

/// 一条 `Reply` 即一个 assistant turn。`echo_reasoning`：DeepSeek 且由本模型产出。
fn assistant(blocks: &[ReplyBlock], echo_reasoning: bool) -> Message {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for block in blocks {
        match block {
            ReplyBlock::Text { text: t, .. } => text.push_str(t),
            ReplyBlock::Reasoning(Reasoning::Visible { text: r, .. }) => reasoning.push_str(r),
            // 密文推理只有 Responses 能回传。
            ReplyBlock::Reasoning(Reasoning::Redacted { .. }) => {}
            ReplyBlock::ToolCall { id, name, args } => tool_calls.push(
                ChatCompletionMessageToolCalls::Function(ChatCompletionMessageToolCall {
                    id: id.clone(),
                    function: FunctionCall {
                        name: name.clone(),
                        arguments: arguments_text(args),
                    },
                }),
            ),
        }
    }
    // 思考模式下工具往返必须回传推理；其余推理不发，省 token。
    let echo = echo_reasoning && !tool_calls.is_empty() && !reasoning.is_empty();
    Message {
        base: ChatCompletionRequestMessage::Assistant(ChatCompletionRequestAssistantMessage {
            content: (!text.is_empty())
                .then_some(ChatCompletionRequestAssistantMessageContent::Text(text)),
            tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
            ..Default::default()
        }),
        reasoning_content: echo.then_some(reasoning),
    }
}

/// 无图时文本片段按行拼接；含图时按序发 text/image_url 分片，相邻文本合并。
fn user_content(
    parts: Vec<ContentPart>,
    images: &HashMap<ImageId, ImageData>,
) -> Result<ChatCompletionRequestUserMessageContent, ProviderError> {
    if !has_image(&parts) {
        return Ok(ChatCompletionRequestUserMessageContent::Text(text_of(
            parts,
        )?));
    }
    let mut out = Vec::with_capacity(parts.len());
    let mut texts: Vec<String> = Vec::new();
    let flush = |texts: &mut Vec<String>, out: &mut Vec<_>| {
        if !texts.is_empty() {
            out.push(ChatCompletionRequestUserMessageContentPart::Text(
                ChatCompletionRequestMessageContentPartText {
                    text: std::mem::take(texts).join("\n"),
                    prompt_cache_breakpoint: None,
                },
            ));
        }
    };
    for part in parts {
        match part {
            ContentPart::Text { text } => texts.push(text),
            ContentPart::File(file) => return Err(unsupported_file(&file)),
            ContentPart::Image(r) => {
                flush(&mut texts, &mut out);
                let img = images
                    .get(&r.id)
                    .expect("core 已按窗口引用加载全部图片原件");
                out.push(ChatCompletionRequestUserMessageContentPart::ImageUrl(
                    ChatCompletionRequestMessageContentPartImage {
                        image_url: ImageUrl {
                            url: data_url(img),
                            detail: None,
                        },
                        prompt_cache_breakpoint: None,
                    },
                ));
            }
        }
    }
    flush(&mut texts, &mut out);
    Ok(ChatCompletionRequestUserMessageContent::Array(out))
}

#[derive(Deserialize)]
pub(crate) struct Chunk {
    choices: Vec<Choice>,
    usage: Option<ChatUsage>,
}

#[derive(Deserialize)]
struct Choice {
    index: u32,
    delta: Delta,
    finish_reason: Option<FinishReason>,
}

#[derive(Deserialize)]
struct Delta {
    #[serde(flatten)]
    base: ChatCompletionStreamResponseDelta,
    /// DeepSeek 推理增量。
    reasoning_content: Option<String>,
    /// Ollama 推理增量。
    reasoning: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    FunctionCall,
    /// DeepSeek：上游资源不足中断。
    InsufficientSystemResource,
}

#[derive(Deserialize)]
struct ChatUsage {
    #[serde(flatten)]
    base: CompletionUsage,
    /// DeepSeek 缓存命中。
    prompt_cache_hit_tokens: Option<u64>,
}

pub(crate) struct Accumulator {
    dialect: Dialect,
    reasoning: String,
    text: String,
    /// 按上游 `index` 排序。
    tool_calls: BTreeMap<u32, PendingCall>,
    finish_reason: Option<FinishReason>,
    usage: Option<ChatUsage>,
}

#[derive(Default)]
struct PendingCall {
    id: Option<String>,
    name: Option<String>,
    arguments: String,
}

impl Accumulator {
    pub(crate) fn new(dialect: Dialect) -> Self {
        Self {
            dialect,
            reasoning: String::new(),
            text: String::new(),
            tool_calls: BTreeMap::new(),
            finish_reason: None,
            usage: None,
        }
    }
}

impl Accumulate for Accumulator {
    type Event = Chunk;

    fn push(&mut self, chunk: Chunk) -> Result<Step, ProviderError> {
        if let Some(usage) = chunk.usage {
            self.usage = Some(usage);
        }
        let mut events = Vec::new();
        for choice in chunk.choices {
            if choice.index != 0 {
                return Err(protocol(format!("收到多个候选（index {}）", choice.index)));
            }
            let Delta {
                base,
                reasoning_content,
                reasoning,
            } = choice.delta;
            let reasoning = match self.dialect {
                Dialect::DeepSeek => reasoning_content,
                Dialect::Ollama => reasoning,
                Dialect::Generic => None,
            }
            .filter(|s| !s.is_empty());
            let content = base.content.filter(|s| !s.is_empty());
            let tool_calls = base.tool_calls.unwrap_or_default();
            if (reasoning.is_some() || content.is_some() || !tool_calls.is_empty())
                && self.finish_reason.is_some()
            {
                return Err(protocol("finish_reason 之后仍有内容增量"));
            }
            if let Some(r) = reasoning {
                self.reasoning.push_str(&r);
                events.push(ModelEvent::ReasoningDelta(r));
            }
            if let Some(t) = content {
                self.text.push_str(&t);
                events.push(ModelEvent::TextDelta(t));
            }
            for call in tool_calls {
                let pending = self.tool_calls.entry(call.index).or_default();
                if let Some(id) = call.id {
                    pending.id = Some(id);
                }
                if let Some(f) = call.function {
                    if let Some(name) = f.name {
                        pending.name.get_or_insert_default().push_str(&name);
                    }
                    if let Some(args) = f.arguments {
                        pending.arguments.push_str(&args);
                    }
                }
            }
            if let Some(reason) = choice.finish_reason {
                self.finish_reason = Some(reason);
            }
        }
        Ok(Step::Deltas(events))
    }

    /// Chat 流以 SDK 正常 EOF 结束；终态以 finish_reason 为准。
    fn eof(self) -> Result<ModelResponse, ProviderError> {
        let stop = match self.finish_reason {
            Some(FinishReason::Stop) => StopReason::EndTurn,
            Some(FinishReason::ToolCalls) => StopReason::ToolUse,
            Some(FinishReason::Length) => StopReason::MaxTokens,
            Some(FinishReason::ContentFilter) => StopReason::ContentFilter,
            Some(FinishReason::InsufficientSystemResource) => {
                return Err(transient("上游资源不足，回复被中断"))
            }
            Some(FinishReason::FunctionCall) => {
                return Err(protocol("收到未请求的 legacy function_call"))
            }
            None => return Err(transient("流在 finish_reason 之前结束")),
        };
        let mut blocks = Vec::new();
        if !self.reasoning.is_empty() {
            blocks.push(ReplyBlock::Reasoning(Reasoning::Visible {
                text: self.reasoning,
                signature: None,
            }));
        }
        if !self.text.is_empty() {
            blocks.push(ReplyBlock::Text {
                text: self.text,
                phase: None,
            });
        }
        for (index, call) in self.tool_calls {
            let (Some(id), Some(name)) = (call.id, call.name) else {
                return Err(protocol(format!("工具调用 #{index} 缺少 id 或 name")));
            };
            blocks.push(ReplyBlock::ToolCall {
                id,
                name,
                args: parse_arguments(call.arguments),
            });
        }
        Ok(ModelResponse {
            blocks,
            stop,
            usage: self.usage.map(|u| usage(self.dialect, u)),
        })
    }
}

fn usage(dialect: Dialect, u: ChatUsage) -> Usage {
    let cache_read_tokens = match dialect {
        Dialect::DeepSeek => u.prompt_cache_hit_tokens,
        Dialect::Ollama => None,
        Dialect::Generic => u
            .base
            .prompt_tokens_details
            .and_then(|d| d.cached_tokens)
            .map(u64::from),
    };
    Usage {
        input_tokens: u.base.prompt_tokens.into(),
        output_tokens: u.base.completion_tokens.into(),
        cache_read_tokens,
        cache_write_tokens: None,
        reasoning_tokens: u
            .base
            .completion_tokens_details
            .and_then(|d| d.reasoning_tokens)
            .map(u64::from),
    }
}
