//! 由上下文窗口组装模型请求。契约：docs/blueprints/run-execution.md §4.4。

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use mic_message::{
    header_legend, ContentPart, ImageData, ImageId, Message, MessageBody, ModelView, ReplyBlock,
};
use mic_store::{ContextWindow, RunSettings};
use mic_tool::ToolHandle;

use crate::ModelRequest;

const BASE_PROMPT: &str = include_str!("prompts/system.md");

static INSTRUCTIONS: LazyLock<String> = LazyLock::new(|| {
    block(
        "instructions",
        &format!("{}\n\n{}", BASE_PROMPT.trim_end(), header_legend()),
    )
});

/// `instructions` 块，设置页只读展示。
pub(crate) fn base_prompt() -> &'static str {
    &INSTRUCTIONS
}

fn block(tag: &str, body: &str) -> String {
    format!("<{tag}>\n{body}\n</{tag}>")
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

/// 三块顺序固定：instructions → persona → context，保证同一会话前缀稳定。
/// 契约：docs/blueprints/system-prompt-layout.md §三。
fn system_prompt(
    settings: &RunSettings,
    pwd: &str,
    tools: &[ToolHandle],
    summary: Option<&str>,
) -> String {
    let mut persona = vec![settings.persona.prompt.clone()];
    if !settings.general_prompt.is_empty() {
        persona.push(format!("User preferences:\n{}", settings.general_prompt));
    }
    let mut context = vec![format!("Working directory: {pwd}")];
    context.extend(
        tools
            .iter()
            .filter_map(|t| t.prompt_hint())
            .map(str::to_owned),
    );
    if let Some(summary) = summary {
        context.push(format!("Summary of the earlier conversation:\n{summary}"));
    }
    [
        INSTRUCTIONS.clone(),
        block("persona", &persona.join("\n\n")),
        block("context", &context.join("\n\n")),
    ]
    .join("\n\n")
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
