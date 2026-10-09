//! 由上下文窗口组装模型请求。契约：docs/blueprints/run-execution.md §4.4。

use std::collections::{HashMap, HashSet};

use mic_message::{ContentPart, ImageData, ImageId, Message, MessageBody, ModelView, ReplyBlock};
use mic_store::{ContextWindow, RunSettings};
use mic_tool::ToolHandle;

use crate::ModelRequest;

const BASE_PROMPT: &str = include_str!("prompts/system.md");

/// 系统块，设置页只读展示。
pub(crate) fn base_prompt() -> &'static str {
    BASE_PROMPT.trim_end()
}

/// 窗口内消息引用的图片 id。
pub(crate) fn image_ids(window: &ContextWindow) -> Vec<ImageId> {
    window
        .messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::UserInput { parts, .. } => Some(parts),
            _ => None,
        })
        .flatten()
        .filter_map(|p| match p {
            ContentPart::Image(r) => Some(r.id),
            _ => None,
        })
        .collect()
}

pub(crate) fn build(
    window: ContextWindow,
    images: HashMap<ImageId, ImageData>,
    pwd: &str,
    tools: &[ToolHandle],
    settings: &RunSettings,
) -> ModelRequest {
    ModelRequest {
        system: system_prompt(settings, pwd, tools, window.summary.as_deref()),
        messages: order(window.messages),
        tools: tools.iter().map(|t| t.spec().clone()).collect(),
        images,
    }
}

/// 顺序固定：系统 → 人设 → 通用偏好 → 工作目录 → 工具提示 → 摘要，保证同一会话前缀稳定。
fn system_prompt(
    settings: &RunSettings,
    pwd: &str,
    tools: &[ToolHandle],
    summary: Option<&str>,
) -> String {
    let mut sections = vec![base_prompt().to_owned(), settings.persona.prompt.clone()];
    if !settings.general_prompt.is_empty() {
        sections.push(format!("User preferences:\n{}", settings.general_prompt));
    }
    sections.push(format!("Working directory: {pwd}"));
    sections.extend(
        tools
            .iter()
            .filter_map(|t| t.prompt_hint())
            .map(str::to_owned),
    );
    if let Some(summary) = summary {
        sections.push(format!("Summary of the earlier conversation:\n{summary}"));
    }
    sections.join("\n\n")
}

/// 有工具调用未结时，user 视图消息待其结果全部到齐后按原顺序放回，保证调用与结果相邻。
/// 未认领输入已由 store 排除。
fn order(messages: Vec<Message>) -> Vec<Message> {
    let mut out = Vec::with_capacity(messages.len());
    let mut open: HashSet<String> = HashSet::new();
    let mut held = Vec::new();
    for m in messages {
        match &m.body {
            MessageBody::Reply { blocks, .. } => {
                open.extend(blocks.iter().filter_map(|b| match b {
                    ReplyBlock::ToolCall { id, .. } => Some(id.clone()),
                    _ => None,
                }));
                out.push(m);
            }
            MessageBody::ToolResult { tool_call_id, .. } => {
                open.remove(tool_call_id);
                out.push(m);
                if open.is_empty() {
                    out.append(&mut held);
                }
            }
            _ if !open.is_empty() && matches!(m.model_view(), Some(ModelView::User(_))) => {
                held.push(m)
            }
            _ => out.push(m),
        }
    }
    out.append(&mut held);
    out
}
