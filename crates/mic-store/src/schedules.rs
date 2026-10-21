//! 定时任务与触发。契约：docs/blueprints/scheduled-tasks.md P2。

use mic_message::{Message, MessageBody, PersonId, SessionCommand, SessionId};
use rusqlite::{params, OptionalExtension, Row, Transaction};
use serde::Serialize;

use crate::messages::insert_message;
use crate::sessions::insert_session;
use crate::store::Store;
use crate::{
    ModelId, NewSession, PersonaId, ReasoningEffort, RunId, RunState, SessionKind, StoreError,
    ToolScope,
};

/// 任务会话的 `Triggered.module`。
pub const SCHEDULE_MODULE: &str = "schedule";

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ScheduleId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct FireId(pub i64);

#[derive(Debug, Clone, Serialize)]
pub struct Schedule {
    pub id: ScheduleId,
    pub person: PersonId,
    pub name: String,
    /// 已校验的规则原文（cron 或一次性本地时刻）。
    pub rule: String,
    pub prompt: String,
    pub enabled: bool,
    pub paused_reason: Option<String>,
    pub active_since: i64,
    pub session_id: SessionId,
    pub created_at: i64,
    pub updated_at: i64,
}

/// 新任务；任务会话的设置拷自调用会话。
pub struct NewSchedule {
    pub person: PersonId,
    pub name: String,
    pub rule: String,
    pub prompt: String,
    pub pwd: String,
    pub persona: PersonaId,
    pub model: Option<ModelId>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub at: i64,
}

/// 只改给出的字段；改规则或重新启用时 `active_since` 置为当前，启用同时清暂停原因。
#[derive(Default)]
pub struct ScheduleEdit {
    pub name: Option<String>,
    pub rule: Option<String>,
    pub prompt: Option<String>,
    pub enabled: Option<bool>,
}

/// 一次触发的结局，由触发行与触发消息所在 run 推导。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum FireOutcome {
    /// 触发消息未认领或 run 执行中。
    Pending,
    /// `Completed`/`MaxTurns`；`pushed` 为本 run 是否调用过 push。
    Completed {
        pushed: bool,
    },
    Failed,
    TimedOut,
    Stopped,
    /// run 被重启中断，或触发消息在重启时未认领而被 held。
    Interrupted,
    Missed {
        count: u32,
    },
    Busy,
}

impl FireOutcome {
    /// 系统级失败：框架推送，并计入连续失败。
    pub fn is_failure(self) -> bool {
        matches!(
            self,
            Self::Failed | Self::TimedOut | Self::Interrupted | Self::Missed { .. }
        )
    }

