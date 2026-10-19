//! 启动时收尾遗留执行。契约：docs/blueprints/run-execution.md §4.6。
//! 在任何 worker 与订阅者出现之前运行，不发事件。

use std::collections::BTreeMap;
use std::path::PathBuf;

use mic_message::{ExecOutcome, MessageBody, ReplyBlock, SessionId, ToolResultOutcome};
use mic_store::{InterruptedRun, SessionKind, Store, StoreError};

use crate::jobs::AGENT_INTERRUPTED;
use crate::run::{now_ms, NOTIFICATION_SOURCE};

const UNKNOWN_RESULT: &str =
    "micnext stopped before this tool call finished; its side effects are unknown.";

/// 遗留 `Executing` → `Interrupted`，补齐悬空工具调用并通知；不重跑。已登记后台任务的调用闭合为
/// `Dispatched`，子 run 向父会话交付中断；随后遗留的后台任务记中断并回注。被中断的子 agent 与
/// 后台任务在树根汇成一条通知。
/// 每棵会话树原子收尾，中途停止则下次启动从未收尾处继续。
pub(crate) async fn recover(store: &Store, data_dir: PathBuf) -> Result<usize, StoreError> {
    let stale = store.executing_runs().await?;
    let count = stale.len();
    let mut trees: BTreeMap<SessionId, Tree> = BTreeMap::new();
    for job in store.running_jobs(None).await? {
        let root = store.session_root(job.owner_session_id).await?.id;
        let label = format!("{} #{}", job.tool_name, job.id.0);
        trees.entry(root).or_default().jobs.push((job.id.0, label));
    }
    for run in stale {
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
        let mut closing = Vec::new();
        for (tool_call_id, tool_name) in open {
            let outcome = match store.job_by_origin(run.id, &tool_call_id).await? {
                Some(job) => ToolResultOutcome::Dispatched {
                    exec_id: job.0.to_string(),
                },
                None => ToolResultOutcome::Terminal(ExecOutcome::Cancelled {
                    message: UNKNOWN_RESULT.into(),
                }),
            };
            closing.push(MessageBody::ToolResult {
                tool_name,
                tool_call_id,
                outcome,
                started_at: None,
            });
        }
        closing.push(MessageBody::Notification {
            source: NOTIFICATION_SOURCE.into(),
            text: text.into(),
        });
        let session = store
            .session(run.session_id)
            .await?
            .expect("run 的会话必然存在");
        let root = store.session_root(session.id).await?.id;
        let Tree { runs, jobs: agents } = trees.entry(root).or_default();
        let report = match session.kind {
            SessionKind::Task { .. } => {
                let job = store
                    .agent_job(session.id)
                    .await?
                    .expect("Task 会话必有 agent job");
                agents.push((job.id.0, format!("子 agent #{}", job.id.0)));
                Some(ExecOutcome::Cancelled {
                    message: AGENT_INTERRUPTED.into(),
                })
            }
            _ => None,
        };
        runs.push(InterruptedRun {
            run,
            closing,
            report,
        });
    }
    for (root, Tree { runs, mut jobs }) in trees {
        jobs.sort_unstable();
        let notice = (!jobs.is_empty()).then(|| {
            let labels: Vec<&str> = jobs.iter().map(|(_, label)| label.as_str()).collect();
            (
                root,
                MessageBody::Notification {
                    source: NOTIFICATION_SOURCE.into(),
                    text: format!("重启前仍在运行的后台任务已中断：{}。", labels.join("、")),
                },
            )
        });
        store.interrupt_runs(runs, notice, now_ms()).await?;
    }
    crate::jobs::recover(store, data_dir).await?;
    Ok(count)
}

/// 一棵会话树的收尾：要收尾的 run，与被中断的子 agent、后台任务（任务号, 显示名）。
#[derive(Default)]
struct Tree {
    runs: Vec<InterruptedRun>,
    jobs: Vec<(i64, String)>,
}
