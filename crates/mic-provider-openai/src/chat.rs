//! Chat Completions：请求/分片类型（含 DeepSeek / Ollama 方言字段）与增量累积。

use std::collections::{BTreeMap, HashMap};

use mic_core::{ModelEvent, ModelRequest, ModelResponse, ProviderError, StopReason, ToolChoice};
use mic_message::{ContentPart, ImageData, ImageId, ModelView, Reasoning, ReplyBlock};
use mic_store::{ReasoningEffort, Usage};
use mic_tool::ToolSpec;
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
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    messages: Vec<Message>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool>,
    stream: bool,
    stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning_effort: Option<ReasoningEffort>,
}

#[derive(Serialize)]
struct StreamOptions {
    include_usage: bool,
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
enum Message {
    System {
        content: String,
    },
    User {
        content: UserContent,
    },
    Assistant {
        #[serde(skip_serializing_if = "Option::is_none")]
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        tool_calls: Option<Vec<ToolCall>>,
        /// DeepSeek 思考模式的推理回传。
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
    },
    Tool {
        content: String,
        tool_call_id: String,
    },
}

#[derive(Serialize)]
#[serde(untagged)]
enum UserContent {
    Text(String),
    Parts(Vec<UserPart>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum UserPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
struct ImageUrl {
    url: String,
}

#[derive(Serialize)]
#[serde(tag = "type", rename = "function")]
struct ToolCall {
    id: String,
    function: FunctionCall,
}

#[derive(Serialize)]
struct FunctionCall {
    name: String,
    arguments: String,
}

#[derive(Serialize)]
#[serde(tag = "type", rename = "function")]
struct Tool {
    function: Function,
}

#[derive(Serialize)]
struct Function {
    name: String,
    description: String,
    parameters: serde_json::Value,
}

impl From<&ToolSpec> for Tool {
    fn from(t: &ToolSpec) -> Self {
        Self {
            function: Function {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
            },
        }
    }
}

pub(crate) fn request(
    cfg: &Resolved,
    dialect: Dialect,
    req: &ModelRequest,
) -> Result<ChatRequest, ProviderError> {
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(Message::System {
            content: req.system.clone(),
        });
    }
    for m in &req.messages {
        let Some(view) = m.model_view() else {
            continue;
        };
        messages.push(match view {
            ModelView::User(parts) => Message::User {
                content: user_content(parts, &req.images)?,
            },
            ModelView::Assistant { model, blocks } => {
                assistant(blocks, dialect == Dialect::DeepSeek && model == cfg.model)
            }
            ModelView::Tool {
                tool_call_id,
                output,
            } => Message::Tool {
                content: text_of(output)?,
                tool_call_id: tool_call_id.to_owned(),
            },
        });
    }
    Ok(ChatRequest {
        model: cfg.model.clone(),
        tool_choice: match req.tool_choice {
            ToolChoice::Auto => None,
            ToolChoice::None => Some("none"),
        },
        messages,
        tools: req.tools.iter().map(Tool::from).collect(),
        stream: true,
        stream_options: StreamOptions {
            include_usage: true,
        },
        max_tokens: cfg.max_tokens,
        reasoning_effort: cfg.reasoning_effort,
    })
}

/// 一条 `Reply` 即一个 assistant turn。`echo_reasoning`：DeepSeek 且由本模型产出。
///
/// 按官方协议每轮都原样回传推理：带 tools 时历史推理全部进上下文，且原样回传才保持前缀缓存；
/// 未思考的轮次发空串（当前轮工具调用缺失或为 null 时上游 400）。
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
            ReplyBlock::ToolCall { id, name, args } => tool_calls.push(ToolCall {
                id: id.clone(),
                function: FunctionCall {
                    name: name.clone(),
                    arguments: arguments_text(args),
                },
            }),
        }
    }
    Message::Assistant {
        content: (!text.is_empty()).then_some(text),
        tool_calls: (!tool_calls.is_empty()).then_some(tool_calls),
        reasoning_content: echo_reasoning.then_some(reasoning),
    }
}

/// 无图时文本片段按行拼接；含图时按序发 text/image_url 分片，相邻文本合并。
fn user_content(
    parts: Vec<ContentPart>,
    images: &HashMap<ImageId, ImageData>,
) -> Result<UserContent, ProviderError> {
    if !has_image(&parts) {
        return Ok(UserContent::Text(text_of(parts)?));
    }
    let mut out = Vec::with_capacity(parts.len());
    let mut texts: Vec<String> = Vec::new();
    let flush = |texts: &mut Vec<String>, out: &mut Vec<UserPart>| {
        if !texts.is_empty() {
            out.push(UserPart::Text {
                text: std::mem::take(texts).join("\n"),
            });
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
                out.push(UserPart::ImageUrl {
                    image_url: ImageUrl { url: data_url(img) },
                });
            }
        }
    }
    flush(&mut texts, &mut out);
    Ok(UserContent::Parts(out))
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
    content: Option<String>,
    tool_calls: Option<Vec<ToolCallChunk>>,
    /// DeepSeek 推理增量。
    reasoning_content: Option<String>,
    /// Ollama 推理增量。
    reasoning: Option<String>,
}

#[derive(Deserialize)]
struct ToolCallChunk {
    index: u32,
    id: Option<String>,
    function: Option<FunctionChunk>,
}

#[derive(Deserialize)]
struct FunctionChunk {
    name: Option<String>,
    arguments: Option<String>,
}

#[derive(Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
enum FinishReason {
    Stop,
    Length,
    ToolCalls,
    ContentFilter,
    /// DeepSeek：上游资源不足中断。
    InsufficientSystemResource,
}

#[derive(Deserialize)]
struct ChatUsage {
    prompt_tokens: u32,
    completion_tokens: u32,
    prompt_tokens_details: Option<PromptDetails>,
    completion_tokens_details: Option<CompletionDetails>,
    /// DeepSeek 缓存命中。
    prompt_cache_hit_tokens: Option<u64>,
}

#[derive(Deserialize)]
struct PromptDetails {
    cached_tokens: Option<u32>,
}

#[derive(Deserialize)]
struct CompletionDetails {
    reasoning_tokens: Option<u32>,
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
                content,
                tool_calls,
                reasoning_content,
                reasoning,
            } = choice.delta;
            let reasoning = match self.dialect {
                Dialect::DeepSeek => reasoning_content,
                Dialect::Ollama => reasoning,
                Dialect::Generic => None,
            }
            .filter(|s| !s.is_empty());
            let content = content.filter(|s| !s.is_empty());
            let tool_calls = tool_calls.unwrap_or_default();
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

    /// EOF 或 `[DONE]`；终态以 finish_reason 为准。
    fn eof(self) -> Result<ModelResponse, ProviderError> {
        let stop = match self.finish_reason {
            Some(FinishReason::Stop) => StopReason::EndTurn,
            Some(FinishReason::ToolCalls) => StopReason::ToolUse,
            Some(FinishReason::Length) => StopReason::MaxTokens,
            Some(FinishReason::ContentFilter) => StopReason::ContentFilter,
            Some(FinishReason::InsufficientSystemResource) => {
                return Err(transient("上游资源不足，回复被中断"))
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
            .prompt_tokens_details
            .and_then(|d| d.cached_tokens)
            .map(u64::from),
    };
    Usage {
        input_tokens: u.prompt_tokens.into(),
        output_tokens: u.completion_tokens.into(),
        cache_read_tokens,
        cache_write_tokens: None,
        reasoning_tokens: u
            .completion_tokens_details
            .and_then(|d| d.reasoning_tokens)
            .map(u64::from),
    }
}
