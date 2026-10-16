//! 模型的任务管理工具，管理范围为调用会话及其后代。

use std::sync::Arc;

use mic_message::{ContentPart, SessionId};
use mic_store::JobId;
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
    /// Also list jobs that finished more than 14 days ago.
    #[serde(default)]
    include_old: bool,
}

impl Tool for List {
    type Args = ListArgs;

    fn name(&self) -> &str {
        "jobs"
    }

    fn description(&self) -> &str {
        "List background jobs started by this session and its sub-sessions: running ones first, then by most recent activity. Each entry shows the job id, tool, description, state, timing, output size and the last few lines of output."
    }

    fn prompt_hint(&self) -> Option<&str> {
        None
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
    /// Byte offset over the job's whole output. Omit to read the tail.
    offset: Option<u64>,
    /// Bytes to read, at most 65536. Defaults to 16384.
    limit: Option<u64>,
}

impl Tool for Output {
    type Args = OutputArgs;

    fn name(&self) -> &str {
        "job_output"
    }

    fn description(&self) -> &str {
        "Read a background job's output, which is readable while the job runs. Without offset it returns the tail; with offset it reads forward from that byte and reports next_offset to continue. Only the most recent 1 MiB is kept: available_from is the earliest readable byte."
    }

    fn prompt_hint(&self) -> Option<&str> {
        None
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
            job.state.as_str(),
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
        "Stop a running background job (a bash job's whole process group is killed). Returns once the job has finished; no completion follows for a job you stop yourself."
    }

    fn prompt_hint(&self) -> Option<&str> {
        None
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
                StopJobOutcome::AlreadyFinished => {
                    "The job had already finished; its result arrives (or arrived) as a completion."
                }
            }
            .to_owned(),
        ))
    }
}