    /// 不计入连续失败，也不打断。
    pub fn is_neutral(self) -> bool {
        matches!(self, Self::Pending | Self::Busy | Self::Stopped)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct Fire {
    pub id: FireId,
    pub schedule_id: ScheduleId,
    pub due_at: i64,
    pub outcome: FireOutcome,
    pub run_id: Option<RunId>,
}

pub enum FireResult {
    /// 到点时会话有执行中的 run 或待处理输入，已记 `busy`。
    Busy,
    /// 已写入的消息（可能先有 `/compact`，最后是 `Trigger`），按 id 升序。
    Fired(Vec<Message>),
    /// 任务已删除或停用。
    Gone,
}

const SCHEDULE_COLS: &str = "id, person_id, name, rule, prompt, enabled, paused_reason,
     active_since, session_id, created_at, updated_at";

/// `f` 为 core_schedule_fires 别名；列序同 `fire`。
const FIRE_SELECT: &str = "
SELECT f.id, f.schedule_id, f.due_at, f.skipped, f.missed_count, r.id, r.state,
       EXISTS (SELECT 1 FROM core_input_holds h WHERE h.message_id = f.message_id),
       EXISTS (SELECT 1 FROM core_pushes p WHERE p.source_run_id = r.id)
FROM core_schedule_fires f
LEFT JOIN core_messages m ON m.id = f.message_id
LEFT JOIN core_runs r ON r.id = m.run_id";

fn schedule(row: &Row<'_>) -> rusqlite::Result<Schedule> {
    Ok(Schedule {
        id: ScheduleId(row.get(0)?),
        person: PersonId(row.get(1)?),
        name: row.get(2)?,
        rule: row.get(3)?,
        prompt: row.get(4)?,
        enabled: row.get(5)?,
        paused_reason: row.get(6)?,
        active_since: row.get(7)?,
        session_id: SessionId(row.get(8)?),
        created_at: row.get(9)?,
        updated_at: row.get(10)?,
    })
}

fn fire(row: &Row<'_>) -> rusqlite::Result<Fire> {
    let skipped: Option<String> = row.get(3)?;
    let run_id: Option<i64> = row.get(5)?;
    let state = match run_id {
        Some(_) => Some(crate::row::run_state(row, 6)?),
        None => None,
    };
    let held: bool = row.get(7)?;
    let pushed: bool = row.get(8)?;
    let outcome = match (skipped.as_deref(), state) {
        (Some("missed"), _) => FireOutcome::Missed { count: row.get(4)? },
        (Some(_), _) => FireOutcome::Busy,
        (None, None) if held => FireOutcome::Interrupted,
        (None, None | Some(RunState::Executing)) => FireOutcome::Pending,
        (None, Some(RunState::Completed | RunState::MaxTurns)) => FireOutcome::Completed { pushed },
        (None, Some(RunState::ProviderFailed)) => FireOutcome::Failed,
        (None, Some(RunState::TimedOut)) => FireOutcome::TimedOut,
        (None, Some(RunState::Stopped)) => FireOutcome::Stopped,
        (None, Some(RunState::Interrupted)) => FireOutcome::Interrupted,
    };
    Ok(Fire {
        id: FireId(row.get(0)?),
        schedule_id: ScheduleId(row.get(1)?),
        due_at: row.get(2)?,
        outcome,
        run_id: run_id.map(RunId),
    })
}

fn read_schedule(
    conn: &rusqlite::Connection,
    id: ScheduleId,
) -> rusqlite::Result<Option<Schedule>> {
    conn.query_row(
        &format!("SELECT {SCHEDULE_COLS} FROM core_schedules WHERE id = ?1"),
        [id.0],
        schedule,
    )
    .optional()
}

impl Store {
    /// 同事务建任务与其会话（`Triggered { module: "schedule", ref_id: 任务 id }`）；
    /// 会话标题即任务名，任务删除后仍可辨认来源。
    pub async fn create_schedule(&self, new: NewSchedule) -> Result<Schedule, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let session = insert_session(
                &tx,
                NewSession {
                    kind: SessionKind::Triggered {
                        module: SCHEDULE_MODULE.to_owned(),
                        ref_id: String::new(),
                    },
                    parent_session_id: None,
                    delivery_target: None,
                    pwd: new.pwd,
                    tool_scope: ToolScope::All,
                    created_at: new.at,
                    persona: Some(new.persona),
                    model: new.model,
                    reasoning_effort: new.reasoning_effort,
                },
            )?;
            tx.execute(
                "INSERT INTO core_schedules
                   (person_id, name, rule, prompt, enabled, active_since, session_id, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?5, ?5)",
                params![new.person.0, new.name, new.rule, new.prompt, new.at, session.id.0],
            )?;
            let id = ScheduleId(tx.last_insert_rowid());
            tx.execute(
                "UPDATE core_sessions SET trigger_ref = ?2, title = ?3 WHERE id = ?1",
                params![session.id.0, id.0.to_string(), new.name],
            )?;
            let schedule = read_schedule(&tx, id)?.expect("刚写入");
            tx.commit()?;
            Ok(schedule)
        })
        .await
    }

    /// 不存在返回 `None`。
    pub async fn update_schedule(
        &self,
        id: ScheduleId,
        edit: ScheduleEdit,
        at: i64,
    ) -> Result<Option<Schedule>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let Some(old) = read_schedule(&tx, id)? else {
                return Ok(None);
            };
            let rule_changed = edit.rule.as_ref().is_some_and(|r| *r != old.rule);
            let enabled = edit.enabled.unwrap_or(old.enabled);
            let restart = rule_changed || (enabled && !old.enabled);
            if let Some(name) = &edit.name {
                tx.execute(
                    "UPDATE core_sessions SET title = ?2 WHERE id = ?1",
                    params![old.session_id.0, name],
                )?;
            }
            tx.execute(
                "UPDATE core_schedules SET name = ?2, rule = ?3, prompt = ?4, enabled = ?5,
                   paused_reason = CASE WHEN ?6 THEN NULL ELSE paused_reason END,
                   active_since = CASE WHEN ?7 THEN ?8 ELSE active_since END, updated_at = ?8
                 WHERE id = ?1",
                params![
                    id.0,
                    edit.name.unwrap_or(old.name),
                    edit.rule.unwrap_or(old.rule),
                    edit.prompt.unwrap_or(old.prompt),
                    enabled,
                    edit.enabled == Some(true),
                    restart,
                    at,
                ],
            )?;
            let schedule = read_schedule(&tx, id)?;
            tx.commit()?;
            Ok(schedule)
        })
        .await
    }

    /// 删任务与触发行；任务会话、run 与 trace 保留。不存在返回 false。
    pub async fn delete_schedule(&self, id: ScheduleId) -> Result<bool, StoreError> {
        self.call(move |conn| {
            Ok(conn.execute("DELETE FROM core_schedules WHERE id = ?1", [id.0])? > 0)
        })
        .await
    }

    pub async fn schedule(&self, id: ScheduleId) -> Result<Option<Schedule>, StoreError> {
        self.call(move |conn| Ok(read_schedule(conn, id)?)).await
    }

    /// 该 person 的全部任务，按 id 升序。
    pub async fn schedules(&self, person: PersonId) -> Result<Vec<Schedule>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {SCHEDULE_COLS} FROM core_schedules WHERE person_id = ?1 ORDER BY id"
                ))?
                .query_map([person.0], schedule)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 全部启用的任务，连同最后一行触发的 `due_at`。
    pub async fn enabled_schedules(&self) -> Result<Vec<(Schedule, Option<i64>)>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {SCHEDULE_COLS},
                       (SELECT due_at FROM core_schedule_fires f WHERE f.schedule_id = s.id
                        ORDER BY f.id DESC LIMIT 1)
                     FROM core_schedules s WHERE enabled = 1 ORDER BY id"
                ))?
                .query_map([], |r| Ok((schedule(r)?, r.get(11)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 最近 `limit` 次触发，新的在前。
    pub async fn schedule_fires(
        &self,
        id: ScheduleId,
        limit: u32,
    ) -> Result<Vec<Fire>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "{FIRE_SELECT} WHERE f.schedule_id = ?1 ORDER BY f.id DESC LIMIT ?2"
                ))?
                .query_map(params![id.0, limit], fire)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 由该 run 执行的触发。
    pub async fn fire_of_run(&self, run: RunId) -> Result<Option<Fire>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(&format!("{FIRE_SELECT} WHERE r.id = ?1"), [run.0], fire)
                .optional()?)
        })
        .await
    }

    /// `since` 起被重启收尾的触发：run 转 `Interrupted` 或触发消息被 held。
    pub async fn fires_interrupted_since(&self, since: i64) -> Result<Vec<Fire>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "{FIRE_SELECT}
                     WHERE (r.state = 'interrupted' AND r.finished_at >= ?1)
                        OR EXISTS (SELECT 1 FROM core_input_holds h
                                   WHERE h.message_id = f.message_id AND h.held_at >= ?1)
                     ORDER BY f.id"
                ))?
                .query_map([since], fire)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 停机期间错过的触发合并为一行；`due_at` 为最后一次。
    pub async fn record_missed(
        &self,
        id: ScheduleId,
        due_at: i64,
        count: u32,
    ) -> Result<Fire, StoreError> {
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO core_schedule_fires (schedule_id, due_at, skipped, missed_count)
                 VALUES (?1, ?2, 'missed', ?3)",
                params![id.0, due_at, count],
            )?;
            let fire_id = conn.last_insert_rowid();
            Ok(conn.query_row(&format!("{FIRE_SELECT} WHERE f.id = ?1"), [fire_id], fire)?)
        })
        .await
    }

    /// 框架自动暂停。
    pub async fn pause_schedule(
        &self,
        id: ScheduleId,
        reason: String,
        at: i64,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE core_schedules SET enabled = 0, paused_reason = ?2, updated_at = ?3
                 WHERE id = ?1",
                params![id.0, reason, at],
            )?;
            Ok(())
        })
        .await
    }

    /// 到点：检查忙、按需先追加 `/compact`、追加 `Trigger` 并写触发行，同一事务。
    /// `once` 为一次性规则，触发后停用。
    pub async fn fire_schedule(
        &self,
        id: ScheduleId,
        due_at: i64,
        compact: bool,
        once: bool,
        at: i64,
    ) -> Result<FireResult, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let Some(s) = read_schedule(&tx, id)?.filter(|s| s.enabled) else {
                return Ok(FireResult::Gone);
            };
            if once {
                tx.execute(
                    "UPDATE core_schedules SET enabled = 0, updated_at = ?2 WHERE id = ?1",
                    params![id.0, at],
                )?;
            }
            let result = if busy(&tx, s.session_id)? {
                tx.execute(
                    "INSERT INTO core_schedule_fires (schedule_id, due_at, skipped)
                     VALUES (?1, ?2, 'busy')",
                    params![id.0, due_at],
                )?;
                FireResult::Busy
            } else {
                let mut bodies = Vec::new();
                if compact {
                    bodies.push(MessageBody::Command {
                        person: s.person,
                        command: SessionCommand::Compact { text: None },
                    });
                }
                bodies.push(MessageBody::Trigger {
                    source: s.name,
                    text: s.prompt,
                });
                let mut messages = Vec::with_capacity(bodies.len());
                for body in bodies {
                    messages.push(Message {
                        id: insert_message(&tx, s.session_id, None, &body, at)?,
                        session_id: s.session_id,
                        body,
                        created_at: at,
                        delivered_at: None,
                    });
                }
                tx.execute(
                    "INSERT INTO core_schedule_fires (schedule_id, due_at, message_id)
                     VALUES (?1, ?2, ?3)",
                    params![id.0, due_at, messages.last().expect("至少有 Trigger").id.0],
                )?;
                FireResult::Fired(messages)
            };
            tx.commit()?;
            Ok(result)
        })
        .await
    }
}

/// 会话有执行中的 run 或非 held 未认领输入。
fn busy(tx: &Transaction<'_>, session: SessionId) -> rusqlite::Result<bool> {
    tx.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM core_runs WHERE session_id = ?1 AND state = 'executing')
                 OR EXISTS(SELECT 1 FROM core_messages m
                           WHERE m.session_id = ?1 AND {} AND NOT ({}))",
            crate::row::UNCLAIMED,
            crate::row::HELD_INPUT
        ),
        [session.0],
        |r| r.get(0),
    )
}

#[cfg(test)]
mod tests;
