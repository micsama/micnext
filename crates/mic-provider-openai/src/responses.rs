//! Responses：请求映射（无状态 `store=false`）、流事件处理与输出项转换。

use std::collections::HashMap;
use std::mem::take;

use mic_core::{ModelEvent, ModelRequest, ModelResponse, ProviderError, StopReason, ToolChoice};
use mic_message::{
    AssistantPhase, ContentPart, ImageData, ImageId, ModelView, Reasoning, ReplyBlock,
};
use mic_store::{ReasoningEffort, Usage};
use serde::{Deserialize, Serialize};

use crate::config::Resolved;
use crate::content::{
    arguments_text, data_url, has_image, parse_arguments, text_of, unsupported_file,
};
use crate::error::{protocol, transient, upstream};
use crate::stream::{Accumulate, Step};

#[derive(Serialize)]
pub(crate) struct ResponsesRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    tool_choice: Option<&'static str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<Input>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningParam>,
    include: [&'static str; 1],
    prompt_cache_key: String,
    store: bool,
    stream: bool,
}

#[derive(Serialize)]
struct ReasoningParam {
    effort: ReasoningEffort,
    summary: &'static str,
}

#[derive(Serialize)]
#[serde(tag = "type", rename = "function")]
struct Tool {
    name: String,
    description: String,
    parameters: serde_json::Value,
    strict: bool,
}

