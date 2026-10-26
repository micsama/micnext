//! 领域类型与行之间的投影/还原。

use mic_message::{Message, MessageBody, MessageId, SessionId};
use rusqlite::types::Type;
use rusqlite::Row;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::{
    DeliveryTarget, ModelCallPurpose, ModelId, Persona, PersonaId, ReasoningEffort, Run, RunId,
    RunState, Session, SessionKind, StopReason, StoreError,
};

/// 消息读取的 FROM 子句：回复的模型与统计只存在调用行上，读出时 JOIN 还原。
pub(crate) const MESSAGE_FROM: &str =
    "core_messages m LEFT JOIN core_model_calls c ON c.id = m.model_call_id";

pub(crate) const MESSAGE_COLS: &str =
    "m.id, m.session_id, m.payload, m.created_at, m.delivered_at, c.model, c.finished_at - c.started_at, c.first_output_at - c.started_at, c.input_tokens, c.output_tokens, c.cache_read_tokens";

/// 紧跟 `MESSAGE_COLS` 追加的列从这个序号开始。
pub(crate) const MESSAGE_NEXT: usize = 11;

pub(crate) const SESSION_COLS: &str = "id, kind, channel, chat, trigger_module, \
     trigger_ref, parent_session_id, delivery_channel, delivery_payload, pwd, \
     tool_scope, created_at, persona_id, model_id, reasoning_effort, archived_at, plain_after";

/// 紧跟 SESSION_COLS 追加的列从这个序号开始。
pub(crate) const SESSION_NEXT: usize = 17;

pub(crate) const RUN_COLS: &str = "id, session_id, state, created_at, finished_at";

pub(crate) const PERSONA_COLS: &str = "id, name, prompt, builtin, deleted_at IS NOT NULL";

impl rusqlite::ToSql for ReasoningEffort {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        let name = match self {
            Self::None => "none",
            Self::Minimal => "minimal",
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::Xhigh => "xhigh",
            Self::Max => "max",
        };
        Ok(name.into())
    }
}

impl rusqlite::ToSql for ModelCallPurpose {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl ModelCallPurpose {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Conversation => "conversation",
            Self::Compaction => "compaction",
            Self::Title => "title",
        }
    }
}

impl StopReason {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::EndTurn => "end_turn",
            Self::ToolUse => "tool_use",
            Self::MaxTokens => "max_tokens",
            Self::ContentFilter => "content_filter",
            Self::Stopped => "stopped",
        }
    }
}

impl rusqlite::ToSql for StopReason {
    fn to_sql(&self) -> rusqlite::Result<rusqlite::types::ToSqlOutput<'_>> {
        Ok(self.as_str().into())
    }
}

impl rusqlite::types::FromSql for StopReason {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(match value.as_str()? {
            "end_turn" => Self::EndTurn,
            "tool_use" => Self::ToolUse,
            "max_tokens" => Self::MaxTokens,
            "content_filter" => Self::ContentFilter,
            "stopped" => Self::Stopped,
            other => {
                return Err(rusqlite::types::FromSqlError::Other(
                    format!("未知 stop: {other}").into(),
                ))
            }
        })
    }
}

impl rusqlite::types::FromSql for ModelCallPurpose {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        Ok(match value.as_str()? {
            "conversation" => Self::Conversation,
            "compaction" => Self::Compaction,
            "title" => Self::Title,
            other => {
                return Err(rusqlite::types::FromSqlError::Other(
                    format!("未知 purpose: {other}").into(),
                ))
            }
        })
    }
}

impl rusqlite::types::FromSql for ReasoningEffort {
    fn column_result(value: rusqlite::types::ValueRef<'_>) -> rusqlite::types::FromSqlResult<Self> {
        serde_json::from_value(Value::String(value.as_str()?.to_owned()))
            .map_err(|error| rusqlite::types::FromSqlError::Other(Box::new(error)))
    }
}

/// run 输入的 kind 列表，与 `messages::is_input` 一致。
macro_rules! input_kinds {
    () => {
        "('UserInput', 'Completion', 'Command', 'JobNotice', 'Trigger')"
    };
}
pub(crate) use input_kinds;

/// 未认领输入谓词，`m` 为 core_messages 别名。
pub(crate) const UNCLAIMED: &str = concat!("m.run_id IS NULL AND m.kind IN ", input_kinds!());

/// 已处置为待命的输入谓词，`m` 为 core_messages 别名。
pub(crate) const HELD_INPUT: &str =
    "EXISTS (SELECT 1 FROM core_input_holds h WHERE h.message_id = m.id)";

