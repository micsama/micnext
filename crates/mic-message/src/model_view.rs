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
            MessageBody::UserInput { person, parts } => {
                let (id, at) = (person.0.to_string(), local_time(self.created_at));
                ModelView::User(with_header(
                    Header::User { id: &id, at: &at }.render(),
                    parts.clone(),
                ))
            }
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
                        "{}\nThe result will arrive later as a completion.",
                        Header::Dispatched { exec_id }.render()
                    ))],
                },
            },
            MessageBody::Completion {
                exec_id, outcome, ..
            } => ModelView::User(with_header(
                Header::Completion { exec_id }.render(),
                outcome_parts(outcome),
            )),
            MessageBody::HarnessNote { text: note } => ModelView::User(vec![text(format!(
                "{}\n{note}",
                Header::RuntimeNote.render()
            ))]),
            MessageBody::Notification {
                source, text: note, ..
            } => {
                let at = local_time(self.created_at);
                let header = Header::Notification {
                    src: source,
                    at: &at,
                }
                .render();
                ModelView::User(vec![text(format!("{header}\n{note}"))])
            }
            MessageBody::Boundary { .. } => return None,
        })
    }
}

fn outcome_parts(outcome: &ExecOutcome) -> Vec<ContentPart> {
    match outcome {
        ExecOutcome::Completed { output } => output.clone(),
        ExecOutcome::Failed { kind, message } => {
            let header = Header::Failed {
                kind: kind.as_str(),
            }
            .render();
            vec![text(format!("{header}\n{message}"))]
        }
        ExecOutcome::Cancelled { message } => {
            vec![text(format!("{}\n{message}", Header::Cancelled.render()))]
        }
    }
}

/// 模型可见的方括号头：标签、属性名与说明只在此定义。
#[derive(Clone, Copy)]
enum Header<'a> {
    User { id: &'a str, at: &'a str },
    RuntimeNote,
    Notification { src: &'a str, at: &'a str },
    Completion { exec_id: &'a str },
    Dispatched { exec_id: &'a str },
    Failed { kind: &'a str },
    Cancelled,
}

/// 说明清单；新增 `Header` 变体须同步此处。
const ALL: [Header<'static>; 7] = [
    Header::User {
        id: "…", at: "…"
    },
    Header::RuntimeNote,
    Header::Notification {
        src: "…", at: "…"
    },
    Header::Dispatched { exec_id: "…" },
    Header::Completion { exec_id: "…" },
    Header::Failed { kind: "…" },
    Header::Cancelled,
];

impl<'a> Header<'a> {
    fn tag(self) -> &'static str {
        match self {
            Self::User { .. } => "user",
            Self::RuntimeNote => "runtime-note",
            Self::Notification { .. } => "notification",
            Self::Completion { .. } => "completion",
            Self::Dispatched { .. } => "dispatched",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
        }
    }

    fn fields(self) -> Vec<(&'static str, &'a str)> {
        match self {
            Self::User { id, at } => vec![("id", id), ("at", at)],
            Self::Notification { src, at } => vec![("src", src), ("at", at)],
            Self::Completion { exec_id } | Self::Dispatched { exec_id } => {
                vec![("exec_id", exec_id)]
            }
            Self::Failed { kind } => vec![("kind", kind)],
            Self::RuntimeNote | Self::Cancelled => vec![],
        }
    }

    /// 分类语义归 `ExecFailureKind`，改分类时同步 `Failed`。
    fn meaning(self) -> &'static str {
        match self {
            Self::User { .. } => "a user message.",
            Self::RuntimeNote => "a framework notice, not the user.",
            Self::Notification { .. } => "an event from src at that time, not the user; it may no longer hold now.",
            Self::Dispatched { .. } => {
                "a background tool started; its result arrives as a completion."
            }
            Self::Completion { .. } => "the result of that background tool.",
            Self::Failed { .. } => {
                "the tool call failed (input: fix the arguments; business: rejected by tool rules; dependency: environment error)."
            }
            Self::Cancelled => {
                "the call was cancelled or interrupted; its effect may be unknown, so check before retrying."
            }
        }
    }

    fn render(self) -> String {
        let mut out = format!("[{}", self.tag());
        for (k, v) in self.fields() {
            out.push_str(&format!(" {k}={}", header_value(v)));
        }
        out.push(']');
        out
    }
}

/// 全部消息头的说明，供 system prompt 使用；英文，多行，无首尾空行。
pub fn header_legend() -> String {
    let mut out = String::from("Bracketed headers are added by the framework:");
    for h in ALL {
        out.push_str(&format!("\n- {}: {}", h.render(), h.meaning()));
    }
    out
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
