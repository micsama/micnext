//! Responses：请求映射（无状态 `store=false`）、流事件处理与终态 output 转换。

use std::collections::HashMap;

use async_openai::types::responses::{
    EasyInputContent, EasyInputMessage, FunctionCallOutput, FunctionCallOutputItemParam,
    FunctionTool, FunctionToolCall, IncludeEnum, InputContent, InputImageContent, InputItem,
    InputTextContent, Item, MessagePhase, MessageType, OutputItem, OutputMessageContent,
    Reasoning as ReasoningParam, ReasoningItem, ReasoningSummary, Response, ResponseErrorCode,
    ResponseStreamEvent, ResponseUsage, Role, SummaryPart, SummaryTextContent, Tool,
};
use mic_core::{ModelEvent, ModelRequest, ModelResponse, ProviderError, StopReason};
use mic_message::{
    AssistantPhase, ContentPart, ImageData, ImageId, ModelView, Reasoning, ReplyBlock,
};
use mic_store::Usage;
use serde::Serialize;

use crate::config::Resolved;
use crate::content::{
    arguments_text, data_url, has_image, parse_arguments, text_of, unsupported_file,
};
use crate::error::{protocol, transient, upstream};
use crate::stream::{Accumulate, Step};

/// BYOT 请求体：SDK `ReasoningItem.id` 必序列化，而无状态续轮不能带 id，故输入项自定。
#[derive(Serialize)]
pub(crate) struct ResponsesRequest {
    model: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    instructions: Option<String>,
    input: Vec<Input>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    tools: Vec<Tool>,
    #[serde(skip_serializing_if = "Option::is_none")]
    max_output_tokens: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    reasoning: Option<ReasoningParam>,
    include: Vec<IncludeEnum>,
    store: bool,
    stream: bool,
}

#[derive(Serialize)]
#[serde(untagged)]
enum Input {
    Item(InputItem),
    Reasoning(ReasoningInput),
}

#[derive(Serialize)]
#[serde(tag = "type", rename = "reasoning")]
struct ReasoningInput {
    summary: Vec<SummaryPart>,
    encrypted_content: String,
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
            ModelView::User(parts) => {
                input.push(message(Role::User, user_content(parts, &req.images)?, None))
            }
            ModelView::Assistant { model, blocks } => {
                assistant(blocks, model == cfg.model, &mut input)
            }
            ModelView::Tool {
                tool_call_id,
                output,
            } => input.push(Input::Item(InputItem::Item(Item::FunctionCallOutput(
                FunctionCallOutputItemParam {
                    call_id: Some(tool_call_id.to_owned()),
                    output: FunctionCallOutput::Text(text_of(output)?),
                    id: None,
                    status: None,
                    name: None,
                    namespace: None,
                    caller: None,
                },
            )))),
        }
    }
    Ok(ResponsesRequest {
        model: cfg.model.clone(),
        instructions: (!req.system.is_empty()).then(|| req.system.clone()),
        input,
        tools: req
            .tools
            .iter()
            .map(|t| {
                Tool::Function(FunctionTool {
                    name: t.name.clone(),
                    parameters: Some(t.parameters.clone()),
                    strict: Some(false),
                    description: Some(t.description.clone()),
                    ..Default::default()
                })
            })
            .collect(),
        max_output_tokens: cfg.max_tokens,
        reasoning: cfg.reasoning_effort.map(|e| ReasoningParam {
            effort: Some(e.sdk()),
            summary: Some(ReasoningSummary::Auto),
            ..Default::default()
        }),
        include: vec![IncludeEnum::ReasoningEncryptedContent],
        store: false,
        stream: true,
    })
}

fn message(role: Role, content: EasyInputContent, phase: Option<MessagePhase>) -> Input {
    Input::Item(InputItem::EasyMessage(EasyInputMessage {
        r#type: MessageType::Message,
        role,
        content,
        phase,
    }))
}

/// 按块原序回传；推理仅同模型且有密文时回传。
fn assistant(blocks: &[ReplyBlock], same_model: bool, input: &mut Vec<Input>) {
    for block in blocks {
        match block {
            ReplyBlock::Text { text, phase } => input.push(message(
                Role::Assistant,
                EasyInputContent::Text(text.clone()),
                phase.map(|p| match p {
                    AssistantPhase::Commentary => MessagePhase::Commentary,
                    AssistantPhase::FinalAnswer => MessagePhase::FinalAnswer,
                }),
            )),
            ReplyBlock::ToolCall { id, name, args } => input.push(Input::Item(InputItem::Item(
                Item::FunctionCall(FunctionToolCall {
                    arguments: arguments_text(args),
                    call_id: id.clone(),
                    namespace: None,
                    name: name.clone(),
                    id: None,
                    status: None,
                    caller: None,
                    r#async: None,
                }),
            ))),
            ReplyBlock::Reasoning(r) if same_model => match r {
                Reasoning::Visible {
                    text,
                    signature: Some(enc),
                } => input.push(Input::Reasoning(ReasoningInput {
                    summary: vec![SummaryPart::SummaryText(SummaryTextContent {
                        text: text.clone(),
                    })],
                    encrypted_content: enc.clone(),
                })),
                Reasoning::Visible {
                    signature: None, ..
                } => {}
                Reasoning::Redacted { data } => input.push(Input::Reasoning(ReasoningInput {
                    summary: Vec::new(),
                    encrypted_content: data.clone(),
                })),
            },
            ReplyBlock::Reasoning(_) => {}
        }
    }
}

