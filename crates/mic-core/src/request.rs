//! 由上下文窗口组装模型请求。契约：docs/blueprints/run-execution.md §4.4。

use std::collections::HashSet;

use mic_message::{Message, MessageBody, ModelView, ReplyBlock};
use mic_store::ContextWindow;
use mic_tool::ToolHandle;

use crate::ModelRequest;

const BASE_PROMPT: &str = "\
You are micnext, a personal assistant agent running on the user's own server. You act through tools with \
the full OS permissions of the micnext process: there is no sandbox and no approval step, so confirm with \
the user before destructive or irreversible actions.

Messages whose first line is a bracketed header come from the framework, not from the user: \
[user ...] is the user; [notification ...] and [runtime-note] are framework notices; tool results with \
[failed kind=...] or [cancelled] describe why a tool call did not succeed.

The user may send new messages while you are working; they appear after your latest tool results. \
Read them before continuing: they may add information, change the task, or ask you to stop. \
Tool calls in one response run in parallel, so only group calls that do not depend on each other. \
A running shell command is not interrupted by new messages; keep commands bounded and use timeouts. \
Reply in the language the user writes in. Be concise.";

pub(crate) fn build(window: ContextWindow, pwd: &str, tools: &[ToolHandle]) -> ModelRequest {
    ModelRequest {
        system: system_prompt(pwd, tools, window.summary.as_deref()),
        messages: order(window.messages),
        tools: tools.iter().map(|t| t.spec().clone()).collect(),
    }
}

/// 顺序固定，保证同一会话前缀稳定。
fn system_prompt(pwd: &str, tools: &[ToolHandle], summary: Option<&str>) -> String {
    let mut sections = vec![BASE_PROMPT.to_owned(), format!("Working directory: {pwd}")];
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
