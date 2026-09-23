//! 领域类型与行之间的投影/还原。判别列只在这里生成。

use mic_message::{
    BoundaryEntry, Message, MessageAuthor, MessageContent, PersonId, SessionEntry, SessionEntryId,
    SessionId,
};
use rusqlite::types::Type;
use rusqlite::Row;
use serde::de::DeserializeOwned;

use crate::{
    CancelReason, DeliveryTarget, FailureReason, Query, QueryId, QueryState, Session, SessionKind,
    StoreError,
};

pub(crate) const ENTRY_COLS: &str =
    "e.id, e.session_id, e.entry_kind, e.author_kind, e.author_ident, e.payload, e.created_at, e.delivered_at";

pub(crate) const SESSION_COLS: &str =
    "id, kind, channel, chat, parent_tool_call_id, trigger_module, \
     trigger_ref, parent_session_id, delivery_channel, delivery_version, delivery_payload, pwd, \
     tool_scope, created_at";

pub(crate) const QUERY_COLS: &str =
    "id, session_id, person_id, claimed_start_id, claimed_end_id, state, reason, created_at";

pub(crate) fn author_cols(author: &MessageAuthor) -> (&'static str, Option<String>) {
    match author {
        MessageAuthor::User { id } => ("user", Some(id.0.to_string())),
        MessageAuthor::Assistant { model } => ("assistant", Some(model.clone())),
        MessageAuthor::Tool { name } => ("tool", Some(name.clone())),
        MessageAuthor::HarnessNote => ("harness_note", None),
        MessageAuthor::Notification { source } => ("notification", Some(source.clone())),
    }
}

pub(crate) fn content_kind(content: &MessageContent) -> &'static str {
    match content {
        MessageContent::Text { .. } => "text",
        MessageContent::Reasoning(_) => "reasoning",
        MessageContent::ToolCall { .. } => "tool_call",
        MessageContent::ToolResult { .. } => "tool_result",
        MessageContent::Completion { .. } => "completion",
        MessageContent::Attachment(_) => "attachment",
    }
}

pub(crate) fn state_cols(state: &QueryState) -> Result<(&'static str, Option<String>), StoreError> {
    Ok(match state {
        QueryState::Executing => ("executing", None),
        QueryState::Completed => ("completed", None),
        QueryState::Failed { reason } => ("failed", Some(serde_json::to_string(reason)?)),
        QueryState::Cancelled { reason } => ("cancelled", Some(serde_json::to_string(reason)?)),
    })
}

pub(crate) struct SessionKindCols<'a> {
    pub kind: &'static str,
    pub channel: Option<&'a str>,
    pub chat: Option<&'a str>,
    pub parent_tool_call_id: Option<&'a str>,
    pub trigger_module: Option<&'a str>,
    pub trigger_ref: Option<&'a str>,
}

pub(crate) fn session_kind_cols(kind: &SessionKind) -> SessionKindCols<'_> {
    let none = SessionKindCols {
        kind: "",
        channel: None,
        chat: None,
        parent_tool_call_id: None,
        trigger_module: None,
        trigger_ref: None,
    };
    match kind {
        SessionKind::Root { channel, chat } => SessionKindCols {
            kind: "root",
            channel: Some(channel),
            chat: Some(chat),
            ..none
        },
        SessionKind::Task {
            parent_tool_call_id,
        } => SessionKindCols {
            kind: "task",
            parent_tool_call_id: Some(parent_tool_call_id),
            ..none
        },
        SessionKind::Triggered { module, ref_id } => SessionKindCols {
            kind: "triggered",
            trigger_module: Some(module),
            trigger_ref: Some(ref_id),
            ..none
        },
    }
}

fn corrupt(idx: usize, msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, msg.into())
}

fn json_col<T: DeserializeOwned>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let text: String = row.get(idx)?;
    serde_json::from_str(&text)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
}

