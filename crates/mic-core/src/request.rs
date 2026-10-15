//! 由上下文窗口组装模型请求。契约：docs/blueprints/run-execution.md §4.4。

use std::collections::{HashMap, HashSet};
use std::sync::LazyLock;

use mic_message::{
    header_legend, ContentPart, ImageData, ImageId, Message, MessageBody, MessageId, ModelView,
    ReplyBlock,
};
use mic_store::{ContextWindow, RunSettings};
use mic_tool::ToolHandle;

use crate::{ModelRequest, ToolChoice};

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

const IMAGE_PLACEHOLDER: &str = "[图片：当前模型不支持查看]";

pub(crate) fn hide_images(window: &mut ContextWindow) {
    for m in &mut window.messages {
        if let MessageBody::UserInput { parts, .. } = &mut m.body {
            for part in parts
                .iter_mut()
                .filter(|p| matches!(p, ContentPart::Image(_)))
            {
                *part = ContentPart::Text {
                    text: IMAGE_PLACEHOLDER.into(),
                };
            }
        }
    }
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
    session: &mic_store::Session,
    window: ContextWindow,
    images: HashMap<ImageId, ImageData>,
    channel_prompt: Option<&str>,
    tools: &[ToolHandle],
    settings: &RunSettings,
    tail: &[MessageId],
) -> ModelRequest {
    ModelRequest {
        tool_choice: ToolChoice::Auto,
        system: system_prompt(
            settings,
            &session.pwd,
            channel_prompt,
            tools,
            window.summary.as_deref(),
            &window.files,
        ),
        messages: arrange(window.messages, tail),
        tools: tools.iter().map(|t| t.spec().clone()).collect(),
        images,
        cache_key: format!("micnext-s{}", session.id.0),
    }
}

/// 三块顺序固定：instructions → persona → context，保证同一会话前缀稳定。
/// 契约：docs/blueprints/system-prompt-layout.md §三。
pub(crate) fn system_prompt(
    settings: &RunSettings,
    pwd: &str,
    channel_prompt: Option<&str>,
    tools: &[ToolHandle],
    summary: Option<&str>,
    files: &[mic_message::FileSnapshot],
) -> String {
    let mut persona = vec![settings.persona.prompt.clone()];
    if !settings.general_prompt.is_empty() {
        persona.push(format!("User preferences:\n{}", settings.general_prompt));
    }
    let mut context = vec![format!("Working directory: {pwd}")];
    context.extend(channel_prompt.map(str::to_owned));
    context.extend(
        tools
            .iter()
            .filter_map(|t| t.prompt_hint())
            .map(str::to_owned),
    );
    if let Some(summary) = summary {
        context.push(format!("Summary of the earlier conversation:\n{summary}"));
    }
    for file in files {
        let content = match &file.outcome {
            mic_message::FileSnapshotOutcome::Read { text, extent } => {
                format!("{text}\nSnapshot extent: {extent:?}")
            }
            mic_message::FileSnapshotOutcome::Unavailable { reason } => {
                format!("Unavailable: {reason}")
            }
        };
        context.push(format!(
            "File snapshot at compaction time: {}\n{content}",
            file.path
        ));
    }
    [
        INSTRUCTIONS.clone(),
        block("persona", &persona.join("\n\n")),
        block("context", &context.join("\n\n")),
    ]
    .join("\n\n")
}

/// 请求里的消息顺序：先 `order`，再把本步待处理输入（`tail`）按原相对顺序挪到末尾。
/// 重建输入走同一函数。
pub(crate) fn arrange(messages: Vec<Message>, tail: &[MessageId]) -> Vec<Message> {
    let mut messages = order(messages);
    messages.sort_by_key(|m| tail.contains(&m.id));
    messages
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
