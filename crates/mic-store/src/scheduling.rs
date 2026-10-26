use mic_message::{ExecOutcome, Message, MessageBody, MessageId, SessionCommand, SessionId};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::jobs::run_person;
use crate::messages::{insert_message, is_input};
use crate::models;
use crate::row::{
    self, input_kinds, HELD_INPUT, MESSAGE_COLS, MESSAGE_FROM, PERSONA_COLS, RUN_COLS, UNCLAIMED,
};
use crate::settings::read_settings;
use crate::store::Store;
use crate::{
    ClaimedModel, ClaimedRun, InterruptedRun, Resumable, Run, RunId, RunInput, RunSettings,
    RunState, StoreError,
};

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
                    files: Vec::new(),
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
            let Some(Head { ids, .. }) = unclaimed_head(&tx, session_id.0)? else {
                return Ok(None);
            };
            let (settings, model) = claim_settings(&tx, &cipher, session_id)?;
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
            Ok(Some(ClaimedRun {
                input,
                run,
                settings,
                model,
            }))
        })
        .await
    }

    pub async fn resumable_run(
        &self,
        session_id: SessionId,
    ) -> Result<Option<Resumable>, StoreError> {
        self.call(move |conn| resumable(conn, session_id)).await
    }

    /// 仍可接续时把 `run` 置回 `executing`，按当前会话定下设置与模型（同 `claim_next`）并还原输入：
    /// 压缩指令未留下边界则重做该指令，否则继续对话，未答复的输入为本 run 末条 `Reply` 之后的输入。
    pub async fn resume_run(
        &self,
        session_id: SessionId,
        run: RunId,
    ) -> Result<Option<ClaimedRun>, StoreError> {
        let cipher = self.cipher();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            if resumable(&tx, session_id)?.map(|r| r.run_id) != Some(run) {
                return Ok(None);
            }
            let created_at = tx.query_row(
                "UPDATE core_runs SET state = ?2, finished_at = NULL WHERE id = ?1
                 RETURNING created_at",
                params![run.0, RunState::Executing.as_str()],
                |r| r.get(0),
            )?;
            let inputs = tx
                .prepare(concat!(
                    "SELECT id FROM core_messages WHERE run_id = ?1 AND kind IN ",
                    input_kinds!(),
                    " ORDER BY id"
                ))?
                .query_map([run.0], |r| r.get::<_, i64>(0))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let first = tx.query_row(
                &format!("SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.id = ?1"),
                [inputs[0]],
                row::message,
            )?;
            let (last_reply, compacted): (Option<i64>, bool) = tx.query_row(
                "SELECT MAX(CASE WHEN kind = 'Reply' THEN id END),
                        COUNT(CASE WHEN kind = 'Boundary' THEN 1 END) > 0
                 FROM core_messages WHERE run_id = ?1",
                [run.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let input = match first.body {
                MessageBody::Command {
                    command: command @ SessionCommand::Compact { .. },
                    ..
                } if !compacted => RunInput::Command {
                    message_id: first.id,
                    command,
                },
                _ => RunInput::Conversation {
                    message_ids: inputs
                        .into_iter()
                        .filter(|id| last_reply.is_none_or(|r| *id > r))
                        .map(MessageId)
                        .collect(),
                },
            };
            let (settings, model) = claim_settings(&tx, &cipher, session_id)?;
            tx.commit()?;
            Ok(Some(ClaimedRun {
                input,
                run: Run {
                    id: run,
                    session_id,
                    state: RunState::Executing,
                    created_at,
                    finished_at: None,
                },
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
    /// `Trigger` 开头的 run 并入任何人的输入。返回并入的输入 id。调用方保证 run 处于 `Executing`。
    pub async fn absorb(&self, run: RunId) -> Result<Vec<MessageId>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let (session_id, person): (i64, Option<i64>) = tx.query_row(
                concat!(
                    "SELECT session_id, person_id FROM core_messages WHERE run_id = ?1 AND kind IN ",
                    input_kinds!(),
                    " ORDER BY id LIMIT 1"
                ),
                [run.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let Some(head) = unclaimed_head(&tx, session_id)? else {
                return Ok(Vec::new());
            };
            if head.exclusive || person.is_some_and(|p| head.person != Some(p)) {
                return Ok(Vec::new());
            }
            assign_run(&tx, run, &head.ids)?;
            tx.commit()?;
            Ok(head.ids.into_iter().map(MessageId).collect())
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

    /// 会话中排队等待认领的输入（未认领且非 held，模型尚不可见），按 id 升序。
    pub async fn queued_inputs(&self, session_id: SessionId) -> Result<Vec<MessageId>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT m.id FROM core_messages m
                     WHERE m.session_id = ?1 AND {UNCLAIMED} AND NOT ({HELD_INPUT})
                     ORDER BY m.id"
                ))?
                .query_map([session_id.0], |r| Ok(MessageId(r.get(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
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

/// 未认领输入开头可成一个 run 的一段。
struct Head {
    ids: Vec<i64>,
    /// `Trigger` 无发起人。
    person: Option<i64>,
    /// `Command`/`Trigger` 独占一个 run。
    exclusive: bool,
}

/// 未认领输入按 id 排序后开头的一段：独占输入单独一条，否则连续同 person。
fn unclaimed_head(tx: &Transaction<'_>, session_id: i64) -> rusqlite::Result<Option<Head>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT m.id, m.person_id, m.kind FROM core_messages m
         WHERE m.session_id = ?1 AND {UNCLAIMED} AND NOT ({HELD_INPUT})
         ORDER BY m.id"
    ))?;
    let mut rows = stmt.query([session_id])?;
    let mut head: Option<Head> = None;
    while let Some(r) = rows.next()? {
        let (id, person): (i64, Option<i64>) = (r.get(0)?, r.get(1)?);
        let kind: String = r.get(2)?;
        let exclusive = matches!(kind.as_str(), "Command" | "Trigger");
        match &mut head {
            None => {
                head = Some(Head {
                    ids: vec![id],
                    person,
                    exclusive,
                });
                if exclusive {
                    break;
                }
            }
            Some(h) if h.person == person && !exclusive => h.ids.push(id),
            Some(_) => break,
        }
    }
    Ok(head)
}

/// 本轮设置与模型：会话所选人设（已删除则改用默认人设并写回会话）与当前偏好。
fn claim_settings(
    tx: &Transaction<'_>,
    cipher: &crate::secrets::Cipher,
    session_id: SessionId,
) -> Result<(RunSettings, ClaimedModel), StoreError> {
    let (persona, replaced) = resolve_persona(tx, session_id)?;
    if replaced {
        tx.execute(
            "UPDATE core_sessions SET persona_id = ?1 WHERE id = ?2",
            params![persona.id.0, session_id.0],
        )?;
    }
    let current = read_settings(tx)?;
    let model = models::claim_model(tx, cipher, session_id)?;
    let reasoning_effort = tx.query_row(
        "SELECT reasoning_effort FROM core_sessions WHERE id = ?1",
        [session_id.0],
        |row| row.get::<_, Option<crate::ReasoningEffort>>(0),
    )?;
    let settings = RunSettings {
        reasoning_effort,
        compaction: current.compaction,
        persona,
        general_prompt: current.general_prompt,
        max_turns: current.max_turns,
    };
    Ok((settings, model))
}

/// 见 [`Resumable`]；任务会话与已归档会话不可接续。
fn resumable(
    conn: &rusqlite::Connection,
    session_id: SessionId,
) -> Result<Option<Resumable>, StoreError> {
    let latest = conn
        .query_row(
            "SELECT r.id, r.state FROM core_runs r JOIN core_sessions s ON s.id = r.session_id
             WHERE r.session_id = ?1 AND s.kind <> 'task'
             ORDER BY r.id DESC LIMIT 1",
            [session_id.0],
            |r| Ok((RunId(r.get(0)?), row::run_state(r, 1)?)),
        )
        .optional()?;
    let Some((run_id, state)) = latest else {
        return Ok(None);
    };
    if !matches!(
        state,
        RunState::ProviderFailed | RunState::MaxTurns | RunState::Interrupted | RunState::TimedOut
    ) || crate::sessions::archived(conn, session_id)?
    {
        return Ok(None);
    }
    let queued: bool = conn.query_row(
        &format!(
            "SELECT EXISTS(SELECT 1 FROM core_messages m
             WHERE m.session_id = ?1 AND {UNCLAIMED} AND NOT ({HELD_INPUT}))"
        ),
        [session_id.0],
        |r| r.get(0),
    )?;
    if queued {
        return Ok(None);
    }
    Ok(Some(Resumable { run_id, state }))
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

#[cfg(test)]
mod tests;
