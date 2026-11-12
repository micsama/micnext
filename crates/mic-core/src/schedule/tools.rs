//! `schedule` / `schedules`：在对话里管理定时任务。

use std::sync::Arc;

use mic_message::{ContentPart, SessionId};
use mic_store::{FireOutcome, FireResult, Schedule, ScheduleId, StoreError};
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

#[derive(Clone, Copy, Deserialize, JsonSchema)]
#[serde(rename_all = "snake_case")]
enum Action {
    Create,
    Update,
    Delete,
    RunNow,
}

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct EditArgs {
    action: Action,
    /// Required except for create.
    id: Option<i64>,
    /// Short name, in the user's language.
    name: Option<String>,
    /// 5-field cron (minute hour day month weekday, 0 = Sunday) or a one-off "YYYY-MM-DD HH:MM", in the server's local time; a past one-off is rejected, and recurring runs must be at least 10 minutes apart.
    when: Option<String>,
    /// Self-contained instruction for every run, including when and what to report to the user; runs do not see this conversation.
    prompt: Option<String>,
    /// Update only; new tasks start enabled.
    enabled: Option<bool>,
}

impl Tool for Edit {
    type Args = EditArgs;

    fn name(&self) -> &str {
        "schedule"
    }

    fn description(&self) -> &str {
        "Manage a scheduled task. create: name, when and prompt required. update: id plus the fields to change. delete: id only; the task's session and history are kept. run_now: id only; runs the task once now, also when disabled, leaving its schedule unchanged. Runs happen in the task's own session, which starts with this session's working directory and model. A one-off task disables itself after its run; a run due while the previous one is still going is skipped; runs missed while the server is down are recorded, not caught up."
    }

    async fn execute(&self, args: EditArgs, _: &ToolContext) -> Result<ToolOutput, ToolError> {
        let edit = ScheduleUpdate {
            name: args.name,
            rule: args.when,
            prompt: args.prompt,
            enabled: args.enabled,
        };
        let fields = edit.name.is_some()
            || edit.rule.is_some()
            || edit.prompt.is_some()
            || edit.enabled.is_some();
        let schedule = match (args.action, args.id.map(ScheduleId)) {
            (Action::Create, Some(_)) => return Err(ToolError::input("create takes no id")),
            (Action::Create, None) => {
                let (Some(name), Some(rule), Some(prompt), None) =
                    (edit.name, edit.rule, edit.prompt, edit.enabled)
                else {
                    return Err(ToolError::input(
                        "create takes name, when and prompt, all required",
                    ));
                };
                self.schedules
                    .create(self.session, name, rule, prompt)
                    .await
                    .map_err(operation_error)?
            }
            (_, None) => return Err(ToolError::input("this action needs an id")),
            (Action::Update, Some(id)) => self
                .schedules
                .update(id, edit)
                .await
                .map_err(operation_error)?,
            (Action::Delete | Action::RunNow, Some(_)) if fields => {
                return Err(ToolError::input("delete and run_now take only id"));
            }
            (Action::Delete, Some(id)) => {
                self.schedules.delete(id).await.map_err(operation_error)?;
                return Ok(text(format!(
                    "Deleted task #{}; its session and history are kept.",
                    id.0
                )));
            }
            (Action::RunNow, Some(id)) => {
                return match self.schedules.run_now(id).await.map_err(store_error)? {
                    FireResult::Fired(_) => Ok(text(format!(
                        "Task #{} started in its own session; the outcome shows in its recent runs.",
                        id.0
                    ))),
                    FireResult::Busy => Err(ToolError::input(
                        "the task's session is still busy with an earlier run; recorded as skipped",
                    )),
                    FireResult::Gone => Err(ToolError::input("no such task")),
                };
            }
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
