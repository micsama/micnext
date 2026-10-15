use std::num::NonZeroU32;

use mic_message::SessionId;
use rusqlite::{params, OptionalExtension, Transaction};

use crate::limits::PREVIEW_CHARS;
use crate::models;
use crate::row::{self, SESSION_COLS};
use crate::store::Store;
use crate::{
    NewSession, PersonaId, Session, SessionCursor, SessionPage, SessionSummary, StoreError,
};

impl Store {
    /// Root 专用原子 get-or-create；已存在则返回既有行，init 被忽略。
    pub async fn resolve_root_session(
        &self,
        channel: &str,
        chat: &str,
        init: NewSession,
    ) -> Result<Session, StoreError> {
        let (channel, chat) = (channel.to_owned(), chat.to_owned());
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let existing = tx
                .query_row(
                    &format!(
                        "SELECT {SESSION_COLS} FROM core_sessions
                         WHERE kind = 'root' AND channel = ?1 AND chat = ?2"
                    ),
                    params![channel, chat],
                    row::session,
                )
                .optional()?;
            let session = match existing {
                Some(session) => session,
                None => insert_session(&tx, init)?,
            };
            tx.commit()?;
            Ok(session)
        })
        .await
    }

    /// Task/Triggered 用。
    pub async fn create_session(&self, new: NewSession) -> Result<Session, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let session = insert_session(&tx, new)?;
            tx.commit()?;
            Ok(session)
        })
        .await
    }

    /// id 可能来自外部（Gateway 路径参数），不存在返回 `None`。
    pub async fn session(&self, id: SessionId) -> Result<Option<Session>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {SESSION_COLS} FROM core_sessions WHERE id = ?1"),
                    [id.0],
                    row::session,
                )
                .optional()?)
        })
        .await
    }

    pub async fn set_pwd(&self, id: SessionId, pwd: &str) -> Result<(), StoreError> {
        let pwd = pwd.to_owned();
        self.call(move |conn| {
            conn.execute(
                "UPDATE core_sessions SET pwd = ?2 WHERE id = ?1",
                params![id.0, pwd],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn archive_session(&self, session: SessionId, at: i64) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE core_sessions SET archived_at = COALESCE(archived_at, ?2) WHERE id = ?1",
                params![session.0, at],
            )?;
            Ok(())
        })
        .await
    }

    /// 单个会话的摘要；没有消息时最近活跃取创建时间。
    pub async fn session_summary(
        &self,
        id: SessionId,
    ) -> Result<Option<SessionSummary>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(&summaries("s.id = ?1"), [id.0], row_summary)
                .optional()?)
        })
        .await
    }

    /// 列出 `channel` 下已有用户输入的 Root 会话，按 `(last_activity_at, id)` 降序。
    pub async fn list_root_sessions(
        &self,
        channel: &str,
        before: Option<SessionCursor>,
        limit: NonZeroU32,
    ) -> Result<SessionPage, StoreError> {
        let channel = channel.to_owned();
        let limit = limit.get() as usize;
        self.call(move |conn| {
            let mut items = conn
                .prepare(&format!(
                    "{} WHERE ?2 IS NULL OR (act, id) < (?2, ?3)
                     ORDER BY act DESC, id DESC
                     LIMIT ?4",
                    summaries(
                        "s.kind = 'root' AND s.channel = ?1 AND s.archived_at IS NULL
                         AND EXISTS (SELECT 1 FROM core_messages m
                                      WHERE m.session_id = s.id AND m.kind IN ('UserInput', 'Command'))"
                    )
                ))?
                .query_map(
                    params![
                        channel,
                        before.map(|c| c.last_activity_at),
                        before.map(|c| c.session_id.0),
                        limit as i64 + 1,
                    ],
                    row_summary,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let next = (items.len() > limit).then(|| {
                items.truncate(limit);
                let last = &items[limit - 1];
                SessionCursor {
                    last_activity_at: last.last_activity_at,
                    session_id: last.session.id,
                }
            });
            Ok(SessionPage { items, next })
        })
        .await
    }
}

fn insert_session(tx: &Transaction<'_>, new: NewSession) -> Result<Session, StoreError> {
    let kind = row::session_kind_cols(&new.kind);
    let tool_scope = serde_json::to_string(&new.tool_scope)?;
    let target = new.delivery_target.as_ref();
    let persona_id = match new.persona {
        Some(id) => id,
        None => PersonaId(tx.query_row(
            "SELECT default_persona_id FROM core_settings",
            [],
            |r| r.get(0),
        )?),
    };
    let model_id = match new.model {
        Some(id) => Some(id),
        None => models::read_default(tx)?,
    };
    tx.execute(
        "INSERT INTO core_sessions
           (kind, channel, chat, parent_tool_call_id, trigger_module, trigger_ref,
            parent_session_id, delivery_channel, delivery_payload,
            pwd, tool_scope, created_at, persona_id, model_id, reasoning_effort)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15)",
        params![
            kind.kind,
            kind.channel,
            kind.chat,
            kind.parent_tool_call_id,
            kind.trigger_module,
            kind.trigger_ref,
            new.parent_session_id.map(|id| id.0),
            target.map(|t| &t.channel),
            target.map(|t| &t.payload),
            new.pwd,
            tool_scope,
            new.created_at,
            persona_id.0,
            model_id.map(|id| id.0),
            new.reasoning_effort,
        ],
    )?;
    Ok(Session {
        id: SessionId(tx.last_insert_rowid()),
        kind: new.kind,
        parent_session_id: new.parent_session_id,
        delivery_target: new.delivery_target,
        pwd: new.pwd,
        tool_scope: new.tool_scope,
        created_at: new.created_at,
        persona_id,
        model_id,
        reasoning_effort: new.reasoning_effort,
    })
}

/// `filter` 筛出的会话连同 `act`（最近活跃）与 `preview`，供外层再筛选排序。
fn summaries(filter: &str) -> String {
    // NOTE: 预览依赖 ContentPart 的 serde 外部标签形状 {"Text":{"text":..}}；无文字时以首个附件占位。
    format!(
        "SELECT {SESSION_COLS}, act, preview, title, title_error FROM (
           SELECT s.*,
             COALESCE((SELECT m.created_at FROM core_messages m
                        WHERE m.session_id = s.id ORDER BY m.id DESC LIMIT 1),
                      s.created_at) AS act,
             COALESCE(
               (SELECT substr(json_extract(p.value, '$.Text.text'), 1, {PREVIEW_CHARS})
                  FROM core_messages m, json_each(m.payload, '$.parts') p
                 WHERE m.session_id = s.id AND m.kind = 'UserInput'
                   AND json_type(p.value, '$.Text') IS NOT NULL
                 ORDER BY m.id, p.key LIMIT 1),
               (SELECT CASE WHEN json_type(p.value, '$.Image') IS NOT NULL THEN '[图片]' ELSE '[文件]' END
                  FROM core_messages m, json_each(m.payload, '$.parts') p
                 WHERE m.session_id = s.id AND m.kind = 'UserInput'
                 ORDER BY m.id, p.key LIMIT 1)) AS preview
           FROM core_sessions s
           WHERE {filter}
         )"
    )
}

fn row_summary(r: &rusqlite::Row<'_>) -> rusqlite::Result<SessionSummary> {
    Ok(SessionSummary {
        title: r.get(18)?,
        title_error: r.get(19)?,
        session: row::session(r)?,
        last_activity_at: r.get(16)?,
        preview: r.get(17)?,
    })
}
