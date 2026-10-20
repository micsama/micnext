//! 子 agent：`agent` 工具（派出、追问）、子 run 的交付、停止。

use std::any::Any;
use std::sync::Arc;

use mic_message::{
    ContentPart, ExecFailureKind, ExecOutcome, Message, MessageBody, ReplyBlock, SessionId,
};
use mic_store::{Job, JobId, JobKind, NewAgentJob, RunState, Session, StoreError, ToolScope};
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{tree, JobPort, Jobs, Origin, StopJobOutcome};
use crate::control::StopBy;
use crate::limits::{AGENT_DELIVERY_MAX, AGENT_RUN_TIMEOUT};
use crate::run::now_ms;

#[cfg(test)]
mod tests;

/// 子 agent 可用登记工具所在的模块。
pub(crate) const AGENT_MODULES: [&str; 3] = ["fs", "shell", "web_fetch"];
/// core 生成的工具名；登记工具不得重名。
pub(crate) const CORE_TOOLS: [&str; 5] = ["jobs", "job_output", "job_stop", "agent", "push"];
pub(crate) const AGENT_INTERRUPTED: &str = "micnext stopped while the sub-agent was running.";

pub(super) fn handle(jobs: &Arc<Jobs>, session: &Session) -> ToolHandle {
    ToolHandle::new(AgentTool {
        jobs: jobs.clone(),
        scope: session.tool_scope.clone(),
    })
}

struct AgentTool {
    jobs: Arc<Jobs>,
    /// 调用会话的工具范围；子 agent 不超出它。
    scope: ToolScope,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct AgentArgs {
    /// The task for a new sub-agent, or the follow-up instruction when job_id is given. The sub-agent sees none of this conversation, so include every fact, path and constraint it needs.
    task: String,
    /// Short name for a new sub-agent, a few words in the language the user writes in, shown in job lists (e.g. "Survey auth module"). Omit with job_id.
    description: Option<String>,
    /// Follow up a sub-agent you started earlier, continuing its own conversation. Omit to start a new one.
    job_id: Option<i64>,
}

impl Tool for AgentTool {
    type Args = AgentArgs;

    fn name(&self) -> &str {
        "agent"
    }

    fn description(&self) -> &str {
        "Run a sub-agent in the background on a self-contained task. It works in its own conversation with file, shell and web_fetch tools, and its final reply arrives as a completion. Pass job_id to follow up the same sub-agent; each follow-up delivers again."
    }

    async fn execute(&self, args: AgentArgs, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let port = (ctx.background() as &dyn Any)
            .downcast_ref::<JobPort>()
            .ok_or_else(|| ToolError::business("sub-agents are not available here"))?;
        let task = args.task.trim().to_owned();
        if task.is_empty() {
            return Err(ToolError::input("task must not be empty"));
        }
        let jobs = self.jobs.clone();
        let origin = port.origin.clone();
        let id = match (args.job_id, args.description) {
            (None, Some(description)) => {
                let description = description.trim().to_owned();
                if description.is_empty() {
                    return Err(ToolError::input("description must not be empty"));
                }
                let scope = self.scope.clone();
                port.register(async move {
                    jobs.spawn_agent(origin, scope, description, task)
                        .await
                        .map_err(|e| {
                            tracing::error!(error = %e, "sub-agent registration failed");
                            format!("failed to start the sub-agent: {e}")
                        })
                })
                .await?
            }
            (Some(id), None) => {
                let child = self.follow_up_target(&origin, JobId(id)).await?;
                port.register(async move {
                    jobs.follow_up(origin, child, task).await.map_err(|e| {
                        tracing::error!(error = %e, "sub-agent follow-up failed");
                        format!("failed to send the follow-up: {e}")
                    })?;
                    Ok(JobId(id))
                })
                .await?
            }
            (None, None) => {
                return Err(ToolError::input(
                    "description is required when starting a new sub-agent",
                ))
            }
            (Some(_), Some(_)) => {
                return Err(ToolError::input(
                    "description only applies to a new sub-agent; omit it when following up with job_id",
                ))
            }
        };
        Ok(ToolOutput::Dispatched {
            exec_id: id.0.to_string(),
        })
    }
}

impl AgentTool {
    async fn follow_up_target(&self, origin: &Origin, id: JobId) -> Result<SessionId, ToolError> {
        let job = self
            .jobs
            .store
            .job(id)
            .await
            .map_err(|e| ToolError::dependency(e.to_string()))?
            .ok_or_else(|| ToolError::input("no such job"))?;
        let JobKind::Agent { child_session_id } = job.kind else {
            return Err(ToolError::input(format!("job {} is not a sub-agent", id.0)));
        };
        if job.owner_session_id != origin.session {
            return Err(ToolError::business(
                "only the session that started a sub-agent can follow it up",
            ));
        }
        if self.jobs.control.closing(child_session_id).is_some() {
            return Err(ToolError::business(
                "this sub-agent is being stopped; follow up once job_stop has returned",
            ));
        }
        Ok(child_session_id)
    }
}

impl Jobs {
    /// 同事务建 job、子会话与首条输入，随后唤醒子会话。
    async fn spawn_agent(
        self: Arc<Self>,
        origin: Origin,
        scope: ToolScope,
        description: String,
        task: String,
    ) -> Result<JobId, StoreError> {
        let tree = tree(&self.store.session_root(origin.session).await?);
        let tool_scope = ToolScope::Only(match scope {
            ToolScope::All => self.agent_tools.clone(),
            ToolScope::Only(names) => names.intersection(&self.agent_tools).cloned().collect(),
        });
        let (job, child) = {
            let publish = self.events.publisher().await;
            let (job, child, input) = self
                .store
                .create_agent_job(NewAgentJob {
                    owner_session_id: origin.session,
                    origin_run_id: origin.run,
                    origin_tool_call_id: origin.tool_call_id,
                    tool_name: origin.tool_name,
                    description,
                    task,
                    tool_scope,
                    created_at: now_ms(),
                })
                .await?;
            publish.appended(child.id, &tree.channel, input);
            (job, child)
        };
        self.wake(child.id).await?;
        self.changed(&tree);
        Ok(job.id)
    }

