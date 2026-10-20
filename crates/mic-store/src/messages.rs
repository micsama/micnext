use std::num::NonZeroU32;

use mic_message::{Message, MessageBody, MessageId, SessionId};
use rusqlite::{params, Connection, OptionalExtension};

use crate::row::{self, MESSAGE_COLS, MESSAGE_FROM};
use crate::store::Store;
use crate::{
    MessagePage, ModelCallId, ModelCallOutcome, NewModelCall, PendingDelivery, RunId, StoreError,
};

impl Store {
    /// 除 `Reply` 外的所有种类；`Reply` 只经 `record_model_call` 产生。
    /// `UserInput`/`Completion` 入站时未认领，`run` 必为 `None`。
    pub async fn append(
        &self,
        session_id: SessionId,
        run: Option<RunId>,
        body: MessageBody,
        at: i64,
    ) -> Result<Message, StoreError> {
        assert!(
            !matches!(body, MessageBody::Reply { .. }),
            "Reply 只能经 record_model_call 写入"
        );
        assert!(
            run.is_none() || !is_input(&body),
            "输入入站时未认领，run 必为 None"
        );
        self.call(move |conn| {
            if matches!(
                body,
                MessageBody::UserInput { .. } | MessageBody::Command { .. }
            ) {
                crate::sessions::ensure_active(conn, session_id)?;
            }
            Ok(Message {
                id: insert_message(conn, session_id, run, &body, at)?,
                session_id,
                body,
                created_at: at,
                delivered_at: None,
            })
        })
        .await
    }

    /// 同一事务写调用行；`Replied` 且 `blocks` 非空时再写指向它的 `Reply` 并返回。
    pub async fn record_model_call(
        &self,
        call: NewModelCall,
    ) -> Result<(ModelCallId, Option<Message>), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let call_id = crate::context::insert_call(&tx, &call)?;
            let reply = match call.outcome {
                ModelCallOutcome::Replied { blocks, .. } if !blocks.is_empty() => {
                    let body = MessageBody::Reply {
                        stats: mic_message::ReplyStats {
                            model_ms: call.finished_at - call.started_at,
                            usage: call.usage.map(|u| mic_message::ReplyUsage {
                                input_tokens: u.input_tokens,
                                output_tokens: u.output_tokens,
                                cache_read_tokens: u.cache_read_tokens,
                            }),
                        },
                        model: call.model,
                        blocks,
                    };
                    tx.execute(
                        "INSERT INTO core_messages
                           (session_id, run_id, model_call_id, payload, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            call.session_id.0,
                            call.run_id.map(|r| r.0),
                            call_id.0,
                            row::payload(&body)?,
                            call.context_at,
                        ],
                    )?;
                    Some(Message {
                        id: MessageId(tx.last_insert_rowid()),
                        session_id: call.session_id,
                        body,
                        created_at: call.context_at,
                        delivered_at: None,
                    })
                }
                _ => None,
            };
            tx.commit()?;
            Ok((call_id, reply))
        })
        .await
    }

    pub async fn run_messages(&self, run: RunId) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                     WHERE m.run_id = ?1 ORDER BY m.id"
                ))?
                .query_map([run.0], row::message)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// 按 id 取消息，id 升序；不存在的 id 跳过。
    pub async fn messages_by_ids(&self, ids: Vec<MessageId>) -> Result<Vec<Message>, StoreError> {
        self.call(move |conn| read_messages(conn, &ids)).await
    }

    /// 稳定回放（含 `Boundary`，不含 `SideOutput`）；游标只能是消息 id，实时增量不作游标。
    pub async fn messages_after(
        &self,
        session_id: SessionId,
        after: Option<MessageId>,
    ) -> Result<Vec<Message>, StoreError> {
        let after = after.map_or(0, |id| id.0);
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                     WHERE m.session_id = ?1 AND m.id > ?2 AND m.kind <> 'SideOutput' ORDER BY m.id"
                ))?
                .query_map(params![session_id.0, after], row::message)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// `before` 之前的一页（不含 `before`；`None` = 到最新）：最近 `rounds` 轮，至多 `cap` 条；不含 `SideOutput`。
    pub async fn messages_page(
        &self,
        session_id: SessionId,
        before: Option<MessageId>,
        rounds: NonZeroU32,
        cap: NonZeroU32,
    ) -> Result<MessagePage, StoreError> {
        let before = before.map_or(i64::MAX, |id| id.0);
        self.call(move |conn| {
            let newest_first = conn
                .prepare(&format!(
                    "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                     WHERE m.session_id = ?1 AND m.id < ?2 AND m.kind <> 'SideOutput' ORDER BY m.id DESC LIMIT ?3"
                ))?
                .query_map(
                    params![session_id.0, before, i64::from(cap.get()) + 1],
                    row::message,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(cut_page(newest_first, rounds, cap))
        })
        .await
    }

    // ---- 投递 ----

    pub async fn pending_deliveries(
        &self,
        channel: &str,
    ) -> Result<Vec<PendingDelivery>, StoreError> {
        let channel = channel.to_owned();
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {MESSAGE_COLS},
                            s.delivery_channel, s.delivery_payload
                     FROM {MESSAGE_FROM} JOIN core_sessions s ON s.id = m.session_id
                     WHERE s.delivery_channel = ?1
                       AND m.delivered_at IS NULL
                       AND m.kind IN ('Reply', 'Notification')
                     ORDER BY m.id"
                ))?
                .query_map([channel], |r| {
                    let target = row::delivery_target(r, 10)?.ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            10,
                            "delivery_channel".to_owned(),
                            rusqlite::types::Type::Null,
                        )
                    })?;
                    Ok(PendingDelivery {
                        message: row::message(r)?,
                        target,
                    })
                })?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    pub async fn mark_delivered(&self, id: MessageId, at: i64) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE core_messages SET delivered_at = ?2 WHERE id = ?1",
                params![id.0, at],
            )?;
            Ok(())
        })
        .await
    }
}

