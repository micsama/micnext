use mic_core::{ModelRequest, ProviderError};
use mic_message::{ContentPart, MessageAuthor, MessageContent, ModelView, Reasoning};

use crate::config::{Dialect, Resolved};
use crate::wire::{
    ChatRequest, StreamOptions, WireFunction, WireFunctionCall, WireMessage, WireTool, WireToolCall,
};

/// 把一次调用映射为请求体（provider-openai §四.1）。
pub(crate) fn build<'a>(
    cfg: &'a Resolved,
    req: &'a ModelRequest,
) -> Result<ChatRequest<'a>, ProviderError> {
    let mut messages = Vec::with_capacity(req.messages.len() + 1);
    if !req.system.is_empty() {
        messages.push(WireMessage::System {
            content: &req.system,
        });
    }

    let mut turn: Option<AssistantTurn<'a>> = None;
    for m in &req.messages {
        let view = m.model_view();
        if let ModelView::Assistant(content) = view {
            let MessageAuthor::Assistant { model } = &m.author else {
                unreachable!("Assistant 视图只来自 Assistant 作者");
            };
            turn.get_or_insert_with(AssistantTurn::default)
                .push(content, model == &cfg.model);
            continue;
        }
        if let Some(t) = turn.take() {
            messages.push(t.finish(cfg.dialect));
        }
        messages.push(match view {
            ModelView::User(parts) => WireMessage::User {
                content: text_of(parts)?,
            },
            ModelView::Tool {
                tool_call_id,
                output,
            } => WireMessage::Tool {
                tool_call_id,
                content: text_of(output)?,
            },
            ModelView::Assistant(_) => unreachable!(),
        });
    }
    if let Some(t) = turn.take() {
        messages.push(t.finish(cfg.dialect));
    }

    Ok(ChatRequest {
        model: &cfg.model,
        messages,
        tools: req
            .tools
            .iter()
            .map(|t| WireTool {
                kind: "function",
                function: WireFunction {
                    name: &t.name,
                    description: &t.description,
                    parameters: &t.parameters,
                },
            })
            .collect(),
        stream: true,
        stream_options: StreamOptions {
            include_usage: true,
        },
        max_tokens: cfg.max_tokens,
        reasoning_effort: cfg.reasoning_effort.map(|e| e.as_str()),
    })
}

/// 连续 `Assistant` 视图合成的一个 assistant turn。
#[derive(Default)]
struct AssistantTurn<'a> {
    text: String,
    reasoning: String,
    tool_calls: Vec<WireToolCall<'a>>,
    /// 有条目不是本条目的模型产出的（换过模型）。
    other_model: bool,
}

impl<'a> AssistantTurn<'a> {
    fn push(&mut self, content: &'a MessageContent, same_model: bool) {
        self.other_model |= !same_model;
        match content {
            MessageContent::Text { content } => self.text.push_str(content),
            MessageContent::Reasoning(Reasoning::Visible { text, .. }) => {
                self.reasoning.push_str(text)
            }
            // 本 crate 不产出密文推理；其它来源的密文无法回传给本协议。
            MessageContent::Reasoning(Reasoning::Redacted { .. }) => {}
            MessageContent::ToolCall { id, name, args } => self.tool_calls.push(WireToolCall {
                id,
                kind: "function",
                function: WireFunctionCall {
                    name,
                    arguments: match args {
                        serde_json::Value::String(raw) => raw.clone(),
                        other => other.to_string(),
                    },
                },
            }),
            other => unreachable!("Assistant 条目不承载 {other:?}"),
        }
    }

    fn finish(self, dialect: Dialect) -> WireMessage<'a> {
        // 思考模式下工具往返必须回传推理；其余推理不发，省 token。
        let echo = dialect == Dialect::DeepSeek
            && !self.other_model
            && !self.tool_calls.is_empty()
            && !self.reasoning.is_empty();
        WireMessage::Assistant {
            content: (!self.text.is_empty()).then_some(self.text),
            reasoning_content: echo.then_some(self.reasoning),
            tool_calls: self.tool_calls,
        }
    }
}

/// 文本片段按行拼接；v0a 不支持多模态，含文件即拒绝。
fn text_of(parts: Vec<ContentPart>) -> Result<String, ProviderError> {
    let mut texts = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ContentPart::Text { text } => texts.push(text),
            ContentPart::File(file) => {
                return Err(ProviderError::Rejected {
                    message: format!("该模型配置不支持文件内容（{}，{}）", file.path, file.mime),
                })
            }
        }
    }
    Ok(texts.join("\n"))
}
