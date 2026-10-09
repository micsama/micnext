//! 启动时收尾遗留执行。契约：docs/blueprints/run-execution.md §4.6。
//! 在任何 worker 与订阅者出现之前运行，不发事件。

use mic_message::{ExecOutcome, MessageBody, ReplyBlock, ToolResultOutcome};
use mic_store::{Store, StoreError};

use crate::run::{now_ms, NOTIFICATION_SOURCE};

const UNKNOWN_RESULT: &str =
    "micnext stopped before this tool call finished; its side effects are unknown.";

/// 遗留 `Executing` → `Interrupted`，补齐悬空工具调用并通知；不重跑。
/// 每个 run 原子收尾，中途停止则下次启动从未收尾的 run 继续。
pub(crate) async fn recover(store: &Store) -> Result<usize, StoreError> {
    let stale = store.executing_runs().await?;
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
        let text = if open.is_empty() {
            "上次执行已中断。"
        } else {
            "上次执行已中断，部分工具结果未知。"
        };
        let closing = open
            .into_iter()
            .map(|(tool_call_id, tool_name)| MessageBody::ToolResult {
                tool_name,
                tool_call_id,
                outcome: ToolResultOutcome::Terminal(ExecOutcome::Cancelled {
                    message: UNKNOWN_RESULT.into(),
                }),
            })
            .chain([MessageBody::Notification {
                source: NOTIFICATION_SOURCE.into(),
                text: text.into(),
                about: None,
            }])
            .collect();
        store.interrupt_run(run, closing, now_ms()).await?;
    }
    Ok(stale.len())
}