pub(crate) fn is_input(body: &MessageBody) -> bool {
    matches!(
        body,
        MessageBody::UserInput { .. }
            | MessageBody::Completion { .. }
            | MessageBody::Command { .. }
            | MessageBody::JobNotice { .. }
            | MessageBody::Trigger { .. }
    )
}

pub(crate) fn insert_message(
    conn: &Connection,
    session_id: SessionId,
    run: Option<RunId>,
    body: &MessageBody,
    at: i64,
) -> Result<MessageId, StoreError> {
    conn.execute(
        "INSERT INTO core_messages (session_id, run_id, payload, created_at)
         VALUES (?1, ?2, ?3, ?4)",
        params![session_id.0, run.map(|r| r.0), row::payload(body)?, at],
    )?;
    Ok(MessageId(conn.last_insert_rowid()))
}

/// `newest_first` 为倒序的至多 `cap + 1` 条：截到第 `rounds` 条 `UserInput`/`Trigger` 或第 `cap` 条为止。
pub(crate) fn cut_page(
    mut newest_first: Vec<Message>,
    rounds: NonZeroU32,
    cap: NonZeroU32,
) -> MessagePage {
    let cap = cap.get() as usize;
    let mut seen = 0;
    let len = newest_first
        .iter()
        .take(cap)
        .position(|m| {
            seen += u32::from(matches!(
                m.body,
                MessageBody::UserInput { .. } | MessageBody::Trigger { .. }
            ));
            seen == rounds.get()
        })
        .map_or(newest_first.len().min(cap), |i| i + 1);
    let has_earlier = newest_first.len() > len;
    newest_first.truncate(len);
    newest_first.reverse();
    MessagePage {
        messages: newest_first,
        has_earlier,
    }
}

/// 按 id 读消息，缺失的略过，按 id 排序。
pub(crate) fn read_messages(
    conn: &Connection,
    ids: &[MessageId],
) -> Result<Vec<Message>, StoreError> {
    let mut stmt = conn.prepare(&format!(
        "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM} WHERE m.id = ?1"
    ))?;
    let mut out = Vec::with_capacity(ids.len());
    for id in ids {
        out.extend(stmt.query_row([id.0], row::message).optional()?);
    }
    out.sort_by_key(|m| m.id);
    Ok(out)
}

#[cfg(test)]
mod tests;
