use std::collections::BTreeMap;
use std::time::Duration;

use mic_core::{ModelEvent, ModelResponse, ProviderError, StopReason};
use mic_message::{Reasoning, ReplyBlock};
use mic_store::Usage;
use reqwest::StatusCode;

use crate::config::Dialect;
use crate::limits::ERROR_BODY_CHARS;
use crate::wire::{Chunk, ErrorBody, WireUsage};

/// 把分片累积为一次完整响应（provider-openai §四.3）。
pub(crate) struct Accumulator {
    dialect: Dialect,
    /// 收到过分片；`[DONE]` 之前一片没有即协议错误。
    started: bool,
    reasoning: String,
    text: String,
    /// 按上游 `index` 排序。
    tool_calls: BTreeMap<u32, PendingCall>,
    finish_reason: Option<String>,
    usage: Option<WireUsage>,
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
            started: false,
            reasoning: String::new(),
            text: String::new(),
            tool_calls: BTreeMap::new(),
            finish_reason: None,
            usage: None,
        }
    }

    /// 返回本分片产出的实时增量。
    pub(crate) fn push(&mut self, chunk: Chunk) -> Result<Vec<ModelEvent>, ProviderError> {
        self.started = true;
        if let Some(usage) = chunk.usage {
            self.usage = Some(usage);
        }
        let mut events = Vec::new();
        for choice in chunk.choices {
            if choice.index != 0 {
                return Err(protocol(format!("收到多个候选（index {}）", choice.index)));
            }
            let delta = choice.delta;
            let reasoning = match self.dialect {
                Dialect::DeepSeek => delta.reasoning_content,
                Dialect::Ollama => delta.reasoning,
                Dialect::Generic => None,
            };
            let has_delta = reasoning.as_deref().is_some_and(|s| !s.is_empty())
                || delta.content.as_deref().is_some_and(|s| !s.is_empty())
                || !delta.tool_calls.is_empty();
            if has_delta && self.finish_reason.is_some() {
                return Err(protocol("finish_reason 之后仍有内容增量".into()));
            }
            if let Some(r) = reasoning.filter(|s| !s.is_empty()) {
                self.reasoning.push_str(&r);
                events.push(ModelEvent::ReasoningDelta(r));
            }
            if let Some(t) = delta.content.filter(|s| !s.is_empty()) {
                self.text.push_str(&t);
                events.push(ModelEvent::TextDelta(t));
            }
            for call in delta.tool_calls {
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
        Ok(events)
    }

    /// 收到 `[DONE]` 后组装最终结果。
    pub(crate) fn finish(self) -> Result<ModelResponse, ProviderError> {
        if !self.started {
            return Err(protocol("[DONE] 之前没有任何分片".into()));
        }
        let stop = match self.finish_reason.as_deref() {
            Some("stop") => StopReason::EndTurn,
            Some("tool_calls") => StopReason::ToolUse,
            Some("length") => StopReason::MaxTokens,
            Some("content_filter") => StopReason::ContentFilter,
            Some("insufficient_system_resource") => {
                return Err(transient("上游资源不足，回复被中断".into(), None))
            }
            Some(other) => return Err(protocol(format!("未知的 finish_reason `{other}`"))),
            None => return Err(protocol("流结束时没有 finish_reason".into())),
        };

        let mut blocks = Vec::new();
        if !self.reasoning.is_empty() {
            blocks.push(ReplyBlock::Reasoning(Reasoning::Visible {
                text: self.reasoning,
                signature: None,
            }));
        }
        if !self.text.is_empty() {
            blocks.push(ReplyBlock::Text { text: self.text });
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
        if stop == StopReason::EndTurn && blocks.is_empty() {
            return Err(transient("上游返回了空回复".into(), None));
        }

        Ok(ModelResponse {
            blocks,
            stop,
            usage: self.usage.map(|u| usage(self.dialect, u)),
        })
    }
}

/// 只有 JSON 对象才存为结构化值；其余原文保留，由 mic-tool 边界报参数错误给模型。
fn parse_arguments(raw: String) -> serde_json::Value {
    match serde_json::from_str::<serde_json::Value>(&raw) {
        Ok(v @ serde_json::Value::Object(_)) => v,
        _ => serde_json::Value::String(raw),
    }
}

fn usage(dialect: Dialect, u: WireUsage) -> Usage {
    let cache_read_tokens = match dialect {
        Dialect::DeepSeek => u.prompt_cache_hit_tokens,
        Dialect::Ollama => None,
        Dialect::Generic => u.prompt_tokens_details.and_then(|d| d.cached_tokens),
    };
    Usage {
        input_tokens: u.prompt_tokens,
        output_tokens: u.completion_tokens,
        cache_read_tokens,
        cache_write_tokens: None,
        reasoning_tokens: u.completion_tokens_details.and_then(|d| d.reasoning_tokens),
    }
}

/// 非 2xx 响应的分类（provider-openai §四.4）。
pub(crate) fn http_error(
    status: StatusCode,
    retry_after: Option<Duration>,
    body: &str,
) -> ProviderError {
    let upstream = match serde_json::from_str::<ErrorBody>(body) {
        Ok(b) => b.error.message,
        Err(_) => body.chars().take(ERROR_BODY_CHARS).collect(),
    };
    let detail = |note: &str| format!("{note}（HTTP {}）：{upstream}", status.as_u16());
    match status.as_u16() {
        401 | 403 => ProviderError::Account {
            message: detail("key 无效或无权限"),
        },
        402 => ProviderError::Account {
            message: detail("余额不足，请充值"),
        },
        429 => transient(detail("请求过于频繁"), retry_after),
        500..=599 => transient(detail("服务端错误"), retry_after),
        _ => ProviderError::Rejected {
            message: detail("请求被拒绝"),
        },
    }
}

pub(crate) fn protocol(message: String) -> ProviderError {
    ProviderError::Protocol { message }
}

pub(crate) fn transient(message: String, retry_after: Option<Duration>) -> ProviderError {
    ProviderError::Transient {
        message,
        retry_after,
    }
}