    async fn follow_up(
        self: Arc<Self>,
        origin: Origin,
        child: SessionId,
        task: String,
    ) -> Result<(), StoreError> {
        let tree = tree(&self.store.session_root(child).await?);
        {
            let publish = self.events.publisher().await;
            let input = self
                .store
                .follow_up_agent(child, origin.run, task, now_ms())
                .await?;
            publish.appended(child, &tree.channel, input);
        }
        self.wake(child).await?;
        Ok(())
    }

    /// 关闭入口、取消执行中的 run，停掉子会话名下的 tool job，排队输入记为 held；完成后重新开放。
    pub(super) async fn stop_agent(
        &self,
        job: &Job,
        child: SessionId,
        by: StopBy,
    ) -> Result<StopJobOutcome, StoreError> {
        let Some((closing, cancelled)) = self.control.close(child, by).await else {
            return Ok(StopJobOutcome::AlreadyIdle);
        };
        let mut stopped = cancelled;
        for tool_job in self.store.running_jobs(Some(child)).await? {
            stopped |=
                self.stop_tool(by.manager, &tool_job, by.by_user).await == StopJobOutcome::Stopped;
        }
        self.store.hold_session_inputs(child, now_ms()).await?;
        drop(closing);
        tracing::info!(job = job.id.0, stopped, "sub-agent stopped");
        Ok(if stopped {
            StopJobOutcome::Stopped
        } else {
            StopJobOutcome::AlreadyIdle
        })
    }
}

/// 子 run 给父会话的交付；`None` 为 owner 模型自己停的，已从 `job_stop` 获知。
pub(crate) fn delivery(
    state: RunState,
    messages: &[Message],
    stopped_by: Option<StopBy>,
    owner: SessionId,
    child: SessionId,
) -> Option<ExecOutcome> {
    let reply = final_reply(messages);
    let cancelled = |message: String| Some(ExecOutcome::Cancelled { message });
    match state {
        RunState::Completed | RunState::MaxTurns => {
            if reply.is_empty() {
                return Some(ExecOutcome::Failed {
                    kind: ExecFailureKind::Business,
                    message: "The sub-agent finished without a final reply.".to_owned(),
                });
            }
            let mut text = clip(reply, child);
            if state == RunState::MaxTurns {
                text = format!(
                    "[The sub-agent used all its model calls; the work may be unfinished. Follow up to continue.]\n{text}"
                );
            }
            Some(ExecOutcome::Completed {
                output: vec![ContentPart::Text { text }],
            })
        }
        RunState::ProviderFailed => Some(ExecOutcome::Failed {
            kind: ExecFailureKind::Dependency,
            message: format!(
                "The sub-agent's model call failed: {}",
                last_notice(messages).unwrap_or("no details")
            ),
        }),
        RunState::TimedOut => cancelled(format!(
            "The sub-agent run hit the {}-minute limit and was stopped. Follow up to continue.",
            AGENT_RUN_TIMEOUT.as_secs() / 60
        )),
        RunState::Stopped => match stopped_by {
            Some(by) if by.by_user => cancelled("Stopped by the user.".to_owned()),
            Some(by) if by.manager == owner => None,
            Some(by) => cancelled(format!("Stopped by session {}.", by.manager.0)),
            None => cancelled("Stopped.".to_owned()),
        },
        RunState::Interrupted => cancelled(AGENT_INTERRUPTED.to_owned()),
        RunState::Executing => unreachable!("交付只在终态"),
    }
}

/// 本 run 最后一条不带工具调用的回复中的文字。
fn final_reply(messages: &[Message]) -> String {
    messages
        .iter()
        .rev()
        .find_map(|m| match &m.body {
            MessageBody::Reply { blocks, .. }
                if !blocks
                    .iter()
                    .any(|b| matches!(b, ReplyBlock::ToolCall { .. })) =>
            {
                Some(
                    blocks
                        .iter()
                        .filter_map(|b| match b {
                            ReplyBlock::Text { text, .. } => Some(text.as_str()),
                            _ => None,
                        })
                        .collect::<Vec<_>>()
                        .join("\n"),
                )
            }
            _ => None,
        })
        .unwrap_or_default()
        .trim()
        .to_owned()
}

fn last_notice(messages: &[Message]) -> Option<&str> {
    messages.iter().rev().find_map(|m| match &m.body {
        MessageBody::Notification { text, .. } => Some(text.as_str()),
        _ => None,
    })
}

fn clip(mut text: String, child: SessionId) -> String {
    if text.len() <= AGENT_DELIVERY_MAX {
        return text;
    }
    let mut cut = AGENT_DELIVERY_MAX;
    while !text.is_char_boundary(cut) {
        cut -= 1;
    }
    let dropped = text.len() - cut;
    text.truncate(cut);
    format!(
        "{text}\n[truncated: {dropped} more bytes; the full reply is in sub-session {}]",
        child.0
    )
}

/// 交付消息里给模型/列表看的正文。
pub(super) fn report_text(body: &MessageBody) -> String {
    let MessageBody::Completion { outcome, .. } = body else {
        unreachable!("交付必是 Completion");
    };
    match outcome {
        ExecOutcome::Completed { output } => output
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ExecOutcome::Failed { message, .. } => format!("[failed] {message}"),
        ExecOutcome::Cancelled { message } => format!("[cancelled] {message}"),
    }
}
