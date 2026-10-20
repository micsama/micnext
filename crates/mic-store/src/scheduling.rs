use mic_message::{ExecOutcome, Message, MessageBody, MessageId, SessionId};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::jobs::run_person;
use crate::messages::{insert_message, is_input};
use crate::models;
use crate::row::{
    self, input_kinds, HELD_INPUT, MESSAGE_COLS, MESSAGE_FROM, PERSONA_COLS, RUN_COLS, UNCLAIMED,
};
use crate::settings::read_settings;
use crate::store::Store;
use crate::{ClaimedRun, InterruptedRun, Run, RunId, RunInput, RunSettings, RunState, StoreError};

impl Store {
    /// 启动时原子登记遗留未认领输入为 held；有用户消息或命令被搁置的会话落一条通知（回注与中途通知由中断通知说明）。不重复处置。
    pub async fn hold_unclaimed_inputs(&self, at: i64) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let sessions = tx
                .prepare(&format!(
                    "SELECT DISTINCT m.session_id FROM core_messages m
                     WHERE {UNCLAIMED} AND NOT ({HELD_INPUT}) AND m.kind IN ('UserInput', 'Command')
                     ORDER BY m.session_id"
                ))?
                .query_map([], |r| Ok(SessionId(r.get(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tx.execute(
                &format!(
                    "INSERT INTO core_input_holds (message_id, held_at)
                     SELECT m.id, ?1 FROM core_messages m
                     WHERE {UNCLAIMED} AND NOT ({HELD_INPUT})"
                ),
                [at],
            )?;
            let mut notifications = Vec::with_capacity(sessions.len());
            for session_id in sessions {
                let body = MessageBody::Notification {
                    source: "micnext".to_owned(),
                    text: "重启前有消息还没处理，发新消息时会一并交给 agent。".to_owned(),
                };
                notifications.push(Message {
                    id: insert_message(&tx, session_id, None, &body, at)?,
                    session_id,
                    body,
                    created_at: at,
                    delivered_at: None,
                });
            }
            tx.commit()?;
            Ok(notifications)
        })
        .await
    }

    /// 会话无 `executing` run 时，把非 held 未认领输入开头连续同 person 的一段认领为新 run，
    /// 同时定下本轮设置（会话所选人设，已删除则改用默认人设并写回会话；当前偏好），设置只随返回值交给执行方，不落库。
    pub async fn claim_next(
        &self,
        session_id: SessionId,
        now: i64,
    ) -> Result<Option<ClaimedRun>, StoreError> {
        let cipher = self.cipher();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            if crate::sessions::archived(&tx, session_id)? {
                return Ok(None);
            }
            let executing: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM core_runs
                               WHERE session_id = ?1 AND state = 'executing')",
                [session_id.0],
                |r| r.get(0),
            )?;
            if executing {
                return Ok(None);
            }
            let Some((ids, _)) = unclaimed_head(&tx, session_id.0)? else {
                return Ok(None);
            };
            let (persona, replaced) = resolve_persona(&tx, session_id)?;
            if replaced {
                tx.execute(
                    "UPDATE core_sessions SET persona_id = ?1 WHERE id = ?2",
                    params![persona.id.0, session_id.0],
                )?;
            }
            let current = read_settings(&tx)?;
            let general_prompt = current.general_prompt;
            let max_turns = current.max_turns;
            let compaction = current.compaction;
            let model = models::claim_model(&tx, &cipher, session_id)?;
            let reasoning_effort = tx.query_row(
                "SELECT reasoning_effort FROM core_sessions WHERE id = ?1",
                [session_id.0],
                |row| row.get::<_, Option<crate::ReasoningEffort>>(0),
            )?;
            tx.execute(
                "INSERT INTO core_runs (session_id, state, created_at) VALUES (?1, ?2, ?3)",
                params![session_id.0, RunState::Executing.as_str(), now],
            )?;
            let id = RunId(tx.last_insert_rowid());
            assign_run(&tx, id, &ids)?;
            let first = tx.query_row(
                &format!("SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.id = ?1"),
                [ids[0]],
                row::message,
            )?;
            let input = match first.body {
                MessageBody::Command { command, .. } => RunInput::Command {
                    message_id: first.id,
                    command,
                },
                _ => RunInput::Conversation {
                    message_ids: ids.iter().map(|id| MessageId(*id)).collect(),
                },
            };
            tx.commit()?;
            let run = Run {
                id,
                session_id,
                state: RunState::Executing,
                created_at: now,
                finished_at: None,
            };
            let settings = RunSettings {
                reasoning_effort,
                compaction,
                persona,
                general_prompt,
                max_turns,
            };
            Ok(Some(ClaimedRun {
                input,
                run,
                settings,
                model,
            }))
        })
        .await
    }

    /// 会话当前的运行设置（不认领、不写回）。
    pub async fn run_settings(&self, session_id: SessionId) -> Result<RunSettings, StoreError> {
        self.call(move |conn| {
            let (persona, _) = resolve_persona(conn, session_id)?;
            let current = read_settings(conn)?;
            let reasoning_effort = conn.query_row(
                "SELECT reasoning_effort FROM core_sessions WHERE id = ?1",
                [session_id.0],
                |row| row.get::<_, Option<crate::ReasoningEffort>>(0),
            )?;
            Ok(RunSettings {
                reasoning_effort,
                compaction: current.compaction,
                persona,
                general_prompt: current.general_prompt,
                max_turns: current.max_turns,
            })
        })
        .await
    }

    /// 把非 held 未认领输入开头同一 person 的一段并入 `run`，要求该 person 与 run 首条输入相同；
    /// 返回并入的输入 id。调用方保证 run 处于 `Executing`。
    pub async fn absorb(&self, run: RunId) -> Result<Vec<MessageId>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let (session_id, person): (i64, i64) = tx.query_row(
                concat!(
                    "SELECT session_id, person_id FROM core_messages WHERE run_id = ?1 AND kind IN ",
                    input_kinds!(),
                    " ORDER BY id LIMIT 1"
                ),
                [run.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let Some((ids, p)) = unclaimed_head(&tx, session_id)? else {
                return Ok(Vec::new());
            };
            if p != person {
                return Ok(Vec::new());
            }
            let kind: String = tx.query_row(
                "SELECT kind FROM core_messages WHERE id = ?1",
                [ids[0]],
                |r| r.get(0),
            )?;
            if kind == "Command" {
                return Ok(Vec::new());
            }
            assign_run(&tx, run, &ids)?;
            tx.commit()?;
            Ok(ids.into_iter().map(MessageId).collect())
        })
        .await
    }

    /// 有非 held 未认领输入的会话，按 id 升序。
    pub async fn sessions_with_unclaimed_input(&self) -> Result<Vec<SessionId>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT DISTINCT m.session_id FROM core_messages m
                     WHERE {UNCLAIMED} AND NOT ({HELD_INPUT}) ORDER BY m.session_id"
                ))?
                .query_map([], |r| Ok(SessionId(r.get(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 落终态；`report` 给出时（子 run）同事务把交付写成父 session 的 Completion 并记到
    /// `core_runs.report_message_id`，返回该消息。
    pub async fn finish_run(
        &self,
        id: RunId,
        state: RunState,
        report: Option<ExecOutcome>,
        now: i64,
    ) -> Result<Option<Message>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "UPDATE core_runs SET state = ?2, finished_at = ?3 WHERE id = ?1",
                params![id.0, state.as_str(), now],
            )?;
            let report = report
                .map(|outcome| report_to_parent(&tx, id, outcome, now))
                .transpose()?;
            tx.commit()?;
            Ok(report)
        })
        .await
    }

    /// 全部 `executing` run，按 id 升序；启动时即上次遗留的。
    pub async fn executing_runs(&self) -> Result<Vec<Run>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {RUN_COLS} FROM core_runs WHERE state = 'executing' ORDER BY id"
                ))?
                .query_map([], row::run)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 同一事务内收尾一棵会话树的遗留 run：写入各自收尾消息、子 run 的交付，转为 `Interrupted`，
    /// 并写入 `notice`。要么全部生效，要么仍是 `executing` 可再收尾。
    pub async fn interrupt_runs(
        &self,
        runs: Vec<InterruptedRun>,
        notice: Option<(SessionId, MessageBody)>,
        now: i64,
    ) -> Result<(), StoreError> {
        assert!(
            runs.iter()
                .flat_map(|r| &r.closing)
                .chain(notice.iter().map(|(_, b)| b))
                .all(|b| !is_input(b) && !matches!(b, MessageBody::Reply { .. })),
            "收尾只写框架产出"
        );
        self.call(move |conn| {
            let tx = conn.transaction()?;
            for r in runs {
                let (id, session_id) = (r.run.id, r.run.session_id);
                for body in &r.closing {
                    insert_message(&tx, session_id, Some(id), body, now)?;
                }
                tx.execute(
                    "UPDATE core_runs SET state = ?2, finished_at = ?3 WHERE id = ?1",
                    params![id.0, RunState::Interrupted.as_str(), now],
                )?;
                if let Some(outcome) = r.report {
                    report_to_parent(&tx, id, outcome, now)?;
                }
            }
            if let Some((session_id, body)) = notice {
                insert_message(&tx, session_id, None, &body, now)?;
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 会话的 `executing` run（不变量保证至多一个）。
    pub async fn executing_run(&self, session_id: SessionId) -> Result<Option<RunId>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM core_runs WHERE session_id = ?1 AND state = 'executing'",
                    [session_id.0],
                    |r| Ok(RunId(r.get(0)?)),
                )
                .optional()?)
        })
        .await
    }
}

