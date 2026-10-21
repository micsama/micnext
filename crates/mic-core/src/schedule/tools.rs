//! `schedule` / `schedules`：在对话里管理定时任务。

use std::sync::Arc;

use mic_message::{ContentPart, SessionId};
use mic_store::{FireOutcome, NewSchedule, Schedule, ScheduleEdit, ScheduleId, Store, StoreError};
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use super::rule::{self, Rule};
use super::{final_paragraph, Schedules};
use crate::limits::SCHEDULE_FIRES_SHOWN;
use crate::run::now_ms;

/// 触发列表里回复首段的字符上限。
const PARAGRAPH_CHARS: usize = 200;

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
    ToolOutput::Completed(vec![ContentPart::Text { text }])
}

fn non_empty(field: &str, value: Option<String>) -> Result<Option<String>, ToolError> {
    match value.map(|v| v.trim().to_owned()) {
        Some(v) if v.is_empty() => Err(ToolError::input(format!("{field} must not be empty"))),
        v => Ok(v),
    }
}

/// 写入前校验规则，连同 now 之后的下一次；一次性时刻须在将来。
fn checked_rule(when: Option<String>) -> Result<Option<(String, i64)>, ToolError> {
    let Some(when) = non_empty("when", when)? else {
        return Ok(None);
    };
    let rule = Rule::parse(&when).map_err(ToolError::input)?;
    let next = upcoming(&rule)?;
    Ok(Some((when, next)))
}

fn upcoming(rule: &Rule) -> Result<i64, ToolError> {
    rule.next_after(now_ms())
        .ok_or_else(|| ToolError::input("that time has already passed; give a new when"))
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
    /// 5-field cron (minute hour day month weekday, 0 = Sunday) or a one-off "YYYY-MM-DD HH:MM", in the server's local time; a past one-off is rejected.
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
        let store = &self.schedules.store;
        let name = non_empty("name", args.name)?;
        let prompt = non_empty("prompt", args.prompt)?;
        let rule = checked_rule(args.when)?;
        let schedule = match args.id.map(ScheduleId) {
            None => {
                let (Some(name), Some((rule, next)), Some(prompt)) = (name, rule, prompt) else {
                    return Err(ToolError::input(
                        "name, when and prompt are required to create a task",
                    ));
                };
                if args.delete || args.enabled.is_some() {
                    return Err(ToolError::input("delete and enabled need an id"));
                }
                let session = store
                    .session(self.session)
                    .await
                    .map_err(store_error)?
                    .expect("调用会话存在");
                store
                    .create_schedule(NewSchedule {
                        person: self.schedules.owner,
                        name,
                        rule,
                        prompt,
                        pwd: session.pwd,
                        persona: session.persona_id,
                        model: session.model_id,
                        reasoning_effort: session.reasoning_effort,
                        next_run_at: next,
                        at: now_ms(),
                    })
                    .await
                    .map_err(store_error)?
            }
            Some(id) if args.delete => {
                if name.is_some() || rule.is_some() || prompt.is_some() || args.enabled.is_some() {
                    return Err(ToolError::input("delete takes only id"));
                }
                if !store.delete_schedule(id).await.map_err(store_error)? {
                    return Err(no_such(id));
                }
                self.schedules.changed();
                return Ok(text(format!(
                    "Deleted task #{}; its session and history are kept.",
                    id.0
                )));
            }
            Some(_)
                if name.is_none()
                    && rule.is_none()
                    && prompt.is_none()
                    && args.enabled.is_none() =>
            {
                return Err(ToolError::input("nothing to update"));
            }
            Some(id) => {
                let old = store
                    .schedule(id)
                    .await
                    .map_err(store_error)?
                    .ok_or_else(|| no_such(id))?;
                // 停用置空；重新启用或启用中改规则，从 now 往后算。
                let next_run_at = match (args.enabled, &rule) {
                    (Some(false), _) => old.enabled().then_some(None),
                    (Some(true), _) if !old.enabled() => Some(Some(match &rule {
                        Some((_, next)) => *next,
                        None => upcoming(&Rule::parse(&old.rule).expect("写入时已校验"))?,
                    })),
                    (_, Some((when, next))) if old.enabled() && *when != old.rule => {
                        Some(Some(*next))
                    }
                    _ => None,
                };
                let edit = ScheduleEdit {
                    name,
                    rule: rule.map(|(when, _)| when),
                    prompt,
                    next_run_at,
                };
                store
                    .update_schedule(id, edit, now_ms())
                    .await
                    .map_err(store_error)?
                    .ok_or_else(|| no_such(id))?
            }
        };
        self.schedules.changed();
        Ok(text(format!("Saved. {}", summary(&schedule))))
    }
}

fn no_such(id: ScheduleId) -> ToolError {
    ToolError::input(format!("no scheduled task #{}", id.0))
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
        let store = &self.0.store;
        let out = match args.id.map(ScheduleId) {
            None => list(store, &self.0).await?,
            Some(id) => detail(store, id).await?,
        };
        Ok(text(out))
    }
}

async fn list(store: &Store, schedules: &Schedules) -> Result<String, ToolError> {
    let all = store
        .schedules(schedules.owner)
        .await
        .map_err(store_error)?;
    if all.is_empty() {
        return Ok("No scheduled tasks.".into());
    }
    let mut lines = Vec::with_capacity(all.len());
    for s in &all {
        let last = store
            .schedule_fires(s.id, 1)
            .await
            .map_err(store_error)?
            .first()
            .map_or("never run".to_owned(), |f| {
                format!(
                    "{} at {}",
                    outcome_label(f.outcome),
                    rule::show_zoned(f.due_at)
                )
            });
        lines.push(format!("{} | last: {last}", summary(s)));
    }
    Ok(lines.join("\n"))
}

async fn detail(store: &Store, id: ScheduleId) -> Result<String, ToolError> {
    let s = store
        .schedule(id)
        .await
        .map_err(store_error)?
        .ok_or_else(|| no_such(id))?;
    let mut out = format!("{}\nprompt: {}\nRecent runs:", summary(&s), s.prompt);
    let fires = store
        .schedule_fires(id, SCHEDULE_FIRES_SHOWN)
        .await
        .map_err(store_error)?;
    if fires.is_empty() {
        out.push_str(" none");
    }
    for f in fires {
        out.push_str(&format!(
            "\n- {} {}",
            rule::show_zoned(f.due_at),
            outcome_label(f.outcome)
        ));
        let Some(run) = f.run_id else { continue };
        if let Some(reply) = final_paragraph(store, run).await.map_err(store_error)? {
            let clipped: String = reply.chars().take(PARAGRAPH_CHARS).collect();
            let ellipsis = if clipped.len() < reply.len() {
                "…"
            } else {
                ""
            };
            out.push_str(&format!(": {clipped}{ellipsis}"));
        }
    }
    Ok(out)
}
