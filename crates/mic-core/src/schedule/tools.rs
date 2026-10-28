//! `schedule` / `schedules`：在对话里管理定时任务。

use std::sync::Arc;

use mic_message::{ContentPart, SessionId};
use mic_store::{FireOutcome, Schedule, ScheduleId, StoreError};
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::operations::{ScheduleError, ScheduleUpdate};
use super::rule;
use super::Schedules;
use crate::limits::SCHEDULE_FIRES_SHOWN;

pub(super) fn handles(schedules: &Arc<Schedules>, session: SessionId) -> [ToolHandle; 2] {
    [
        ToolHandle::new(Edit {
            schedules: schedules.clone(),
            session,
        }),
        ToolHandle::new(List(schedules.clone())),
    ]
}

fn store_error(e: StoreError) -> ToolError {
    ToolError::dependency(e.to_string())
}

fn text(text: String) -> ToolOutput {
    ToolOutput::completed(vec![ContentPart::Text { text }])
}

fn operation_error(e: ScheduleError) -> ToolError {
    match e {
        ScheduleError::Store(e) => store_error(e),
        e => ToolError::input(e.to_string()),
    }
}

/// 一行概要：编号、名字、规则、启停、下次运行。
fn summary(s: &Schedule) -> String {
    let state = match (&s.paused_reason, s.enabled()) {
        (_, true) => "enabled".to_owned(),
        (Some(reason), false) => format!("paused by the system: {reason}"),
        (None, false) => "disabled".to_owned(),
    };
    let next = s.next_run_at.map_or("none".to_owned(), rule::show_zoned);
    format!(
        "#{} \"{}\" | when: {} | {state} | next: {next}",
        s.id.0, s.name, s.rule
    )
}

fn outcome_label(outcome: FireOutcome) -> String {
    match outcome {
        FireOutcome::Pending => "running".into(),
        FireOutcome::Completed { pushed: true } => "completed, pushed".into(),
        FireOutcome::Completed { pushed: false } => "completed, not pushed".into(),
        FireOutcome::Failed => "failed".into(),
        FireOutcome::TimedOut => "timed out".into(),
        FireOutcome::Stopped => "stopped by the user".into(),
        FireOutcome::Interrupted => "interrupted by a restart".into(),
        FireOutcome::Missed { count } => format!("missed {count}× while the server was down"),
        FireOutcome::Busy => "skipped: the session was busy".into(),
    }
}

struct Edit {
    schedules: Arc<Schedules>,
    session: SessionId,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    /// Update or delete this task; omit to create one.
    id: Option<i64>,
    /// Short name, in the user's language.
    name: Option<String>,
    /// 5-field cron (minute hour day month weekday, 0 = Sunday) or a one-off "YYYY-MM-DD HH:MM", in the server's local time; a past one-off is rejected, and recurring runs must be at least 10 minutes apart.
    when: Option<String>,
    /// Self-contained instruction for every run, including when and what to report to the user; runs do not see this conversation.
    prompt: Option<String>,
    /// Update only; new tasks start enabled.
    enabled: Option<bool>,
    #[serde(default)]
    delete: bool,
}

impl Tool for Edit {
    type Args = EditArgs;

    fn name(&self) -> &str {
        "schedule"
    }

    fn description(&self) -> &str {
        "Create (name, when and prompt required), update (id; omitted fields stay unchanged) or delete (id and delete only) a scheduled task. Runs happen in the task's own session, which starts with this session's working directory and model. A one-off task disables itself after its run; a run due while the previous one is still going is skipped; runs missed while the server is down are recorded, not caught up."
    }

    async fn execute(&self, args: EditArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let edit = ScheduleUpdate {
            name: args.name,
            rule: args.when,
            prompt: args.prompt,
            enabled: args.enabled,
        };
        let schedule = match args.id.map(ScheduleId) {
            None => {
                if args.delete || edit.enabled.is_some() {
                    return Err(ToolError::input("delete and enabled need an id"));
                }
                let (Some(name), Some(rule), Some(prompt)) = (edit.name, edit.rule, edit.prompt)
                else {
                    return Err(ToolError::input(
                        "name, when and prompt are required to create a task",
                    ));
                };
                self.schedules
                    .create(self.session, name, rule, prompt)
                    .await
                    .map_err(operation_error)?
            }
            Some(id) if args.delete => {
                if edit.name.is_some()
                    || edit.rule.is_some()
                    || edit.prompt.is_some()
                    || edit.enabled.is_some()
                {
                    return Err(ToolError::input("delete takes only id"));
                }
                self.schedules.delete(id).await.map_err(operation_error)?;
                return Ok(text(format!(
                    "Deleted task #{}; its session and history are kept.",
                    id.0
                )));
            }
            Some(id) => self
                .schedules
                .update(id, edit)
                .await
                .map_err(operation_error)?,
        };
        Ok(text(format!("Saved. {}", summary(&schedule))))
    }
}

struct List(Arc<Schedules>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct ListArgs {
    /// Show this task's recent runs instead of the list.
    id: Option<i64>,
}

impl Tool for List {
    type Args = ListArgs;

    fn name(&self) -> &str {
        "schedules"
    }

    fn description(&self) -> &str {
        "List scheduled tasks with their last outcome, or with id, a task's prompt and recent runs."
    }

    async fn execute(&self, args: ListArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let out = match args.id.map(ScheduleId) {
            None => list(&self.0).await?,
            Some(id) => detail(&self.0, id).await?,
        };
        Ok(text(out))
    }
}

async fn list(schedules: &Schedules) -> Result<String, ToolError> {
    let all = schedules.list().await.map_err(operation_error)?;
    if all.is_empty() {
        return Ok("No scheduled tasks.".into());
    }
    Ok(all
        .iter()
        .map(|v| {
            let last = v.recent.first().map_or("never run".into(), |f| {
                format!(
                    "{} at {}",
                    outcome_label(f.outcome),
                    rule::show_zoned(f.due_at)
                )
            });
            format!("{} | last: {last}", summary(&v.schedule))
        })
        .collect::<Vec<_>>()
        .join("\n"))
}

async fn detail(schedules: &Schedules, id: ScheduleId) -> Result<String, ToolError> {
    let s = schedules.get(id).await.map_err(operation_error)?;
    let mut out = format!("{}\nprompt: {}\nRecent runs:", summary(&s), s.prompt);
    let page = schedules
        .fires(id, None, SCHEDULE_FIRES_SHOWN)
        .await
        .map_err(operation_error)?;
    if page.fires.is_empty() {
        out.push_str(" none");
    }
    for f in page.fires {
        out.push_str(&format!(
            "\n- {} {}",
            rule::show_zoned(f.fire.due_at),
            outcome_label(f.fire.outcome)
        ));
        if let Some(reply) = f.reply {
            out.push_str(&format!(": {reply}"));
        }
    }
    Ok(out)
}
