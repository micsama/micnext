//! 启动时收尾遗留执行。契约：docs/blueprints/query-execution.md §4.6。
//! 在任何 worker 与订阅者出现之前运行，不发事件。

use mic_message::{
    ExecOutcome, MessageAuthor, MessageContent, SessionEntry, SessionEntryId, ToolResultOutcome,
};
use mic_store::{OutputInput, Store, StoreError};

use crate::query::{notification, now_ms};

const UNKNOWN_RESULT: &str =
    "micnext stopped before this tool call finished; its side effects are unknown.";
const INTERRUPTED: &str =
    "上次执行因 micnext 停止而中断，未完成的工具调用结果未知（可能已部分执行）。需要时请重新发送。";

/// 遗留 `Executing` → `Failed{Interrupted}`，补齐悬空工具调用并通知；不重跑。
pub(crate) async fn recover(store: &Store) -> Result<usize, StoreError> {
    let stale = store.interrupt_stale_queries(now_ms()).await?;
    for query in &stale {
        let after = SessionEntryId(query.claimed_start_id.0 - 1);
        let mut open: Vec<(String, String)> = Vec::new();
        for entry in store.entries_after(query.session_id, Some(after)).await? {
            let SessionEntry::Message(m) = entry else {
                continue;
            };
            match m.content {
                MessageContent::ToolCall { id, name, .. } => open.push((id, name)),
                MessageContent::ToolResult { tool_call_id, .. } => {
                    open.retain(|(id, _)| *id != tool_call_id)
                }
                _ => {}
            }
        }
        for (id, name) in open {
            append(
                store,
                query.session_id,
                MessageAuthor::Tool { name },
                MessageContent::ToolResult {
                    tool_call_id: id,
                    outcome: ToolResultOutcome::Terminal(ExecOutcome::Cancelled {
                        message: UNKNOWN_RESULT.into(),
                    }),
                },
            )
            .await?;
        }
        append(
            store,
            query.session_id,
            notification(),
            MessageContent::Text {
                content: INTERRUPTED.into(),
            },
        )
        .await?;
    }
    Ok(stale.len())
}

async fn append(
    store: &Store,
    session_id: mic_message::SessionId,
    author: MessageAuthor,
    content: MessageContent,
) -> Result<(), StoreError> {
    store
        .append_output(OutputInput {
            session_id,
            author,
            content,
            created_at: now_ms(),
        })
        .await?;
    Ok(())
}