/// 调用事实不重复写入 Reply payload。
pub(crate) fn payload(body: &MessageBody) -> Result<String, StoreError> {
    let mut value = serde_json::to_value(body)?;
    if matches!(body, MessageBody::Reply { .. }) {
        let fields = value.as_object_mut().expect("MessageBody 序列化为对象");
        fields.remove("model");
        fields.remove("stats");
    }
    Ok(value.to_string())
}

pub(crate) struct SessionKindCols<'a> {
    pub kind: &'static str,
    pub channel: Option<&'a str>,
    pub chat: Option<&'a str>,
    pub trigger_module: Option<&'a str>,
    pub trigger_ref: Option<&'a str>,
}

pub(crate) fn session_kind_cols(kind: &SessionKind) -> SessionKindCols<'_> {
    let none = SessionKindCols {
        kind: "",
        channel: None,
        chat: None,
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
        SessionKind::Task => SessionKindCols {
            kind: "task",
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

pub(crate) fn corrupt(idx: usize, msg: String) -> rusqlite::Error {
    rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, msg.into())
}

pub(crate) fn json_col<T: DeserializeOwned>(row: &Row<'_>, idx: usize) -> rusqlite::Result<T> {
    let text: String = row.get(idx)?;
    serde_json::from_str(&text)
        .map_err(|e| rusqlite::Error::FromSqlConversionFailure(idx, Type::Text, Box::new(e)))
}

/// 列序同 `MESSAGE_COLS`。
pub(crate) fn message(row: &Row<'_>) -> rusqlite::Result<Message> {
    let mut payload: Value = json_col(row, 2)?;
    let is_reply = payload.get("kind").and_then(Value::as_str) == Some("Reply");
    if is_reply {
        let fields = payload
            .as_object_mut()
            .ok_or_else(|| corrupt(2, "payload 不是对象".to_owned()))?;
        fields.insert("model".to_owned(), Value::String(row.get(5)?));
        let stats = mic_message::ReplyStats {
            model_ms: row.get(6)?,
            first_output_ms: row.get(7)?,
            usage: row
                .get::<_, Option<i64>>(8)?
                .map(|input| -> rusqlite::Result<_> {
                    Ok(mic_message::ReplyUsage {
                        input_tokens: input as u64,
                        output_tokens: row.get::<_, i64>(9)? as u64,
                        cache_read_tokens: row.get::<_, Option<i64>>(10)?.map(|n| n as u64),
                    })
                })
                .transpose()?,
        };
        fields.insert(
            "stats".to_owned(),
            serde_json::to_value(stats).expect("ReplyStats 序列化为对象"),
        );
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

/// 从 `idx` 起连续两列 channel/payload；同为 NULL 表示无目标。
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
        payload: row.get(idx + 1)?,
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
        "task" => SessionKind::Task,
        "triggered" => SessionKind::Triggered {
            module: row.get(4)?,
            ref_id: row.get(5)?,
        },
        other => return Err(corrupt(1, format!("未知 session kind: {other}"))),
    };
    Ok(Session {
        id: SessionId(row.get(0)?),
        kind,
        parent_session_id: row.get::<_, Option<i64>>(6)?.map(SessionId),
        delivery_target: delivery_target(row, 7)?,
        pwd: row.get(9)?,
        tool_scope: json_col(row, 10)?,
        created_at: row.get(11)?,
        persona_id: PersonaId(row.get(12)?),
        model_id: row.get::<_, Option<i64>>(13)?.map(ModelId),
        reasoning_effort: row.get(14)?,
        archived_at: row.get(15)?,
        plain_after: row.get::<_, Option<i64>>(16)?.map(MessageId),
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

pub(crate) fn run_state(row: &Row<'_>, idx: usize) -> rusqlite::Result<RunState> {
    let state: String = row.get(idx)?;
    Ok(match state.as_str() {
        "executing" => RunState::Executing,
        "completed" => RunState::Completed,
        "provider_failed" => RunState::ProviderFailed,
        "max_turns" => RunState::MaxTurns,
        "interrupted" => RunState::Interrupted,
        "stopped" => RunState::Stopped,
        "timed_out" => RunState::TimedOut,
        other => return Err(corrupt(idx, format!("未知 run state: {other}"))),
    })
}

/// 列序同 `RUN_COLS`。
pub(crate) fn run(row: &Row<'_>) -> rusqlite::Result<Run> {
    let state = run_state(row, 2)?;
    Ok(Run {
        id: RunId(row.get(0)?),
        session_id: SessionId(row.get(1)?),
        state,
        created_at: row.get(3)?,
        finished_at: row.get(4)?,
    })
}
