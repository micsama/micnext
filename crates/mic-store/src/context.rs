use mic_message::{
    Compaction, ContextBoundary, Message, MessageBody, MessageId, PersonId, SessionCommand,
    SessionId,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::messages::{insert_message, read_messages};
use crate::row::{self, HELD_INPUT, MESSAGE_COLS, MESSAGE_FROM, UNCLAIMED};
use crate::{
    CompactionState, CompactionView, ContextCommandError, ContextUsage, ContextView, ContextWindow,
    ModelCallId, ModelCallOutcome, NewModelCall, RunId, Store, StoreError,
};

impl Store {
    pub async fn append_command(
        &self,
        session: SessionId,
        person: PersonId,
        command: SessionCommand,
        now: i64,
    ) -> Result<Message, StoreError> {
        self.append(session, None, MessageBody::Command { person, command }, now)
            .await
    }

    pub async fn context_view(&self, session: SessionId) -> Result<ContextView, StoreError> {
        self.call(move |conn| {
            let active_boundary = active_boundary(conn, session)?.map(|m| m.id);
            let mut stmt = conn.prepare(&format!("SELECT {MESSAGE_COLS}, m.compaction_revoked_at FROM {MESSAGE_FROM} WHERE m.session_id = ?1 AND m.kind = 'Boundary' ORDER BY m.id"))?;
            let mut compactions = Vec::new();
            let mut rows = stmt.query([session.0])?;
            while let Some(row) = rows.next()? {
                let message = row::message(row)?;
                if let MessageBody::Boundary { boundary: ContextBoundary::Compaction(result) } = message.body {
                    let at: Option<i64> = row.get(row::MESSAGE_NEXT)?;
                    compactions.push(CompactionView { boundary_id: message.id, result, state: match at { Some(at) => CompactionState::Revoked { at }, None => CompactionState::Applied } });
                }
            }
            Ok(ContextView { active_boundary, compactions })
        }).await
    }

    pub async fn context_window(&self, session: SessionId) -> Result<ContextWindow, StoreError> {
        self.call(move |conn| {
            let boundary = active_boundary(conn, session)?;
            let boundary_id = boundary.as_ref().map(|m| m.id);
            let start = window_start(boundary);
            let mut messages = Vec::new();
            let mut skipped = Vec::new();
            for (message, filtered) in read_range(conn, session, &start, boundary_id, None)? {
                if filtered {
                    skipped.push(message.id);
                } else {
                    messages.push(message);
                }
            }
            let through = messages.iter().map(|m| m.id).max();
            let filtered = skipped
                .into_iter()
                .filter(|id| through.is_some_and(|t| *id < t))
                .collect();
            let quoted = match &start.compaction {
                Some(c) => read_messages(conn, &c.quoted)?,
                None => Vec::new(),
            };
            let (summary, files) = match start.compaction {
                Some(c) => (Some(c.summary), c.files),
                None => (None, Vec::new()),
            };
            let plain_after = conn.query_row(
                "SELECT plain_after FROM core_sessions WHERE id = ?1",
                [session.0],
                |r| r.get::<_, Option<i64>>(0),
            )?;
            Ok(ContextWindow {
                boundary_id,
                summary,
                files,
                messages,
                filtered,
                quoted,
                plain_after: plain_after.map(MessageId),
            })
        })
        .await
    }

    pub async fn context_usage(
        &self,
        session: SessionId,
    ) -> Result<Option<ContextUsage>, StoreError> {
        self.call(move |conn| {
            let boundary_id = active_boundary(conn, session)?.map(|m| m.id.0);
            Ok(conn
                .query_row(
                    "SELECT id, input_tokens, output_tokens FROM core_model_calls
                     WHERE session_id = ?1 AND purpose = 'conversation' AND error IS NULL
                       AND input_tokens IS NOT NULL AND output_tokens IS NOT NULL
                       AND context_boundary_id IS ?2
                     ORDER BY id DESC LIMIT 1",
                    params![session.0, boundary_id],
                    |r| {
                        Ok(ContextUsage {
                            call: ModelCallId(r.get(0)?),
                            input_tokens: r.get::<_, i64>(1)? as u64,
                            output_tokens: r.get::<_, i64>(2)? as u64,
                        })
                    },
                )
                .optional()?)
        })
        .await
    }

    /// 某次调用当时的窗口：按其边界读到 `through`，不看认领状态，排除项由调用方按记录剔除。
    pub async fn window_at(
        &self,
        session: SessionId,
        boundary: Option<MessageId>,
        through: Option<MessageId>,
    ) -> Result<Vec<Message>, StoreError> {
        let Some(through) = through else {
            return Ok(Vec::new());
        };
        self.call(move |conn| {
            let boundary_message = match boundary {
                Some(id) => Some(conn.query_row(
                    &format!("SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.id = ?1"),
                    [id.0],
                    row::message,
                )?),
                None => None,
            };
            let start = window_start(boundary_message);
            Ok(read_range(conn, session, &start, boundary, Some(through))?
                .into_iter()
                .map(|(message, _)| message)
                .collect())
        })
        .await
    }

    pub async fn commit_compaction(
        &self,
        session: SessionId,
        run: RunId,
        result: Compaction,
        unanswered: Vec<MessageId>,
        call: NewModelCall,
        now: i64,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let call_id = insert_call(&tx, &call)?;
            let (elided, quoted) = (result.elided.clone(), result.quoted.clone());
            let kept: u32 = match result.keep_from {
                Some(start) => tx.query_row("SELECT count(*) FROM core_messages WHERE session_id = ?1 AND kind = 'Reply' AND (created_at, id) >= (SELECT created_at, id FROM core_messages WHERE id = ?2)", params![session.0, start.0], |r| r.get(0))?,
                None => 0,
            };
            let boundary = append(&tx, session, run, MessageBody::Boundary { boundary: ContextBoundary::Compaction(result) }, now)?;
            tx.execute("UPDATE core_messages SET model_call_id = ?2 WHERE id = ?1", params![boundary.id.0, call_id.0])?;
            preserve_pending(&tx, session, boundary.id, None)?;
            for input in unanswered {
                tx.execute("INSERT OR IGNORE INTO core_boundary_inputs VALUES (?1, ?2)", params![boundary.id.0, input.0])?;
            }
            let mut text = format!("已压缩上下文，保留最近 {kept} 步");
            if !elided.is_empty() {
                text += &format!("，省略 {} 条工具输出", elided.len());
            }
            if !quoted.is_empty() {
                text += &format!("，带回 {} 条用户原话", quoted.len());
            }
            let notice = append(&tx, session, run, notice(text + "。"), now)?;
            tx.commit()?;
            Ok(vec![boundary, notice])
        }).await
    }

    pub async fn clear_context(
        &self,
        session: SessionId,
        run: RunId,
        command_id: MessageId,
        now: i64,
    ) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let boundary = append(
                &tx,
                session,
                run,
                MessageBody::Boundary {
                    boundary: ContextBoundary::UserClear,
                },
                now,
            )?;
            preserve_pending(&tx, session, boundary.id, Some(command_id))?;
            let notice = append(
                &tx,
                session,
                run,
                notice("已清空上下文，之前的对话不再带给模型。".into()),
                now,
            )?;
            tx.commit()?;
            Ok(vec![boundary, notice])
        })
        .await
    }

    pub async fn revoke_compaction(
        &self,
        session: SessionId,
        run: RunId,
        boundary_id: MessageId,
        now: i64,
    ) -> Result<Vec<Message>, ContextCommandError> {
        self.blocking(move |conn| {
            let tx = conn.transaction()?;
            let exists: bool = tx.query_row("SELECT EXISTS (SELECT 1 FROM core_messages WHERE id = ?1 AND session_id = ?2 AND kind = 'Boundary')", params![boundary_id.0, session.0], |r| r.get(0))?;
            if !exists { return Err(ContextCommandError::BoundaryNotFound); }
            match active_boundary(&tx, session)? {
                Some(Message { id, body: MessageBody::Boundary { boundary: ContextBoundary::Compaction(_) }, .. }) if id == boundary_id => {}
                _ => return Err(ContextCommandError::NotCurrentCompaction),
            }
            tx.execute("UPDATE core_messages SET compaction_revoked_at = ?2 WHERE id = ?1", params![boundary_id.0, now])?;
            if let Some(previous) = active_boundary(&tx, session)? {
                tx.execute("INSERT OR IGNORE INTO core_boundary_inputs (boundary_id, message_id) SELECT ?1, message_id FROM core_boundary_inputs WHERE boundary_id = ?2", params![previous.id.0, boundary_id.0])?;
            }
            let notification = append(&tx, session, run, notice("已撤回压缩，恢复之前的上下文。".into()), now)?;
            tx.commit()?;
            Ok(vec![notification])
        }).await
    }
}