/// 无图时文本片段按行拼接；含图时按序发 input_text/input_image，相邻文本合并。
fn user_content(
    parts: Vec<ContentPart>,
    images: &HashMap<ImageId, ImageData>,
) -> Result<EasyInputContent, ProviderError> {
    if !has_image(&parts) {
        return Ok(EasyInputContent::Text(text_of(parts)?));
    }
    let mut out = Vec::with_capacity(parts.len());
    let mut texts: Vec<String> = Vec::new();
    let flush = |texts: &mut Vec<String>, out: &mut Vec<InputContent>| {
        if !texts.is_empty() {
            out.push(InputContent::InputText(InputTextContent {
                text: std::mem::take(texts).join("\n"),
                prompt_cache_breakpoint: None,
            }));
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
                out.push(InputContent::InputImage(InputImageContent {
                    image_url: Some(data_url(img)),
                    ..Default::default()
                }));
            }
        }
    }
    flush(&mut texts, &mut out);
    Ok(EasyInputContent::ContentList(out))
}

/// 增量只用于实时展示；终态 `response.output` 是唯一内容来源。
pub(crate) struct Accumulator;

impl Accumulate for Accumulator {
    type Event = ResponseStreamEvent;

    fn push(&mut self, event: ResponseStreamEvent) -> Result<Step, ProviderError> {
        use ResponseStreamEvent as E;
        Ok(match event {
            E::ResponseOutputTextDelta(e) => Step::Deltas(vec![ModelEvent::TextDelta(e.delta)]),
            E::ResponseRefusalDelta(e) => Step::Deltas(vec![ModelEvent::TextDelta(e.delta)]),
            E::ResponseReasoningSummaryTextDelta(e) => {
                Step::Deltas(vec![ModelEvent::ReasoningDelta(e.delta)])
            }
            E::ResponseCompleted(e) => Step::Finished(finish(e.response, None)?),
            E::ResponseIncomplete(e) => {
                let stop = match e
                    .response
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
                Step::Finished(finish(e.response, Some(stop))?)
            }
            E::ResponseFailed(e) => {
                return Err(match e.response.error {
                    Some(err) => upstream(Some(&code_str(&err.code)), &err.message),
                    None => protocol("response.failed 未给出错误"),
                })
            }
            E::ResponseError(e) => return Err(upstream(e.code.as_deref(), &e.message)),
            _ => Step::Deltas(Vec::new()),
        })
    }

    fn eof(self) -> Result<ModelResponse, ProviderError> {
        Err(transient("流在终态事件之前结束"))
    }
}

fn code_str(code: &ResponseErrorCode) -> String {
    match code {
        ResponseErrorCode::Other(s) => s.clone(),
        known => serde_json::to_value(known)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .expect("已知错误码序列化为字符串"),
    }
}

/// `stop` 为 `None` 时由输出推断：有工具调用即 ToolUse，有拒答即 ContentFilter。
fn finish(resp: Response, stop: Option<StopReason>) -> Result<ModelResponse, ProviderError> {
    let mut blocks = Vec::new();
    let mut refused = false;
    for item in resp.output {
        match item {
            OutputItem::Message(m) => {
                let mut text = String::new();
                for c in m.content {
                    match c {
                        OutputMessageContent::OutputText(t) => text.push_str(&t.text),
                        OutputMessageContent::Refusal(r) => {
                            refused = true;
                            text.push_str(&r.refusal);
                        }
                    }
                }
                if !text.is_empty() {
                    blocks.push(ReplyBlock::Text {
                        text,
                        phase: m.phase.map(|p| match p {
                            MessagePhase::Commentary => AssistantPhase::Commentary,
                            MessagePhase::FinalAnswer => AssistantPhase::FinalAnswer,
                        }),
                    });
                }
            }
            OutputItem::FunctionCall(c) => blocks.push(ReplyBlock::ToolCall {
                id: c.call_id,
                name: c.name,
                args: parse_arguments(c.arguments),
            }),
            OutputItem::Reasoning(r) => {
                if let Some(block) = reasoning(r) {
                    blocks.push(ReplyBlock::Reasoning(block));
                }
            }
            _ => return Err(protocol("收到未请求的输出项（如内置工具调用）")),
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
    Ok(ModelResponse {
        blocks,
        stop,
        usage: resp.usage.map(usage).transpose()?,
    })
}

fn reasoning(r: ReasoningItem) -> Option<Reasoning> {
    let summary: Vec<String> = r
        .summary
        .into_iter()
        .map(|SummaryPart::SummaryText(s)| s.text)
        .collect();
    if summary.is_empty() {
        return r.encrypted_content.map(|data| Reasoning::Redacted { data });
    }
    Some(Reasoning::Visible {
        text: summary.join("\n\n"),
        signature: r.encrypted_content,
    })
}

fn usage(u: ResponseUsage) -> Result<Usage, ProviderError> {
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