/// 子 run 的交付：父 session 的 Completion，发起人取本 run 首条输入。
fn report_to_parent(
    tx: &Transaction<'_>,
    run: RunId,
    outcome: ExecOutcome,
    at: i64,
) -> Result<Message, StoreError> {
    let job = tx.query_row(
        "SELECT j.id, j.owner_session_id, j.tool_name FROM core_jobs j
         JOIN core_runs r ON r.session_id = j.child_session_id WHERE r.id = ?1",
        [run.0],
        |r| {
            Ok((
                r.get::<_, i64>(0)?,
                SessionId(r.get(1)?),
                r.get::<_, String>(2)?,
            ))
        },
    )?;
    let (job_id, parent, tool_name) = job;
    let body = MessageBody::Completion {
        person: run_person(tx, run)?,
        tool_name,
        exec_id: job_id.to_string(),
        outcome,
    };
    let id = insert_message(tx, parent, None, &body, at)?;
    tx.execute(
        "UPDATE core_runs SET report_message_id = ?2 WHERE id = ?1",
        params![run.0, id.0],
    )?;
    Ok(Message {
        id,
        session_id: parent,
        body,
        created_at: at,
        delivered_at: None,
    })
}

/// 未认领输入按 id 排序后开头连续同 person 的一段 `(ids, person)`。
fn unclaimed_head(
    tx: &Transaction<'_>,
    session_id: i64,
) -> rusqlite::Result<Option<(Vec<i64>, i64)>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT m.id, m.person_id, m.kind FROM core_messages m
         WHERE m.session_id = ?1 AND {UNCLAIMED} AND NOT ({HELD_INPUT})
         ORDER BY m.id"
    ))?;
    let mut rows = stmt.query([session_id])?;
    let mut head: Option<(Vec<i64>, i64)> = None;
    while let Some(r) = rows.next()? {
        let (id, p): (i64, i64) = (r.get(0)?, r.get(1)?);
        let kind: String = r.get(2)?;
        match &mut head {
            None => {
                head = Some((vec![id], p));
                if kind == "Command" {
                    break;
                }
            }
            Some((ids, cur)) if *cur == p && kind != "Command" => ids.push(id),
            Some(_) => break,
        }
    }
    Ok(head)
}

fn assign_run(tx: &Transaction<'_>, run: RunId, ids: &[i64]) -> rusqlite::Result<()> {
    let mut stmt = tx.prepare("UPDATE core_messages SET run_id = ?1 WHERE id = ?2")?;
    for id in ids {
        stmt.execute(params![run.0, id])?;
    }
    Ok(())
}

/// 会话所选人设；已删除则取默认人设，布尔值表示发生了替换。
fn resolve_persona(
    conn: &rusqlite::Connection,
    session_id: SessionId,
) -> Result<(crate::Persona, bool), StoreError> {
    let persona = conn.query_row(
        &format!(
            "SELECT {PERSONA_COLS} FROM core_personas
             WHERE id = (SELECT persona_id FROM core_sessions WHERE id = ?1)"
        ),
        [session_id.0],
        row::persona,
    )?;
    if !persona.deleted {
        return Ok((persona, false));
    }
    let default = conn.query_row(
        &format!(
            "SELECT {PERSONA_COLS} FROM core_personas
             WHERE id = (SELECT default_persona_id FROM core_settings)"
        ),
        [],
        row::persona,
    )?;
    Ok((default, true))
}