fn active_boundary(conn: &Connection, session: SessionId) -> Result<Option<Message>, StoreError> {
    Ok(conn.query_row(&format!("SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.session_id = ?1 AND m.kind = 'Boundary' AND m.compaction_revoked_at IS NULL ORDER BY m.id DESC LIMIT 1"), [session.0], row::message).optional()?)
}

fn preserve_pending(
    conn: &Connection,
    session: SessionId,
    boundary: MessageId,
    after: Option<MessageId>,
) -> Result<(), StoreError> {
    conn.execute(&format!("INSERT INTO core_boundary_inputs (boundary_id, message_id) SELECT ?2, m.id FROM core_messages m WHERE m.session_id = ?1 AND {UNCLAIMED} AND NOT ({HELD_INPUT}) AND (?3 IS NULL OR m.id > ?3)"), params![session.0, boundary.0, after.map(|id| id.0)])?;
    Ok(())
}

fn append(
    conn: &Connection,
    session: SessionId,
    run: RunId,
    body: MessageBody,
    now: i64,
) -> Result<Message, StoreError> {
    Ok(Message {
        id: insert_message(conn, session, Some(run), &body, now)?,
        session_id: session,
        body,
        created_at: now,
        delivered_at: None,
    })
}

fn notice(text: String) -> MessageBody {
    MessageBody::Notification {
        source: "micnext".into(),
        text,
        files: Vec::new(),
    }
}