/// 无状态续轮：输入项一律不带 id。
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Input {
    Message {
        role: Role,
        content: Content,
        #[serde(skip_serializing_if = "Option::is_none")]
        phase: Option<Phase>,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    FunctionCallOutput {
        call_id: String,
        output: String,
    },
    Reasoning {
        summary: Vec<SummaryPart>,
        encrypted_content: String,
    },
}

#[derive(Serialize)]
#[serde(rename_all = "lowercase")]
enum Role {
    User,
    Assistant,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Content {
    Text(String),
    Parts(Vec<InputPart>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum InputPart {
    InputText {
        text: String,
    },
    InputImage {
        image_url: String,
        detail: &'static str,
    },
}

#[derive(Serialize, Deserialize, Clone, Copy)]
#[serde(rename_all = "snake_case")]
pub(crate) enum Phase {
    Commentary,
    FinalAnswer,
}

#[derive(Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum SummaryPart {
    SummaryText { text: String },
}

pub(crate) fn request(
    cfg: &Resolved,
    req: &ModelRequest,
) -> Result<ResponsesRequest, ProviderError> {
    let mut input = Vec::with_capacity(req.messages.len());
    for m in &req.messages {
        let Some(view) = m.model_view() else {
            continue;
        };
        match view {
            ModelView::User(parts) => input.push(Input::Message {
                role: Role::User,
                content: user_content(parts, &req.images)?,
                phase: None,
            }),
            ModelView::Assistant { model, blocks } => {
                assistant(blocks, model == cfg.model, &mut input)
            }
            ModelView::Tool {
                tool_call_id,
                output,
            } => input.push(Input::FunctionCallOutput {
                call_id: tool_call_id.to_owned(),
                output: text_of(output)?,
            }),
        }
    }
    Ok(ResponsesRequest {
        model: cfg.model.clone(),
        tool_choice: match req.tool_choice {
            ToolChoice::Auto => None,
            ToolChoice::None => Some("none"),
        },
        instructions: (!req.system.is_empty()).then(|| req.system.clone()),
        input,
        tools: req
            .tools
            .iter()
            .map(|t| Tool {
                name: t.name.clone(),
                description: t.description.clone(),
                parameters: t.parameters.clone(),
                strict: false,
            })
            .collect(),
        max_output_tokens: cfg.max_tokens,
        reasoning: cfg.reasoning_effort.map(|effort| ReasoningParam {
            effort,
            summary: "auto",
        }),
        include: ["reasoning.encrypted_content"],
        prompt_cache_key: req.cache_key.clone(),
        store: false,
        stream: true,
    })
}

/// 按块原序回传；推理仅同模型且有密文时回传。
fn assistant(blocks: &[ReplyBlock], same_model: bool, input: &mut Vec<Input>) {
    for block in blocks {
        match block {
            ReplyBlock::Text { text, phase } => input.push(Input::Message {
                role: Role::Assistant,
                content: Content::Text(text.clone()),
                phase: phase.map(|p| match p {
                    AssistantPhase::Commentary => Phase::Commentary,
                    AssistantPhase::FinalAnswer => Phase::FinalAnswer,
                }),
            }),
            ReplyBlock::ToolCall { id, name, args } => input.push(Input::FunctionCall {
                call_id: id.clone(),
                name: name.clone(),
                arguments: arguments_text(args),
            }),
            ReplyBlock::Reasoning(r) if same_model => match r {
                Reasoning::Visible {
                    text,
                    signature: Some(enc),
                } => input.push(Input::Reasoning {
                    summary: vec![SummaryPart::SummaryText { text: text.clone() }],
                    encrypted_content: enc.clone(),
                }),
                Reasoning::Visible {
                    signature: None, ..
                } => {}
                Reasoning::Redacted { data } => input.push(Input::Reasoning {
                    summary: Vec::new(),
                    encrypted_content: data.clone(),
                }),
            },
            ReplyBlock::Reasoning(_) => {}
        }
    }
}

/// 无图时文本片段按行拼接；含图时按序发 input_text/input_image，相邻文本合并。
fn user_content(
    parts: Vec<ContentPart>,
    images: &HashMap<ImageId, ImageData>,
) -> Result<Content, ProviderError> {
    if !has_image(&parts) {
        return Ok(Content::Text(text_of(parts)?));
    }
    let mut out = Vec::with_capacity(parts.len());
    let mut texts: Vec<String> = Vec::new();
    let flush = |texts: &mut Vec<String>, out: &mut Vec<InputPart>| {
        if !texts.is_empty() {
            out.push(InputPart::InputText {
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
                out.push(InputPart::InputImage {
                    image_url: data_url(img),
                    detail: "auto",
                });
            }
        }
    }
    flush(&mut texts, &mut out);
    Ok(Content::Parts(out))
}

/// 只解析用到的事件；已知不用的静默跳过，未知的忽略并告警（provider-handwritten §四）。
#[derive(Deserialize)]
#[serde(tag = "type")]
pub(crate) enum Event {
    #[serde(rename = "response.output_text.delta")]
    OutputTextDelta { delta: String },
    #[serde(rename = "response.refusal.delta")]
    RefusalDelta { delta: String },
    #[serde(rename = "response.reasoning_summary_text.delta")]
    ReasoningSummaryTextDelta { delta: String },
    #[serde(rename = "response.function_call_arguments.delta")]
    FunctionCallArgumentsDelta {},
    #[serde(rename = "response.output_item.done")]
    OutputItemDone { output_index: u32, item: OutputItem },
    #[serde(rename = "response.completed")]
    Completed { response: ResponseBody },
    #[serde(rename = "response.incomplete")]
    Incomplete { response: ResponseBody },
    #[serde(rename = "response.failed")]
    Failed { response: ResponseBody },
    #[serde(rename = "error")]
    Error {
        #[serde(flatten)]
        detail: StreamError,
    },
    #[serde(
        rename = "response.created",
        alias = "response.in_progress",
        alias = "response.queued",
        alias = "response.output_item.added",
        alias = "response.content_part.added",
        alias = "response.content_part.done",
        alias = "response.output_text.done",
        alias = "response.output_text.annotation.added",
        alias = "response.refusal.done",
        alias = "response.function_call_arguments.done",
        alias = "response.reasoning_summary_part.added",
        alias = "response.reasoning_summary_part.done",
        alias = "response.reasoning_summary_text.done",
        alias = "response.reasoning_text.delta",
        alias = "response.reasoning_text.done",
        alias = "codex.rate_limits"
    )]
    Unused,
    #[serde(other)]
    Other,
}

/// 官方 API 在顶层给错误；Codex 后端放在 `error` 对象内。
#[derive(Deserialize)]
#[serde(untagged)]
pub(crate) enum StreamError {
    Nested { error: StreamErrorDetail },
    Flat(StreamErrorDetail),
}

#[derive(Deserialize)]
pub(crate) struct StreamErrorDetail {
    code: Option<String>,
    message: String,
}

#[derive(Deserialize)]
pub(crate) struct ResponseBody {
    usage: Option<ResponseUsage>,
    incomplete_details: Option<IncompleteDetails>,
    error: Option<ResponseError>,
}

#[derive(Deserialize)]
struct IncompleteDetails {
    reason: String,
}

#[derive(Deserialize)]
struct ResponseError {
    code: String,
    message: String,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OutputItem {
    Message {
        content: Vec<OutputContent>,
        phase: Option<Phase>,
    },
    FunctionCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    Reasoning {
        summary: Vec<SummaryPart>,
        encrypted_content: Option<String>,
    },
    /// 内置工具调用等未请求的输出项。
    #[serde(other)]
    Other,
}

#[derive(Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum OutputContent {
    OutputText { text: String },
    Refusal { refusal: String },
}

#[derive(Deserialize)]
struct ResponseUsage {
    input_tokens: u32,
    input_tokens_details: InputTokensDetails,
    output_tokens: u32,
    output_tokens_details: OutputTokensDetails,
}

#[derive(Deserialize)]
struct InputTokensDetails {
    cached_tokens: u32,
    cache_write_tokens: Option<i64>,
}

#[derive(Deserialize)]
struct OutputTokensDetails {
    reasoning_tokens: u32,
}

/// 增量只用于实时展示；`response.output_item.done` 的条目是唯一内容来源
/// （Codex 后端终态 `output` 为空，官方 API 两处一致）。
#[derive(Default)]
pub(crate) struct Accumulator {
    items: Vec<(u32, OutputItem)>,
}

impl Accumulate for Accumulator {
    type Event = Event;

    fn push(&mut self, event: Event) -> Result<Step, ProviderError> {
        Ok(match event {
            Event::OutputTextDelta { delta } | Event::RefusalDelta { delta } => {
                Step::Deltas(vec![ModelEvent::TextDelta(delta)])
            }
            Event::ReasoningSummaryTextDelta { delta } => {
                Step::Deltas(vec![ModelEvent::ReasoningDelta(delta)])
            }
            Event::FunctionCallArgumentsDelta {} => Step::Deltas(vec![ModelEvent::ToolCallDelta]),
            Event::OutputItemDone { output_index, item } => {
                self.items.push((output_index, item));
                Step::Deltas(Vec::new())
            }
            Event::Completed { response } => {
                let usage = response.usage.map(convert_usage).transpose()?;
                Step::Finished(finish(take(&mut self.items), None)?, usage)
            }
            Event::Incomplete { response } => {
                let stop = match response
                    .incomplete_details
                    .as_ref()
                    .map(|d| d.reason.as_str())
                {
                    Some("max_output_tokens") => StopReason::MaxTokens,
                    Some("content_filter") => StopReason::ContentFilter,
                    other => {
                        return Err(protocol(format!(
                            "未知的未完成原因 `{}`",
                            other.unwrap_or("无")
                        )))
                    }
                };
                let usage = response.usage.map(convert_usage).transpose()?;
                Step::Finished(finish(take(&mut self.items), Some(stop))?, usage)
            }
            Event::Failed { response } => {
                let error = match response.error {
                    Some(err) => upstream(Some(&err.code), &err.message),
                    None => protocol("response.failed 未给出错误"),
                };
                let usage = response.usage.and_then(|u| convert_usage(u).ok());
                Step::Failed(error, usage)
            }
            Event::Error { detail } => {
                let detail = match detail {
                    StreamError::Nested { error } | StreamError::Flat(error) => error,
                };
                return Err(upstream(detail.code.as_deref(), &detail.message));
            }
            Event::Unused => Step::Deltas(Vec::new()),
            Event::Other => Step::Unknown,
        })
    }

    fn eof(self) -> Result<ModelResponse, ProviderError> {
        Err(transient("流在终态事件之前结束"))
    }
}

/// `stop` 为 `None` 时由输出推断：有工具调用即 ToolUse，有拒答即 ContentFilter。
fn finish(
    mut items: Vec<(u32, OutputItem)>,
    stop: Option<StopReason>,
) -> Result<ModelResponse, ProviderError> {
    items.sort_by_key(|(index, _)| *index);
    let mut blocks = Vec::new();
    let mut refused = false;
    for (_, item) in items {
        match item {
            OutputItem::Message { content, phase } => {
                let mut text = String::new();
                for c in content {
                    match c {
                        OutputContent::OutputText { text: t } => text.push_str(&t),
                        OutputContent::Refusal { refusal } => {
                            refused = true;
                            text.push_str(&refusal);
                        }
                    }
                }
                if !text.is_empty() {
                    blocks.push(ReplyBlock::Text {
                        text,
                        phase: phase.map(|p| match p {
                            Phase::Commentary => AssistantPhase::Commentary,
                            Phase::FinalAnswer => AssistantPhase::FinalAnswer,
                        }),
                    });
                }
            }
            OutputItem::FunctionCall {
                call_id,
                name,
                arguments,
            } => blocks.push(ReplyBlock::ToolCall {
                id: call_id,
                name,
                args: parse_arguments(arguments),
            }),
            OutputItem::Reasoning {
                summary,
                encrypted_content,
            } => {
                if let Some(block) = reasoning(summary, encrypted_content) {
                    blocks.push(ReplyBlock::Reasoning(block));
                }
            }
            OutputItem::Other => return Err(protocol("收到未请求的输出项（如内置工具调用）")),
        }
    }
    let stop = stop.unwrap_or_else(|| {
        if blocks
            .iter()
            .any(|b| matches!(b, ReplyBlock::ToolCall { .. }))
        {
            StopReason::ToolUse
        } else if refused {
            StopReason::ContentFilter
        } else {
            StopReason::EndTurn
        }
    });
    Ok(ModelResponse { blocks, stop })
}

fn reasoning(summary: Vec<SummaryPart>, encrypted: Option<String>) -> Option<Reasoning> {
    let summary: Vec<String> = summary
        .into_iter()
        .map(|SummaryPart::SummaryText { text }| text)
        .collect();
    if summary.is_empty() {
        return encrypted.map(|data| Reasoning::Redacted { data });
    }
    Some(Reasoning::Visible {
        text: summary.join("\n\n"),
        signature: encrypted,
    })
}

fn convert_usage(u: ResponseUsage) -> Result<Usage, ProviderError> {
    let cache_write_tokens = u
        .input_tokens_details
        .cache_write_tokens
        .map(|n| u64::try_from(n).map_err(|_| protocol(format!("cache_write_tokens 为负数：{n}"))))
        .transpose()?;
    Ok(Usage {
        input_tokens: u.input_tokens.into(),
        output_tokens: u.output_tokens.into(),
        cache_read_tokens: Some(u.input_tokens_details.cached_tokens.into()),
        cache_write_tokens,
        reasoning_tokens: Some(u.output_tokens_details.reasoning_tokens.into()),
    })
}
