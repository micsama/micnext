//! 领域类型与行之间的投影/还原。

use mic_message::{Message, MessageBody, MessageId, SessionId};
use rusqlite::types::Type;
use rusqlite::Row;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    DeliveryTarget, Persona, PersonaId, Run, RunId, RunState, Session, SessionKind, StoreError,
};

/// 消息读取的 FROM 子句：`Reply.model` 只存在调用行上，读出时 JOIN 还原。
pub(crate) const MESSAGE_FROM: &str =
    "core_messages m LEFT JOIN core_model_calls c ON c.id = m.model_call_id";

pub(crate) const MESSAGE_COLS: &str =
    "m.id, m.session_id, m.payload, m.created_at, m.delivered_at, c.model";

pub(crate) const SESSION_COLS: &str =
    "id, kind, channel, chat, parent_tool_call_id, trigger_module, \
     trigger_ref, parent_session_id, delivery_channel, delivery_version, delivery_payload, pwd, \
     tool_scope, created_at, persona_id";

pub(crate) const RUN_COLS: &str = "id, session_id, state, created_at, finished_at";

pub(crate) const PERSONA_COLS: &str = "id, name, prompt, builtin, deleted_at IS NOT NULL";

/// 未认领输入谓词，`m` 为 core_messages 别名。
pub(crate) const UNCLAIMED: &str = "m.run_id IS NULL AND m.kind IN ('UserInput', 'Completion')";

/// `MessageBody` → payload；`Reply.model` 不进 payload。
pub(crate) fn payload(body: &MessageBody) -> Result<String, StoreError> {
    let mut value = serde_json::to_value(body)?;
    if matches!(body, MessageBody::Reply { .. }) {
        value
            .as_object_mut()
            .expect("MessageBody 序列化为对象")
            .remove("model");
    }
    Ok(value.to_string())
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

/// 列序同 `MESSAGE_COLS`。
pub(crate) fn message(row: &Row<'_>) -> rusqlite::Result<Message> {
    let mut payload: Value = json_col(row, 2)?;
    if let Some(model) = row.get::<_, Option<String>>(5)? {
        payload
            .as_object_mut()
            .ok_or_else(|| corrupt(2, "payload 不是对象".to_owned()))?
            .insert("model".to_owned(), Value::String(model));
    }
    let body = serde_json::from_value(payload)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(2, Type::Text, Box::new(e)))?;
    Ok(Message {
        id: MessageId(row.get(0)?),
        session_id: SessionId(row.get(1)?),
        body,
        created_at: row.get(3)?,
        delivered_at: row.get(4)?,
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
        persona_id: PersonaId(row.get(14)?),
    })
}

/// 列序同 `PERSONA_COLS`。
pub(crate) fn persona(row: &Row<'_>) -> rusqlite::Result<Persona> {
    Ok(Persona {
        id: PersonaId(row.get(0)?),
        name: row.get(1)?,
        prompt: row.get(2)?,
        builtin: row.get(3)?,
        deleted: row.get(4)?,
    })
}

/// 列序同 `RUN_COLS`。
pub(crate) fn run(row: &Row<'_>) -> rusqlite::Result<Run> {
    let state: String = row.get(2)?;
    let state = match state.as_str() {
        "executing" => RunState::Executing,
        "completed" => RunState::Completed,
        "provider_failed" => RunState::ProviderFailed,
        "max_turns" => RunState::MaxTurns,
        "interrupted" => RunState::Interrupted,
        other => return Err(corrupt(2, format!("未知 run state: {other}"))),
    };
    Ok(Run {
        id: RunId(row.get(0)?),
        session_id: SessionId(row.get(1)?),
        state,
        created_at: row.get(3)?,
        finished_at: row.get(4)?,
    })
}
