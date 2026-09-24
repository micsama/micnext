//! 启动时收尾遗留执行。契约：docs/blueprints/run-execution.md §4.6。
//! 在任何 worker 与订阅者出现之前运行，不发事件。

use mic_message::{ExecOutcome, MessageBody, ReplyBlock, SessionId, ToolResultOutcome};
use mic_store::{RunId, Store, StoreError};

use crate::run::{now_ms, NOTIFICATION_SOURCE};

const UNKNOWN_RESULT: &str =
    "micnext stopped before this tool call finished; its side effects are unknown.";
const INTERRUPTED: &str =
    "上次执行因 micnext 停止而中断，未完成的工具调用结果未知（可能已部分执行）。需要时请重新发送。";

/// 遗留 `Executing` → `Interrupted`，补齐悬空工具调用并通知；不重跑。
pub(crate) async fn recover(store: &Store) -> Result<usize, StoreError> {
    let stale = store.interrupt_stale_runs(now_ms()).await?;
    for run in &stale {
        let mut open: Vec<(String, String)> = Vec::new();
        for m in store.run_messages(run.id).await? {
            match m.body {
                MessageBody::Reply { blocks, .. } => {
                    open.extend(blocks.into_iter().filter_map(|b| match b {
                        ReplyBlock::ToolCall { id, name, .. } => Some((id, name)),
                        _ => None,
                    }))
                }
                MessageBody::ToolResult { tool_call_id, .. } => {
                    open.retain(|(id, _)| *id != tool_call_id)
                }
                _ => {}
            }
        }
        for (tool_call_id, tool_name) in open {
            append(
                store,
                run.session_id,
                run.id,
                MessageBody::ToolResult {
                    tool_name,
                    tool_call_id,
                    outcome: ToolResultOutcome::Terminal(ExecOutcome::Cancelled {
                        message: UNKNOWN_RESULT.into(),
                    }),
                },
            )
            .await?;
        }
        append(
            store,
            run.session_id,
            run.id,
            MessageBody::Notification {
                source: NOTIFICATION_SOURCE.into(),
                text: INTERRUPTED.into(),
            },
        )
        .await?;
    }
    Ok(stale.len())
}

async fn append(
    store: &Store,
    session_id: SessionId,
    run: RunId,
    body: MessageBody,
) -> Result<(), StoreError> {
    store.append(session_id, Some(run), body, now_ms()).await?;
    Ok(())
}
