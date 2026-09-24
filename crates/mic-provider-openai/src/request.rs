use mic_core::{ModelRequest, ProviderError};
use mic_message::{ContentPart, ModelView, Reasoning, ReplyBlock};

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

    for m in &req.messages {
        let Some(view) = m.model_view() else {
            continue;
        };
        messages.push(match view {
            ModelView::User(parts) => WireMessage::User {
                content: text_of(parts)?,
            },
            ModelView::Assistant { model, blocks } => {
                assistant(blocks, cfg.dialect, model == cfg.model)
            }
            ModelView::Tool {
                tool_call_id,
                output,
            } => WireMessage::Tool {
                tool_call_id,
                content: text_of(output)?,
            },
        });
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

/// 一条 `Reply` 即一个 assistant turn。`same_model`：该回复由本条目的模型产出。
fn assistant(blocks: &[ReplyBlock], dialect: Dialect, same_model: bool) -> WireMessage<'_> {
    let mut text = String::new();
    let mut reasoning = String::new();
    let mut tool_calls = Vec::new();
    for block in blocks {
        match block {
            ReplyBlock::Text { text: t } => text.push_str(t),
            ReplyBlock::Reasoning(Reasoning::Visible { text: r, .. }) => reasoning.push_str(r),
            // 本 crate 不产出密文推理；其它来源的密文无法回传给本协议。
            ReplyBlock::Reasoning(Reasoning::Redacted { .. }) => {}
            ReplyBlock::ToolCall { id, name, args } => tool_calls.push(WireToolCall {
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
        }
    }
    // 思考模式下工具往返必须回传推理；其余推理不发，省 token。
    let echo = dialect == Dialect::DeepSeek
        && same_model
        && !tool_calls.is_empty()
        && !reasoning.is_empty();
    WireMessage::Assistant {
        content: (!text.is_empty()).then_some(text),
        reasoning_content: echo.then_some(reasoning),
        tool_calls,
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
