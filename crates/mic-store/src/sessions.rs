use std::num::NonZeroU32;

use mic_message::{MessageId, SessionId};
use rusqlite::{params, OptionalExtension, Transaction};

use crate::limits::PREVIEW_CHARS;
use crate::models;
use crate::row::{self, SESSION_COLS};
use crate::store::Store;
use crate::{
    NewSession, PersonaId, Session, SessionCursor, SessionPage, SessionSummary, StoreError, TagId,
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

    /// 会话所在树的 Root（Root 即自身）。
    pub async fn session_root(&self, id: SessionId) -> Result<Session, StoreError> {
        self.call(move |conn| {
            Ok(conn.query_row(
                &format!(
                    "WITH RECURSIVE up(id, parent) AS (
                       SELECT id, parent_session_id FROM core_sessions WHERE id = ?1
                       UNION ALL
                       SELECT s.id, s.parent_session_id FROM core_sessions s JOIN up ON s.id = up.parent
                     )
                     SELECT {SESSION_COLS} FROM core_sessions
                     WHERE id = (SELECT id FROM up WHERE parent IS NULL)"
                ),
                [id.0],
                row::session,
            )?)
        })
        .await
    }

    pub async fn archive_session(&self, session: SessionId, at: i64) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE core_sessions SET archived_at = COALESCE(archived_at, ?2), pinned_at = NULL
                 WHERE id = ?1",
                params![session.0, at],
            )?;
            Ok(())
        })
        .await
    }

    /// 开启时把水位记在当前最新消息上，已开启则保持原水位；关闭清空。
    pub async fn set_session_plain(&self, session: SessionId, on: bool) -> Result<(), StoreError> {
        self.call(move |conn| {
            let sql = if on {
                "UPDATE core_sessions SET plain_after = COALESCE(plain_after,
                     (SELECT COALESCE(MAX(id), 0) FROM core_messages WHERE session_id = ?1))
                 WHERE id = ?1"
            } else {
                "UPDATE core_sessions SET plain_after = NULL WHERE id = ?1"
            };
            conn.execute(sql, [session.0])?;
            Ok(())
        })
        .await
    }

    /// `at` 为空取消置顶；已置顶时保留原时间。
    pub async fn set_session_pinned(
        &self,
        session: SessionId,
        at: Option<i64>,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            ensure_active(conn, session)?;
            conn.execute(
                "UPDATE core_sessions SET pinned_at = CASE WHEN ?2 IS NULL THEN NULL
                   ELSE COALESCE(pinned_at, ?2) END WHERE id = ?1",
                params![session.0, at],
            )?;
            Ok(())
        })
        .await
    }

    /// 已读水位只前移；返回是否前移。`message` 不属于该会话返回 `MessageNotInSession`。
    pub async fn mark_session_seen(
        &self,
        session: SessionId,
        message: MessageId,
    ) -> Result<bool, StoreError> {
        self.call(move |conn| {
            let owned: bool = conn.query_row(
                "SELECT EXISTS(SELECT 1 FROM core_messages WHERE id = ?1 AND session_id = ?2)",
                params![message.0, session.0],
                |r| r.get(0),
            )?;
            if !owned {
                return Err(StoreError::MessageNotInSession);
            }
            Ok(conn.execute(
                "UPDATE core_sessions SET seen_message_id = ?2
                 WHERE id = ?1 AND COALESCE(seen_message_id, 0) < ?2",
                params![session.0, message.0],
            )? > 0)
        })
        .await
    }

    /// 全部置顶的未归档 Root，跨渠道，新置顶在前；`tag` 只留带这个标签的。
    pub async fn list_pinned_sessions(
        &self,
        tag: Option<TagId>,
    ) -> Result<Vec<SessionSummary>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "{} ORDER BY pinned_at DESC, id DESC",
                    summaries(&format!(
                        "s.kind = 'root' AND s.archived_at IS NULL AND s.pinned_at IS NOT NULL AND {}",
                        tagged("?1")
                    ))
                ))?
                .query_map([tag.map(|t| t.0)], row_summary)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
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

    /// 列出 `channel` 下已有用户输入的 Root 会话，按 `(last_activity_at, id)` 降序；`tag` 只留带这个标签的。
    pub async fn list_root_sessions(
        &self,
        channel: &str,
        tag: Option<TagId>,
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
                    summaries(&format!(
                        "s.kind = 'root' AND s.channel = ?1 AND s.archived_at IS NULL
                         AND s.pinned_at IS NULL AND {}
                         AND EXISTS (SELECT 1 FROM core_messages m
                                      WHERE m.session_id = s.id AND m.kind IN ('UserInput', 'Command'))",
                        tagged("?5")
                    ))
                ))?
                .query_map(
                    params![
                        channel,
                        before.map(|c| c.last_activity_at),
                        before.map(|c| c.session_id.0),
                        limit as i64 + 1,
                        tag.map(|t| t.0),
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

pub(crate) fn insert_session(tx: &Transaction<'_>, new: NewSession) -> Result<Session, StoreError> {
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
           (kind, channel, chat, trigger_module, trigger_ref,
            parent_session_id, delivery_channel, delivery_payload,
            pwd, tool_scope, created_at, persona_id, model_id, reasoning_effort)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
        params![
            kind.kind,
            kind.channel,
            kind.chat,
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
        archived_at: None,
        plain_after: None,
    })
}

/// `param` 为空或会话带这个标签。
fn tagged(param: &str) -> String {
    format!("({param} IS NULL OR EXISTS (SELECT 1 FROM core_session_tags st WHERE st.session_id = s.id AND st.tag_id = {param}))")
}

/// `filter` 筛出的会话连同 `act`（最近活跃）与 `preview`，供外层再筛选排序。
fn summaries(filter: &str) -> String {
    // NOTE: 预览依赖 ContentPart 的 serde 外部标签形状 {"Text":{"text":..}}；无文字时以首个附件占位。
    format!(
        "SELECT {SESSION_COLS}, act, preview, title, title_error, pinned_at, unread, running, tag_ids FROM (
           SELECT s.*,
             (SELECT json_group_array(tag_id) FROM (SELECT tag_id FROM core_session_tags
                WHERE session_id = s.id ORDER BY tag_id)) AS tag_ids,
             (s.kind = 'root' AND s.channel = 'web' AND EXISTS (
                SELECT 1 FROM core_messages m
                 WHERE m.session_id = s.id AND m.kind IN ('Reply', 'Notification')
                   AND m.id > COALESCE(s.seen_message_id, 0))) AS unread,
             EXISTS (SELECT 1 FROM core_runs r WHERE r.session_id = s.id AND r.state = 'executing') AS running,
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
        title: r.get(row::SESSION_NEXT + 2)?,
        title_error: r.get(row::SESSION_NEXT + 3)?,
        session: row::session(r)?,
        last_activity_at: r.get(row::SESSION_NEXT)?,
        preview: r.get(row::SESSION_NEXT + 1)?,
        pinned_at: r.get(row::SESSION_NEXT + 4)?,
        unread: r.get(row::SESSION_NEXT + 5)?,
        running: r.get(row::SESSION_NEXT + 6)?,
        tag_ids: serde_json::from_str::<Vec<i64>>(&r.get::<_, String>(row::SESSION_NEXT + 7)?)
            .map_err(|e| {
                rusqlite::Error::FromSqlConversionFailure(
                    row::SESSION_NEXT + 7,
                    rusqlite::types::Type::Text,
                    e.into(),
                )
            })?
            .into_iter()
            .map(TagId)
            .collect(),
    })
}

