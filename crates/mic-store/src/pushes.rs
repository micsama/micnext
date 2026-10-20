//! 推送：发给 person 的消息与各 IM 投递；每条都进 Web 通知中心。契约：docs/blueprints/scheduled-tasks.md P1。

use mic_message::{Message, MessageBody, PersonId, SessionId};
use rusqlite::{params, Connection};
use serde::Serialize;

use crate::messages::insert_message;
use crate::store::Store;
use crate::{RunId, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct PushId(pub i64);

pub struct NewPush {
    pub person: PersonId,
    pub source_session: SessionId,
    pub source_run: Option<RunId>,
    /// 写入私聊会话的 `Notification.source`。
    pub source_label: String,
    pub text: String,
    pub routes: Vec<PushRoute>,
}

pub enum PushRoute {
    /// 写入该 person 在此 Channel 的私聊会话。
    Session { channel: String, session: SessionId },
    /// 发不出去，只记原因。
    Unavailable { channel: String, reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PushView {
    pub id: PushId,
    pub source_session_id: SessionId,
    pub source_session_title: Option<String>,
    /// 来源是定时任务会话时的任务名；任务删除后为空。
    pub source_schedule: Option<String>,
    pub text: String,
    pub created_at: i64,
    pub read_at: Option<i64>,
    pub deliveries: Vec<PushDelivery>,
}

#[derive(Debug, Clone, Serialize)]
pub struct PushDelivery {
    pub channel: String,
    pub state: PushDeliveryState,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PushDeliveryState {
    Pending,
    Delivered { at: i64 },
    Failed { reason: String },
}

#[derive(Debug, Clone, Serialize)]
pub struct PushPage {
    pub pushes: Vec<PushView>,
    pub has_more: bool,
    pub unread: u32,
}

impl Store {
    /// 用户选的默认推送 Channel；空 = 未设，由调用方按连接情况取默认。
    pub async fn push_channels(&self) -> Result<Vec<String>, StoreError> {
        self.call(|conn| Ok(read_push_channels(conn)?)).await
    }

    /// 名字是否为已知 Channel 由调用方保证。
    pub async fn set_push_channels(&self, channels: Vec<String>) -> Result<(), StoreError> {
        let json = serde_json::to_string(&channels).expect("字符串数组可序列化");
        self.call(move |conn| {
            conn.execute("UPDATE core_settings SET push_channels = ?1", [json])?;
            Ok(())
        })
        .await
    }

    /// 一个事务写推送、各投递与私聊会话里的 `Notification`；返回写入的通知，供调用方按序发布。
    pub async fn create_push(
        &self,
        p: NewPush,
        now: i64,
    ) -> Result<(PushId, Vec<Message>), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO core_pushes (person_id, source_session_id, source_run_id, text, created_at)
                 VALUES (?1, ?2, ?3, ?4, ?5)",
                params![p.person.0, p.source_session.0, p.source_run.map(|r| r.0), p.text, now],
            )?;
            let id = PushId(tx.last_insert_rowid());
            let mut notices = Vec::new();
            for route in p.routes {
                let (channel, message, failure) = match route {
                    PushRoute::Session { channel, session } => {
                        let body = MessageBody::Notification {
                            source: p.source_label.clone(),
                            text: p.text.clone(),
                        };
                        let message = insert_message(&tx, session, None, &body, now)?;
                        notices.push(Message {
                            id: message,
                            session_id: session,
                            body,
                            created_at: now,
                            delivered_at: None,
                        });
                        (channel, Some(message), None)
                    }
                    PushRoute::Unavailable { channel, reason } => (channel, None, Some(reason)),
                };
                tx.execute(
                    "INSERT INTO core_push_deliveries (push_id, channel, message_id, failure)
                     VALUES (?1, ?2, ?3, ?4)",
                    params![id.0, channel, message.map(|m| m.0), failure],
                )?;
            }
            tx.commit()?;
            Ok((id, notices))
        })
        .await
    }

    /// 新的在前；`before` 为上一页最后一条的 id。
    pub async fn pushes(
        &self,
        person: PersonId,
        before: Option<PushId>,
        limit: u32,
    ) -> Result<PushPage, StoreError> {
        self.call(move |conn| {
            let mut stmt = conn.prepare(
                "SELECT p.id, p.source_session_id, s.title, sc.name, p.text, p.created_at, p.read_at
                 FROM core_pushes p JOIN core_sessions s ON s.id = p.source_session_id
                 LEFT JOIN core_schedules sc ON sc.session_id = p.source_session_id
                 WHERE p.person_id = ?1 AND p.id < ?2
                 ORDER BY p.id DESC LIMIT ?3",
            )?;
            let mut pushes = stmt
                .query_map(
                    params![
                        person.0,
                        before.map_or(i64::MAX, |b| b.0),
                        i64::from(limit) + 1
                    ],
                    |r| {
                        Ok(PushView {
                            id: PushId(r.get(0)?),
                            source_session_id: SessionId(r.get(1)?),
                            source_session_title: r.get(2)?,
                            source_schedule: r.get(3)?,
                            text: r.get(4)?,
                            created_at: r.get(5)?,
                            read_at: r.get(6)?,
                            deliveries: Vec::new(),
                        })
                    },
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let has_more = pushes.len() > limit as usize;
            pushes.truncate(limit as usize);
            for push in &mut pushes {
                push.deliveries = deliveries(conn, push.id)?;
            }
            let unread = conn.query_row(
                "SELECT COUNT(*) FROM core_pushes WHERE person_id = ?1 AND read_at IS NULL",
                [person.0],
                |r| r.get(0),
            )?;
            Ok(PushPage {
                pushes,
                has_more,
                unread,
            })
        })
        .await
    }

    pub async fn mark_pushes_read(
        &self,
        person: PersonId,
        ids: Vec<PushId>,
        now: i64,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            {
                let mut stmt = tx.prepare(
                    "UPDATE core_pushes SET read_at = ?3
                     WHERE id = ?1 AND person_id = ?2 AND read_at IS NULL",
                )?;
                for id in ids {
                    stmt.execute(params![id.0, person.0, now])?;
                }
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

fn deliveries(conn: &Connection, push: PushId) -> rusqlite::Result<Vec<PushDelivery>> {
    let mut stmt = conn.prepare_cached(
        "SELECT d.channel, d.message_id, d.failure, m.delivered_at
         FROM core_push_deliveries d LEFT JOIN core_messages m ON m.id = d.message_id
         WHERE d.push_id = ?1 ORDER BY d.channel",
    )?;
    let rows = stmt.query_map([push.0], |r| {
        let channel: String = r.get(0)?;
        let message: Option<i64> = r.get(1)?;
        let failure: Option<String> = r.get(2)?;
        let delivered: Option<i64> = r.get(3)?;
        let state = match (failure, message, delivered) {
            (Some(reason), _, _) => PushDeliveryState::Failed { reason },
            (None, Some(_), Some(at)) => PushDeliveryState::Delivered { at },
            (None, Some(_), None) => PushDeliveryState::Pending,
            (None, None, _) => unreachable!("表约束：投递行有消息或失败原因"),
        };
        Ok(PushDelivery { channel, state })
    })?;
    rows.collect()
}

pub(crate) fn read_push_channels(conn: &Connection) -> rusqlite::Result<Vec<String>> {
    let json: String =
        conn.query_row("SELECT push_channels FROM core_settings", [], |r| r.get(0))?;
    serde_json::from_str(&json).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
    })
}

#[cfg(test)]
mod tests;
