//! 历史条目在模型请求里的呈现；所有 Provider 共用，规则只此一处。
//! 契约：docs/blueprints/provider-port.md §三.5、docs/blueprints/mic-tool.md §四.3。

use chrono::{Local, SecondsFormat, TimeZone};

use crate::{ContentPart, ExecOutcome, Message, MessageBody, ReplyBlock, ToolResultOutcome};

/// 一条历史消息在模型请求里的呈现。
#[derive(Debug, Clone, PartialEq)]
pub enum ModelView<'a> {
    /// user role。首个片段是 `Text`，以方括号头开头。
    User(Vec<ContentPart>),
    /// 一条 `Reply` 即一个 assistant turn；`model` 为请求模型名。
    Assistant {
        model: &'a str,
        blocks: &'a [ReplyBlock],
    },
    /// 工具结果，对应 `tool_call_id` 的那次调用。
    Tool {
        tool_call_id: &'a str,
        output: Vec<ContentPart>,
    },
}

impl Message {
    /// `Boundary` 不进上下文，返回 `None`。
    pub fn model_view(&self) -> Option<ModelView<'_>> {
        Some(match &self.body {
            MessageBody::UserInput { person, parts } => ModelView::User(with_header(
                format!("[user id={} at={}]", person.0, local_time(self.created_at)),
                parts.clone(),
            )),
            MessageBody::Reply { model, blocks } => ModelView::Assistant { model, blocks },
            MessageBody::ToolResult {
                tool_call_id,
                outcome,
                ..
            } => ModelView::Tool {
                tool_call_id,
                output: match outcome {
                    ToolResultOutcome::Terminal(outcome) => outcome_parts(outcome),
                    ToolResultOutcome::Dispatched { exec_id } => vec![text(format!(
                        "[dispatched exec_id={}]\nThe result will arrive later as a completion.",
                        header_value(exec_id)
                    ))],
                },
            },
            MessageBody::Completion {
                exec_id, outcome, ..
            } => ModelView::User(with_header(
                format!("[completion exec_id={}]", header_value(exec_id)),
                outcome_parts(outcome),
            )),
            MessageBody::HarnessNote { text: note } => {
                ModelView::User(vec![text(format!("[runtime-note]\n{note}"))])
            }
            MessageBody::Notification { source, text: note } => {
                ModelView::User(vec![text(format!(
                    "[notification src={} at={}]\n{note}",
                    header_value(source),
                    local_time(self.created_at)
                ))])
            }
            MessageBody::Boundary { .. } => return None,
        })
    }
}

fn outcome_parts(outcome: &ExecOutcome) -> Vec<ContentPart> {
    match outcome {
        ExecOutcome::Completed { output } => output.clone(),
        ExecOutcome::Failed { kind, message } => {
            vec![text(format!("[failed kind={}]\n{message}", kind.as_str()))]
        }
        ExecOutcome::Cancelled { message } => vec![text(format!("[cancelled]\n{message}"))],
    }
}

/// 头独占首行：并入首个文本片段，首片段不是文本时单独成片。
fn with_header(header: String, mut parts: Vec<ContentPart>) -> Vec<ContentPart> {
    match parts.first_mut() {
        Some(ContentPart::Text { text }) => text.insert_str(0, &format!("{header}\n")),
        _ => parts.insert(0, self::text(header)),
    }
    parts
}

fn text(text: String) -> ContentPart {
    ContentPart::Text { text }
}

/// 头部取值里的 `]` 与控制字符替换为 `_`。
fn header_value(v: &str) -> String {
    v.chars()
        .map(|c| if c == ']' || c.is_control() { '_' } else { c })
        .collect()
}

/// 带本地时区偏移的 RFC 3339，秒精度。
fn local_time(unix_ms: i64) -> String {
    Local
        .timestamp_millis_opt(unix_ms)
        .single()
        .expect("created_at 为合法 unix 毫秒")
        .to_rfc3339_opts(SecondsFormat::Secs, false)
}