/// 归档标记只存根会话，子会话在执行边界沿祖先链检查。
pub(crate) fn archived(conn: &rusqlite::Connection, session: SessionId) -> rusqlite::Result<bool> {
    conn.query_row(
        "WITH RECURSIVE up(id, parent, archived_at) AS (
           SELECT id, parent_session_id, archived_at FROM core_sessions WHERE id = ?1
           UNION ALL
           SELECT s.id, s.parent_session_id, s.archived_at FROM core_sessions s JOIN up ON s.id = up.parent
         ) SELECT EXISTS(SELECT 1 FROM up WHERE archived_at IS NOT NULL)",
        [session.0], |r| r.get(0),
    )
}

pub(crate) fn ensure_active(
    conn: &rusqlite::Connection,
    session: SessionId,
) -> Result<(), StoreError> {
    if archived(conn, session)? {
        return Err(StoreError::SessionArchived);
    }
    Ok(())
}

impl Store {
    pub async fn ensure_session_active(&self, session: SessionId) -> Result<(), StoreError> {
        self.call(move |conn| ensure_active(conn, session)).await
    }

    /// 一棵会话树；归档收尾使用，与会话列表的可见性无关。
    pub async fn subtree_sessions(&self, root: SessionId) -> Result<Vec<Session>, StoreError> {
        self.call(move |conn| {
            Ok(conn.prepare(&format!(
                "WITH RECURSIVE tree(id) AS (SELECT ?1 UNION ALL
                 SELECT s.id FROM core_sessions s JOIN tree t ON s.parent_session_id = t.id)
                 SELECT {SESSION_COLS} FROM core_sessions WHERE id IN (SELECT id FROM tree) ORDER BY id"
            ))?.query_map([root.0], row::session)?.collect::<rusqlite::Result<Vec<_>>>()?)
        }).await
    }
}

#[cfg(test)]
mod tests;
