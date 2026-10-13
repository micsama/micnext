use mic_message::{
    Compaction, ContextBoundary, Message, MessageBody, MessageId, PersonId, SessionCommand,
    SessionId,
};
use rusqlite::{params, Connection, OptionalExtension};

use crate::row::{self, HELD_INPUT, MESSAGE_COLS, MESSAGE_FROM, UNCLAIMED};
use crate::store::insert_message;
use crate::{
    CompactionState, CompactionView, ContextCommandError, ContextUsage, ContextView, ContextWindow,
    ModelCallId, ModelCallOutcome, ModelId, NewModelCall, RunId, Store, StoreError,
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
                    let at: Option<i64> = row.get(6)?;
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
            let (start, summary, files) = match boundary {
                Some(Message { id, body: MessageBody::Boundary { boundary: ContextBoundary::Compaction(result) }, .. }) => (
                    result.keep_from.map_or(id.0 + 1, |m| m.0), Some(result.summary), result.files,
                ),
                Some(message) => (message.id.0 + 1, None, Vec::new()),
                None => (0, None, Vec::new()),
            };
            let messages = conn.prepare(&format!(
                "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                 WHERE m.session_id = ?1
                   AND (m.id >= ?2 OR EXISTS (SELECT 1 FROM core_boundary_inputs b WHERE b.boundary_id = ?3 AND b.message_id = m.id))
                   AND (NOT ({UNCLAIMED}) OR {HELD_INPUT}) ORDER BY m.id"
            ))?.query_map(params![session.0, start, boundary_id.map(|id| id.0)], row::message)?.collect::<rusqlite::Result<Vec<_>>>()?;
            let usage = conn.query_row(
                "SELECT r.model_id, c.input_tokens, c.context_through_id
                 FROM core_model_calls c JOIN core_runs r ON r.id = c.run_id
                 WHERE c.session_id = ?1 AND c.error IS NULL AND c.input_tokens IS NOT NULL
                   AND c.context_boundary_id IS ?2 AND c.context_through_id IS NOT NULL AND r.model_id IS NOT NULL
                 ORDER BY c.id DESC LIMIT 1",
                params![session.0, boundary_id.map(|id| id.0)],
                |r| Ok(ContextUsage { model_id: ModelId(r.get(0)?), input_tokens: r.get::<_, i64>(1)? as u64, through: r.get::<_, Option<i64>>(2)?.map(MessageId) }),
            ).optional()?;
            Ok(ContextWindow { boundary_id, summary, files, messages, usage })
        }).await
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
            insert_call(&tx, &call)?;
            let kept: u32 = match result.keep_from {
                Some(start) => tx.query_row("SELECT count(*) FROM core_messages WHERE session_id = ?1 AND kind = 'Reply' AND id >= ?2", params![session.0, start.0], |r| r.get(0))?,
                None => 0,
            };
            let boundary = append(&tx, session, run, MessageBody::Boundary { boundary: ContextBoundary::Compaction(result) }, now)?;
            preserve_pending(&tx, session, boundary.id, None)?;
            for input in unanswered {
                tx.execute("INSERT OR IGNORE INTO core_boundary_inputs VALUES (?1, ?2)", params![boundary.id.0, input.0])?;
            }
            let notice = append(&tx, session, run, notice(format!("已压缩上下文，保留最近 {kept} 步。")), now)?;
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
    }
}

pub(crate) fn insert_call(
    conn: &Connection,
    call: &NewModelCall,
) -> Result<ModelCallId, StoreError> {
    let (error, usage) = match &call.outcome {
        ModelCallOutcome::Replied { usage, .. } => (None, *usage),
        ModelCallOutcome::Failed { error } => (Some(error.as_str()), None),
    };
    let tokens = |f: fn(&crate::Usage) -> Option<u64>| usage.as_ref().and_then(f).map(|n| n as i64);
    conn.execute(
        "INSERT INTO core_model_calls (session_id, run_id, model, error, input_tokens, output_tokens, cache_read_tokens, cache_write_tokens, reasoning_tokens, started_at, finished_at, context_boundary_id, context_through_id, purpose) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![call.session_id.0, call.run_id.map(|r| r.0), call.model, error,
            tokens(|u| Some(u.input_tokens)), tokens(|u| Some(u.output_tokens)), tokens(|u| u.cache_read_tokens), tokens(|u| u.cache_write_tokens), tokens(|u| u.reasoning_tokens), call.started_at, call.finished_at, call.context.boundary_id.map(|id| id.0), call.context.through.map(|id| id.0), call.purpose],
    )?;
    Ok(ModelCallId(conn.last_insert_rowid()))
}
