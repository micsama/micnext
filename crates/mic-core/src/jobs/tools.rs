//! 模型的任务管理工具，管理范围为调用会话及其后代。

use std::sync::Arc;

use mic_message::{ContentPart, SessionId};
use mic_store::{JobId, JobKind};
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::{JobAccessError, Jobs, StopJobOutcome};
use crate::limits::{JOB_READ_DEFAULT, JOB_READ_MAX};

pub(super) fn handles(jobs: &Arc<Jobs>, session: SessionId) -> [ToolHandle; 3] {
    let bound = || Bound {
        jobs: jobs.clone(),
        session,
    };
    [
        ToolHandle::new(List(bound())),
        ToolHandle::new(Output(bound())),
        ToolHandle::new(Stop(bound())),
    ]
}

struct Bound {
    jobs: Arc<Jobs>,
    session: SessionId,
}

fn access(e: JobAccessError) -> ToolError {
    match e {
        JobAccessError::NotFound => ToolError::input("no such job"),
        JobAccessError::Forbidden => {
            ToolError::business("this job belongs to another session; only your own and your sub-sessions' jobs are visible")
        }
        JobAccessError::ReadLimit => {
            ToolError::input(format!("limit must be between 1 and {JOB_READ_MAX}"))
        }
        JobAccessError::AgentOffset => {
            ToolError::input("a sub-agent's delivery is read whole; omit offset")
        }
        JobAccessError::Io(e) => ToolError::dependency(format!("failed to read job output: {e}")),
        JobAccessError::Store(e) => ToolError::dependency(e.to_string()),
    }
}

fn text(text: String) -> ToolOutput {
    ToolOutput::Completed(vec![ContentPart::Text { text }])
}

struct List(Bound);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    /// Include those inactive for over 14 days.
    #[serde(default)]
    include_old: bool,
}

impl Tool for List {
    type Args = ListArgs;

    fn name(&self) -> &str {
        "jobs"
    }

    fn description(&self) -> &str {
        "List background jobs and sub-agents of this session and its sub-sessions, active first."
    }

    async fn execute(&self, args: ListArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let views = self
            .0
            .jobs
            .views(self.0.session, args.include_old)
            .await
            .map_err(access)?;
        if views.is_empty() {
            return Ok(text("No jobs.".to_owned()));
        }
        let json = serde_json::to_string_pretty(&views).expect("任务列表可序列化");
        Ok(text(json))
    }
}

struct Output(Bound);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct OutputArgs {
    job_id: i64,
    /// Byte offset; omit to read the tail.
    offset: Option<u64>,
    /// Bytes; default 16384, max 65536.
    limit: Option<u64>,
}

impl Tool for Output {
    type Args = OutputArgs;

    fn name(&self) -> &str {
        "job_output"
    }

    fn description(&self) -> &str {
        "Read a background job's log, also while it runs; only the last 1 MiB is kept. For a sub-agent, returns its latest reply in full."
    }

    async fn execute(&self, args: OutputArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let (job, out) = self
            .0
            .jobs
            .output(
                self.0.session,
                JobId(args.job_id),
                args.offset,
                args.limit.unwrap_or(JOB_READ_DEFAULT),
            )
            .await
            .map_err(access)?;
        Ok(text(format!(
            "[job {} {}; bytes {}..{} of {}; available_from={}; eof={}]\n{}",
            job.id.0,
            match job.kind {
                JobKind::Tool { state, .. } => state.as_str(),
                JobKind::Agent { .. } => "agent",
            },
            out.offset,
            out.next_offset,
            out.total_bytes,
            out.available_from,
            out.eof,
            out.text
        )))
    }
}

struct Stop(Bound);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct StopArgs {
    job_id: i64,
}

impl Tool for Stop {
    type Args = StopArgs;

    fn name(&self) -> &str {
        "job_stop"
    }

    fn description(&self) -> &str {
        "Stop a background job (kills its process group) or a sub-agent's current run (stops its jobs too; it can still be followed up). No completion follows."
    }

    async fn execute(&self, args: StopArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let outcome = self
            .0
            .jobs
            .stop(self.0.session, JobId(args.job_id), false)
            .await
            .map_err(access)?;
        Ok(text(
            match outcome {
                StopJobOutcome::Stopped => "Stopped.",
                StopJobOutcome::AlreadyIdle => "The sub-agent was not running.",
                StopJobOutcome::AlreadyFinished => {
                    "The job had already finished; its result arrives (or arrived) as a completion."
                }
            }
            .to_owned(),
        ))
    }
}
