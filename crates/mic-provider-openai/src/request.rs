use mic_core::{ModelRequest, ProviderError};
use std::collections::HashMap;

use base64::engine::general_purpose::STANDARD as BASE64;
use base64::Engine;
use mic_message::{ContentPart, FileRef, ImageData, ImageId, ModelView, Reasoning, ReplyBlock};

use crate::config::{Dialect, Resolved};
use crate::wire::{
    ChatRequest, ImageUrl, StreamOptions, UserContent, UserPart, WireFunction, WireFunctionCall,
    WireMessage, WireTool, WireToolCall,
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
                content: user_content(parts, &req.images)?,
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

/// 无图时文本片段按行拼接；含图时按序发 text/image_url 分片，相邻文本合并。
fn user_content(
    parts: Vec<ContentPart>,
    images: &HashMap<ImageId, ImageData>,
) -> Result<UserContent, ProviderError> {
    if !parts.iter().any(|p| matches!(p, ContentPart::Image(_))) {
        return Ok(UserContent::Text(text_of(parts)?));
    }
    let mut out = Vec::with_capacity(parts.len());
    let mut texts: Vec<String> = Vec::new();
    for part in parts {
        match part {
            ContentPart::Text { text } => texts.push(text),
            ContentPart::File(file) => return Err(unsupported_file(&file)),
            ContentPart::Image(r) => {
                if !texts.is_empty() {
                    out.push(UserPart::Text {
                        text: std::mem::take(&mut texts).join("\n"),
                    });
                }
                let img = images
                    .get(&r.id)
                    .expect("core 已按窗口引用加载全部图片原件");
                out.push(UserPart::ImageUrl {
                    image_url: ImageUrl {
                        url: format!(
                            "data:{};base64,{}",
                            img.format.mime(),
                            BASE64.encode(&img.bytes)
                        ),
                    },
                });
            }
        }
    }
    if !texts.is_empty() {
        out.push(UserPart::Text {
            text: texts.join("\n"),
        });
    }
    Ok(UserContent::Parts(out))
}

fn unsupported_file(file: &FileRef) -> ProviderError {
    ProviderError::Rejected {
        message: format!("该模型配置不支持文件内容（{}，{}）", file.path, file.mime),
    }
}

/// 工具结果与无图 user 消息：文本片段按行拼接，含文件或图片即拒绝。
fn text_of(parts: Vec<ContentPart>) -> Result<String, ProviderError> {
    let mut texts = Vec::with_capacity(parts.len());
    for part in parts {
        match part {
            ContentPart::Text { text } => texts.push(text),
            ContentPart::File(file) => return Err(unsupported_file(&file)),
            ContentPart::Image(_) => {
                return Err(ProviderError::Rejected {
                    message: "工具结果里的图片暂不支持".into(),
                })
            }
        }
    }
    Ok(texts.join("\n"))
}