/// 列序同 `ENTRY_COLS`。
pub(crate) fn entry(row: &Row<'_>) -> rusqlite::Result<SessionEntry> {
    let id = SessionEntryId(row.get(0)?);
    let session_id = SessionId(row.get(1)?);
    let entry_kind: String = row.get(2)?;
    let created_at = row.get(6)?;
    match entry_kind.as_str() {
        "message" => Ok(SessionEntry::Message(Message {
            id,
            session_id,
            author: author(row, 3, 4)?,
            content: json_col(row, 5)?,
            created_at,
            delivered_at: row.get(7)?,
        })),
        "boundary" => Ok(SessionEntry::Boundary(BoundaryEntry {
            id,
            session_id,
            boundary: json_col(row, 5)?,
            created_at,
        })),
        other => Err(corrupt(2, format!("未知 entry_kind: {other}"))),
    }
}

/// 列序同 `ENTRY_COLS`，只接受 message 行。
pub(crate) fn message(row: &Row<'_>) -> rusqlite::Result<Message> {
    match entry(row)? {
        SessionEntry::Message(m) => Ok(m),
        SessionEntry::Boundary(_) => Err(corrupt(2, "期望 message 行".to_owned())),
    }
}

fn author(row: &Row<'_>, kind_idx: usize, ident_idx: usize) -> rusqlite::Result<MessageAuthor> {
    let kind: String = row.get(kind_idx)?;
    let ident: Option<String> = row.get(ident_idx)?;
    Ok(match (kind.as_str(), ident) {
        ("user", Some(id)) => MessageAuthor::User {
            id: PersonId(id.parse().map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(ident_idx, Type::Text, Box::new(e))
            })?),
        },
        ("assistant", Some(model)) => MessageAuthor::Assistant { model },
        ("tool", Some(name)) => MessageAuthor::Tool { name },
        ("harness_note", None) => MessageAuthor::HarnessNote,
        ("notification", Some(source)) => MessageAuthor::Notification { source },
        (kind, ident) => return Err(corrupt(kind_idx, format!("非法 author: {kind}/{ident:?}"))),
    })
}

/// 从 `idx` 起连续三列 channel/version/payload；三列同为 NULL 表示无目标。
pub(crate) fn delivery_target(
    row: &Row<'_>,
    idx: usize,
) -> rusqlite::Result<Option<DeliveryTarget>> {
    let channel: Option<String> = row.get(idx)?;
    let Some(channel) = channel else {
        return Ok(None);
    };
    Ok(Some(DeliveryTarget {
        channel,
        version: row.get(idx + 1)?,
        payload: row.get(idx + 2)?,
    }))
}

/// 列序同 `SESSION_COLS`。
pub(crate) fn session(row: &Row<'_>) -> rusqlite::Result<Session> {
    let kind: String = row.get(1)?;
    let kind = match kind.as_str() {
        "root" => SessionKind::Root {
            channel: row.get(2)?,
            chat: row.get(3)?,
        },
        "task" => SessionKind::Task {
            parent_tool_call_id: row.get(4)?,
        },
        "triggered" => SessionKind::Triggered {
            module: row.get(5)?,
            ref_id: row.get(6)?,
        },
        other => return Err(corrupt(1, format!("未知 session kind: {other}"))),
    };
    Ok(Session {
        id: SessionId(row.get(0)?),
        kind,
        parent_session_id: row.get::<_, Option<i64>>(7)?.map(SessionId),
        delivery_target: delivery_target(row, 8)?,
        pwd: row.get(11)?,
        tool_scope: json_col(row, 12)?,
        created_at: row.get(13)?,
    })
}

/// 列序同 `QUERY_COLS`。
pub(crate) fn query(row: &Row<'_>) -> rusqlite::Result<Query> {
    let state: String = row.get(5)?;
    let state = match state.as_str() {
        "executing" => QueryState::Executing,
        "completed" => QueryState::Completed,
        "failed" => QueryState::Failed {
            reason: json_col::<FailureReason>(row, 6)?,
        },
        "cancelled" => QueryState::Cancelled {
            reason: json_col::<CancelReason>(row, 6)?,
        },
        other => return Err(corrupt(5, format!("未知 query state: {other}"))),
    };
    Ok(Query {
        id: QueryId(row.get(0)?),
        session_id: SessionId(row.get(1)?),
        person_id: PersonId(row.get(2)?),
        claimed_start_id: SessionEntryId(row.get(3)?),
        claimed_end_id: SessionEntryId(row.get(4)?),
        state,
        created_at: row.get(7)?,
    })
}