/// 窗口起点（含）与当前压缩边界：压缩边界按 `keep_from`，其余边界之后全部。
struct WindowStart {
    start: i64,
    compaction: Option<Compaction>,
}

fn window_start(boundary: Option<Message>) -> WindowStart {
    match boundary {
        Some(Message {
            id,
            body:
                MessageBody::Boundary {
                    boundary: ContextBoundary::Compaction(result),
                },
            ..
        }) => WindowStart {
            start: id.0 + 1,
            compaction: Some(result),
        },
        Some(message) => WindowStart {
            start: message.id.0 + 1,
            compaction: None,
        },
        None => WindowStart {
            start: 0,
            compaction: None,
        },
    }
}

/// 窗口范围内的全部消息（按位置，写入范围至多到 `through`），边界 `elided` 已换成占位；
/// 布尔值 = 未认领且非 held，对话窗口不可见。
fn read_range(
    conn: &Connection,
    session: SessionId,
    start: &WindowStart,
    boundary: Option<MessageId>,
    through: Option<MessageId>,
) -> Result<Vec<(Message, bool)>, StoreError> {
    let mut rows = conn.prepare(&format!(
        "SELECT {MESSAGE_COLS}, ({UNCLAIMED}) AND NOT ({HELD_INPUT}) FROM {MESSAGE_FROM}
         WHERE m.session_id = ?1
           AND ((CASE WHEN ?5 IS NULL THEN m.id >= ?2 ELSE (m.created_at, m.id) >= (SELECT created_at, id FROM core_messages WHERE id = ?5) END) OR EXISTS (SELECT 1 FROM core_boundary_inputs b WHERE b.boundary_id = ?3 AND b.message_id = m.id))
           AND m.id <= ?4 ORDER BY m.created_at, m.id"
    ))?.query_map(
        params![session.0, start.start, boundary.map(|id| id.0), through.map_or(i64::MAX, |id| id.0), start.compaction.as_ref().and_then(|c| c.keep_from).map(|id| id.0)],
        |r| Ok((row::message(r)?, r.get(row::MESSAGE_NEXT)?)),
    )?.collect::<rusqlite::Result<Vec<_>>>()?;
    if let Some(c) = &start.compaction {
        for (message, _) in &mut rows {
            if c.elided.contains(&message.id) {
                message.elide_output();
            }
        }
    }
    Ok(rows)
}

pub(crate) fn insert_call(
    conn: &Connection,
    call: &NewModelCall,
) -> Result<ModelCallId, StoreError> {
    let error = match &call.outcome {
        ModelCallOutcome::Replied { .. } => None,
        ModelCallOutcome::Failed { error } => Some(error.as_str()),
    };
    let tokens =
        |f: fn(&crate::Usage) -> Option<u64>| call.usage.as_ref().and_then(f).map(|n| n as i64);
    let context = &call.context;
    conn.execute(
        "INSERT INTO core_model_calls
           (session_id, run_id, purpose, model, model_id, reasoning_effort, error, stop,
            input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens,
            started_at, first_output_at, finished_at, context_boundary_id, context_through_id, context_skip, context_keep_from)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19, ?20)",
        params![call.session_id.0, call.run_id.map(|r| r.0), call.purpose, call.model, call.model_id.map(|id| id.0), call.reasoning_effort, error, call.stop,
            tokens(|u| Some(u.input_tokens)), tokens(|u| Some(u.output_tokens)), tokens(|u| u.cache_read_tokens), tokens(|u| u.cache_write_tokens), tokens(|u| u.reasoning_tokens),
            call.started_at, call.first_output_at, call.finished_at, context.boundary_id.map(|id| id.0), context.through.map(|id| id.0), context.skip, context.keep_from.map(|id| id.0)],
    )?;
    let id = ModelCallId(conn.last_insert_rowid());
    for (role, ids) in [
        ("excluded", &context.excluded),
        ("instruction", &context.instruction),
        ("elide", &context.elide),
    ] {
        for message in ids {
            conn.execute(
                "INSERT OR IGNORE INTO core_model_call_inputs (call_id, message_id, role) VALUES (?1, ?2, ?3)",
                params![id.0, message.0, role],
            )?;
        }
    }
    Ok(id)
}
