//! 历史条目在模型请求里的呈现；所有 Provider 共用，规则只此一处。
//! 契约：docs/blueprints/provider-port.md §三.5、docs/blueprints/mic-tool.md §四.3。

use chrono::{Local, SecondsFormat, TimeZone};

use crate::{
    ContentPart, ExecOutcome, Message, MessageBody, ReplyBlock, SessionCommand, ToolResultOutcome,
};

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
    /// `Attachment`、`Boundary`、`SideOutput` 不进上下文，返回 `None`。
    pub fn model_view(&self) -> Option<ModelView<'_>> {
        Some(match &self.body {
            MessageBody::Command {
                person,
                command: SessionCommand::Compact { text: Some(value) },
            } => ModelView::User(with_header(
                Header::User {
                    id: &person.0.to_string(),
                    at: &local_time(self.created_at),
                }
                .render(),
                vec![text(value.clone())],
            )),
            MessageBody::Command { .. } => return None,
            MessageBody::UserInput { person, parts } => {
                let (id, at) = (person.0.to_string(), local_time(self.created_at));
                ModelView::User(with_header(
                    Header::User { id: &id, at: &at }.render(),
                    parts.iter().map(user_part).collect(),
                ))
            }
            MessageBody::Reply { model, blocks, .. } => ModelView::Assistant { model, blocks },
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
            MessageBody::JobNotice {
                exec_id,
                text: note,
                ..
            } => ModelView::User(vec![text(format!(
                "{}\n{note}",
                Header::JobNotice { exec_id }.render()
            ))]),
            MessageBody::Trigger { source, text: note } => {
                let at = local_time(self.created_at);
                let header = Header::Trigger {
                    src: source,
                    at: &at,
                }
                .render();
                ModelView::User(vec![text(format!("{header}\n{note}"))])
            }
            MessageBody::HarnessNote { text: note } => ModelView::User(vec![text(format!(
                "{}\n{note}",
                Header::RuntimeNote.render()
            ))]),
            MessageBody::Notification {
                source,
                text: note,
                files,
            } => {
                let at = local_time(self.created_at);
                let header = Header::Notification {
                    src: source,
                    at: &at,
                }
                .render();
                let mut parts = vec![text(format!("{header}\n{note}"))];
                parts.extend(
                    files
                        .iter()
                        .map(|f| user_part(&ContentPart::File(f.clone()))),
                );
                ModelView::User(parts)
            }
            MessageBody::Attachment { .. }
            | MessageBody::Boundary { .. }
            | MessageBody::SideOutput { .. } => return None,
        })
    }
}

impl Message {
    /// 换成占位：`Completed` 工具输出整段换掉（原长按字符计），用户输入只换 skill 正文。
    pub fn elide_output(&mut self) {
        let (tool_name, output) = match &mut self.body {
            MessageBody::ToolResult {
                tool_name,
                outcome: ToolResultOutcome::Terminal(ExecOutcome::Completed { output }),
                ..
            }
            | MessageBody::Completion {
                tool_name,
                outcome: ExecOutcome::Completed { output },
                ..
            } => (tool_name, output),
            MessageBody::UserInput { parts, .. } => {
                for part in parts {
                    if let ContentPart::Skill { name, .. } = part {
                        *part = text(format!(
                            "[Skill {name} was loaded here; its body is omitted. Load it again if needed.]"
                        ));
                    }
                }
                return;
            }
            _ => return,
        };
        let chars: usize = output
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => text.chars().count(),
                _ => 0,
            })
            .sum();
        *output = vec![text(format!(
            "[Output of {tool_name} omitted ({chars} chars). Call it again if needed.]"
        ))];
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
        ExecOutcome::TimedOut { output } => {
            let mut parts = vec![text(Header::TimedOut.render())];
            parts.extend(output.clone());
            parts
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
    Trigger { src: &'a str, at: &'a str },
    Completion { exec_id: &'a str },
    JobNotice { exec_id: &'a str },
    Dispatched { exec_id: &'a str },
    Failed { kind: &'a str },
    Cancelled,
    TimedOut,
}

/// 说明清单；新增 `Header` 变体须同步此处。
const ALL: [Header<'static>; 10] = [
    Header::User {
        id: "…", at: "…"
    },
    Header::RuntimeNote,
    Header::Notification {
        src: "…", at: "…"
    },
    Header::Trigger {
        src: "…", at: "…"
    },
    Header::Dispatched { exec_id: "…" },
    Header::Completion { exec_id: "…" },
    Header::JobNotice { exec_id: "…" },
    Header::Failed { kind: "…" },
    Header::Cancelled,
    Header::TimedOut,
];

impl<'a> Header<'a> {
    fn tag(self) -> &'static str {
        match self {
            Self::User { .. } => "user",
            Self::RuntimeNote => "runtime-note",
            Self::Notification { .. } => "notification",
            Self::Trigger { .. } => "trigger",
            Self::Completion { .. } => "completion",
            Self::JobNotice { .. } => "job-notice",
            Self::Dispatched { .. } => "dispatched",
            Self::Failed { .. } => "failed",
            Self::Cancelled => "cancelled",
            Self::TimedOut => "timed_out",
        }
    }

    fn fields(self) -> Vec<(&'static str, &'a str)> {
        match self {
            Self::User { id, at } => vec![("id", id), ("at", at)],
            Self::Notification { src, at } | Self::Trigger { src, at } => {
                vec![("src", src), ("at", at)]
            }
            Self::Completion { exec_id }
            | Self::JobNotice { exec_id }
            | Self::Dispatched { exec_id } => {
                vec![("job_id", exec_id)]
            }
            Self::Failed { kind } => vec![("kind", kind)],
            Self::RuntimeNote | Self::Cancelled | Self::TimedOut => vec![],
        }
    }

    /// 分类语义归 `ExecFailureKind`，改分类时同步 `Failed`。
    fn meaning(self) -> &'static str {
        match self {
            Self::User { .. } => "a user message.",
            Self::RuntimeNote => "a framework notice, not the user.",
            Self::Notification { .. } => "an event from src at that time, not the user; it may no longer hold now.",
            Self::Trigger { .. } => {
                "a scheduled instruction from task src, not from the user."
            }
            Self::Dispatched { .. } => {
                "a background job or sub-agent was dispatched; its result arrives as a completion."
            }
            Self::Completion { .. } => {
                "the result of that background job; for a sub-agent, the delivery of one of its runs (each follow-up delivers again)."
            }
            Self::JobNotice { .. } => {
                "a notice from that background job while it is still running; the job has not necessarily ended, and its result still arrives as a completion."
            }
            Self::Failed { .. } => {
                "the tool call failed (input: fix the arguments; business: rejected by tool rules; dependency: environment error)."
            }
            Self::TimedOut => "the operation reached its time limit; partial output is preserved. Check its effects before retrying.",
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

/// 文件只给路径，模型按需用工具读取；skill 渲染成文本块。Provider 因此不会见到 `File`、`Skill`。
fn user_part(part: &ContentPart) -> ContentPart {
    match part {
        ContentPart::File(file) => text(format!(
            "[Attached file saved at {} ({} bytes)]",
            file.path, file.size_bytes
        )),
        ContentPart::Skill { name, text: body } => {
            text(format!("<skill name=\"{name}\">\n{body}\n</skill>"))
        }
        other => other.clone(),
    }
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
