use std::num::NonZeroU32;
use std::path::Path;
use std::sync::{Arc, Mutex};

use mic_message::{ContextBoundary, Message, MessageBody, MessageId, PersonId, SessionId};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::limits::PREVIEW_CHARS;
use crate::row::{self, MESSAGE_COLS, MESSAGE_FROM, RUN_COLS, SESSION_COLS, UNCLAIMED};
use crate::{
    ContextWindow, Identity, Migration, ModelCallId, ModelCallOutcome, NewModelCall, NewSession,
    PendingDelivery, Person, Run, RunId, RunState, Session, SessionCursor, SessionPage,
    SessionSummary, StoreError,
};

#[derive(Clone)]
pub struct Store {
    conn: Arc<Mutex<Connection>>,
}

impl Store {
    pub async fn open(path: &Path, modules: &[Migration]) -> Result<Self, StoreError> {
        let path = path.to_owned();
        Self::init(move || Connection::open(path), modules).await
    }

    pub async fn open_in_memory(modules: &[Migration]) -> Result<Self, StoreError> {
        Self::init(Connection::open_in_memory, modules).await
    }

    async fn init<F>(connect: F, modules: &[Migration]) -> Result<Self, StoreError>
    where
        F: FnOnce() -> rusqlite::Result<Connection> + Send + 'static,
    {
        let modules = modules.to_vec();
        let conn = tokio::task::spawn_blocking(move || {
            let mut conn = connect()?;
            crate::schema::init(&mut conn, &modules)?;
            Ok::<_, StoreError>(conn)
        })
        .await
        .expect("store 阻塞任务 panic")?;
        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
        })
    }

    async fn call<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, StoreError> + Send + 'static,
    {
        let conn = Arc::clone(&self.conn);
        tokio::task::spawn_blocking(move || {
            let mut conn = conn.lock().expect("store 锁中毒");
            f(&mut conn)
        })
        .await
        .expect("store 阻塞任务 panic")
    }

    // ---- Person ----

    pub async fn ensure_person(&self, name: &str, now: i64) -> Result<PersonId, StoreError> {
        let name = name.to_owned();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let id = ensure_person(&tx, &name, now)?;
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    /// 返回改绑前的 person（原先未绑定则 None）。
    pub async fn bind_identity(
        &self,
        identity: Identity,
        person: PersonId,
        now: i64,
    ) -> Result<Option<PersonId>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let old = identity_person(&tx, &identity)?;
            // NOTE: 新建绑定时 display_name 留空，待该身份下次入站经 resolve_identity 刷新。
            tx.execute(
                "INSERT INTO core_person_identities
                   (channel, external_id, person_id, display_name, created_at)
                 VALUES (?1, ?2, ?3, '', ?4)
                 ON CONFLICT(channel, external_id) DO UPDATE SET person_id = excluded.person_id",
                params![identity.channel, identity.external_id, person.0, now],
            )?;
            tx.commit()?;
            Ok(old)
        })
        .await
    }

    /// 原子 get-or-create；已存在时刷新 display_name。
    pub async fn resolve_identity(
        &self,
        identity: Identity,
        display_name: &str,
        now: i64,
    ) -> Result<PersonId, StoreError> {
        let display_name = display_name.to_owned();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let person = match identity_person(&tx, &identity)? {
                Some(person) => {
                    tx.execute(
                        "UPDATE core_person_identities SET display_name = ?3
                         WHERE channel = ?1 AND external_id = ?2",
                        params![identity.channel, identity.external_id, display_name],
                    )?;
                    person
                }
                None => {
                    let name = format!("{}:{}", identity.channel, identity.external_id);
                    let person = ensure_person(&tx, &name, now)?;
                    tx.execute(
                        "INSERT INTO core_person_identities
                           (channel, external_id, person_id, display_name, created_at)
                         VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            identity.channel,
                            identity.external_id,
                            person.0,
                            display_name,
                            now
                        ],
                    )?;
                    person
                }
            };
            tx.commit()?;
            Ok(person)
        })
        .await
    }

    pub async fn person(&self, id: PersonId) -> Result<Person, StoreError> {
        self.call(move |conn| {
            Ok(conn.query_row(
                "SELECT id, name, created_at FROM core_persons WHERE id = ?1",
                [id.0],
                |r| {
                    Ok(Person {
                        id: PersonId(r.get(0)?),
                        name: r.get(1)?,
                        created_at: r.get(2)?,
                    })
                },
            )?)
        })
        .await
    }

    // ---- Session ----

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
            // NOTE: 预览依赖 ContentPart 的 serde 外部标签形状 {"Text":{"text":..}}。
            let mut items = conn
                .prepare(&format!(
                    "SELECT {SESSION_COLS}, act, preview FROM (
                       SELECT s.*,
                         (SELECT m.created_at FROM core_messages m
                           WHERE m.session_id = s.id ORDER BY m.id DESC LIMIT 1) AS act,
                         (SELECT substr(json_extract(p.value, '$.Text.text'), 1, ?5)
                            FROM core_messages m, json_each(m.payload, '$.parts') p
                           WHERE m.session_id = s.id AND m.kind = 'UserInput'
                             AND json_type(p.value, '$.Text') IS NOT NULL
                           ORDER BY m.id, p.key LIMIT 1) AS preview
                       FROM core_sessions s
                       WHERE s.kind = 'root' AND s.channel = ?1
                         AND EXISTS (SELECT 1 FROM core_messages m
                                      WHERE m.session_id = s.id AND m.kind = 'UserInput')
                     )
                     WHERE ?2 IS NULL OR (act, id) < (?2, ?3)
                     ORDER BY act DESC, id DESC
                     LIMIT ?4"
                ))?
                .query_map(
                    params![
                        channel,
                        before.map(|c| c.last_activity_at),
                        before.map(|c| c.session_id.0),
                        limit as i64 + 1,
                        PREVIEW_CHARS,
                    ],
                    |r| {
                        Ok(SessionSummary {
                            session: row::session(r)?,
                            last_activity_at: r.get(14)?,
                            preview: r.get(15)?,
                        })
                    },
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

    // ---- 写消息 ----

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
            run.is_none()
                || !matches!(
                    body,
                    MessageBody::UserInput { .. } | MessageBody::Completion { .. }
                ),
            "输入入站时未认领，run 必为 None"
        );
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO core_messages (session_id, run_id, payload, created_at)
                 VALUES (?1, ?2, ?3, ?4)",
                params![session_id.0, run.map(|r| r.0), row::payload(&body)?, at],
            )?;
            Ok(Message {
                id: MessageId(conn.last_insert_rowid()),
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
            let (error, usage) = match &call.outcome {
                ModelCallOutcome::Replied { usage, .. } => (None, *usage),
                ModelCallOutcome::Failed { error } => (Some(error.as_str()), None),
            };
            let tokens =
                |f: fn(&crate::Usage) -> Option<u64>| usage.as_ref().and_then(f).map(|n| n as i64);
            tx.execute(
                "INSERT INTO core_model_calls
                   (session_id, run_id, model, error, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    call.session_id.0,
                    call.run_id.map(|r| r.0),
                    call.model,
                    error,
                    tokens(|u| Some(u.input_tokens)),
                    tokens(|u| Some(u.output_tokens)),
                    tokens(|u| u.cache_read_tokens),
                    tokens(|u| u.cache_write_tokens),
                    tokens(|u| u.reasoning_tokens),
                    call.started_at,
                    call.finished_at,
                ],
            )?;
            let call_id = ModelCallId(tx.last_insert_rowid());
            let reply = match call.outcome {
                ModelCallOutcome::Replied { blocks, .. } if !blocks.is_empty() => {
                    let body = MessageBody::Reply {
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
                            call.finished_at,
                        ],
                    )?;
                    Some(Message {
                        id: MessageId(tx.last_insert_rowid()),
                        session_id: call.session_id,
                        body,
                        created_at: call.finished_at,
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

    // ---- 调度 ----

    /// 会话无 `executing` run 时，把未认领输入开头连续同 person 的一段认领为新 run。
    pub async fn claim_next(
        &self,
        session_id: SessionId,
        now: i64,
    ) -> Result<Option<Run>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let executing: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM core_runs
                               WHERE session_id = ?1 AND state = 'executing')",
                [session_id.0],
                |r| r.get(0),
            )?;
            if executing {
                return Ok(None);
            }
            let Some((ids, _)) = unclaimed_head(&tx, session_id.0)? else {
                return Ok(None);
            };
            tx.execute(
                "INSERT INTO core_runs (session_id, state, created_at)
                 VALUES (?1, ?2, ?3)",
                params![session_id.0, row::run_state_col(RunState::Executing), now],
            )?;
            let id = RunId(tx.last_insert_rowid());
            assign_run(&tx, id, &ids)?;
            tx.commit()?;
            Ok(Some(Run {
                id,
                session_id,
                state: RunState::Executing,
                created_at: now,
                finished_at: None,
            }))
        })
        .await
    }

    /// 把未认领输入开头同一 person 的一段并入 `run`，要求该 person 与 run 首条输入相同；
    /// 有并入返回 true。调用方保证 run 处于 `Executing`。
    pub async fn absorb(&self, run: RunId) -> Result<bool, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let (session_id, person): (i64, i64) = tx.query_row(
                "SELECT session_id, person_id FROM core_messages
                 WHERE run_id = ?1 AND kind IN ('UserInput', 'Completion')
                 ORDER BY id LIMIT 1",
                [run.0],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )?;
            let Some((ids, p)) = unclaimed_head(&tx, session_id)? else {
                return Ok(false);
            };
            if p != person {
                return Ok(false);
            }
            assign_run(&tx, run, &ids)?;
            tx.commit()?;
            Ok(true)
        })
        .await
    }

    /// 有未认领输入的会话，按 id 升序。
    pub async fn sessions_with_unclaimed_input(&self) -> Result<Vec<SessionId>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT DISTINCT m.session_id FROM core_messages m
                     WHERE {UNCLAIMED} ORDER BY m.session_id"
                ))?
                .query_map([], |r| Ok(SessionId(r.get(0)?)))?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    pub async fn finish_run(&self, id: RunId, state: RunState, now: i64) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE core_runs SET state = ?2, finished_at = ?3 WHERE id = ?1",
                params![id.0, row::run_state_col(state), now],
            )?;
            Ok(())
        })
        .await
    }

    /// 启动时把遗留 `Executing` 收尾为 `Interrupted`，返回被收尾的 run。
    pub async fn interrupt_stale_runs(&self, now: i64) -> Result<Vec<Run>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut stale = tx
                .prepare(&format!(
                    "UPDATE core_runs SET state = ?1, finished_at = ?2
                     WHERE state = 'executing'
                     RETURNING {RUN_COLS}"
                ))?
                .query_map(
                    params![row::run_state_col(RunState::Interrupted), now],
                    row::run,
                )?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            tx.commit()?;
            stale.sort_by_key(|r| r.id);
            Ok(stale)
        })
        .await
    }

    // ---- 读 ----

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

    pub async fn context_window(&self, session_id: SessionId) -> Result<ContextWindow, StoreError> {
        self.call(move |conn| {
            let boundary = conn
                .query_row(
                    &format!(
                        "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                         WHERE m.session_id = ?1 AND m.kind = 'Boundary'
                         ORDER BY m.id DESC LIMIT 1"
                    ),
                    [session_id.0],
                    row::message,
                )
                .optional()?;
            let (after, summary) = match boundary {
                Some(Message {
                    id,
                    body: MessageBody::Boundary { boundary },
                    ..
                }) => (
                    id.0,
                    match boundary {
                        ContextBoundary::Compaction { summary } => Some(summary),
                        ContextBoundary::UserClear => None,
                    },
                ),
                _ => (0, None),
            };
            let messages = conn
                .prepare(&format!(
                    "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                     WHERE m.session_id = ?1 AND m.id > ?2 AND NOT ({UNCLAIMED})
                     ORDER BY m.id"
                ))?
                .query_map(params![session_id.0, after], row::message)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(ContextWindow { summary, messages })
        })
        .await
    }

    /// 稳定回放（含 `Boundary`）；游标只能是消息 id，实时增量不作游标。
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
                     WHERE m.session_id = ?1 AND m.id > ?2 ORDER BY m.id"
                ))?
                .query_map(params![session_id.0, after], row::message)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
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
                            s.delivery_channel, s.delivery_version, s.delivery_payload
                     FROM {MESSAGE_FROM} JOIN core_sessions s ON s.id = m.session_id
                     WHERE s.delivery_channel = ?1
                       AND m.delivered_at IS NULL
                       AND m.kind IN ('Reply', 'Notification')
                     ORDER BY m.id"
                ))?
                .query_map([channel], |r| {
                    let target = row::delivery_target(r, 6)?.ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            6,
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

    // ---- 模块表 ----

    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<R> + Send + 'static,
    {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let r = f(&tx)?;
            tx.commit()?;
            Ok(r)
        })
        .await
    }
}

fn ensure_person(tx: &Transaction<'_>, name: &str, now: i64) -> Result<PersonId, StoreError> {
    tx.execute(
        "INSERT INTO core_persons (name, created_at) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING",
        params![name, now],
    )?;
    Ok(PersonId(tx.query_row(
        "SELECT id FROM core_persons WHERE name = ?1",
        [name],
        |r| r.get(0),
    )?))
}

fn identity_person(
    tx: &Transaction<'_>,
    identity: &Identity,
) -> Result<Option<PersonId>, StoreError> {
    Ok(tx
        .query_row(
            "SELECT person_id FROM core_person_identities WHERE channel = ?1 AND external_id = ?2",
            params![identity.channel, identity.external_id],
            |r| r.get(0),
        )
        .optional()?
        .map(PersonId))
}

fn insert_session(tx: &Transaction<'_>, new: NewSession) -> Result<Session, StoreError> {
    let kind = row::session_kind_cols(&new.kind);
    let tool_scope = serde_json::to_string(&new.tool_scope)?;
    let target = new.delivery_target.as_ref();
    tx.execute(
        "INSERT INTO core_sessions
           (kind, channel, chat, parent_tool_call_id, trigger_module, trigger_ref,
            parent_session_id, delivery_channel, delivery_version, delivery_payload,
            pwd, tool_scope, created_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)",
        params![
            kind.kind,
            kind.channel,
            kind.chat,
            kind.parent_tool_call_id,
            kind.trigger_module,
            kind.trigger_ref,
            new.parent_session_id.map(|id| id.0),
            target.map(|t| &t.channel),
            target.map(|t| t.version),
            target.map(|t| &t.payload),
            new.pwd,
            tool_scope,
            new.created_at,
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
    })
}

/// 未认领输入按 id 排序后开头连续同 person 的一段 `(ids, person)`。
fn unclaimed_head(
    tx: &Transaction<'_>,
    session_id: i64,
) -> rusqlite::Result<Option<(Vec<i64>, i64)>> {
    let mut stmt = tx.prepare(&format!(
        "SELECT m.id, m.person_id FROM core_messages m
         WHERE m.session_id = ?1 AND {UNCLAIMED}
         ORDER BY m.id"
    ))?;
    let mut rows = stmt.query([session_id])?;
    let mut head: Option<(Vec<i64>, i64)> = None;
    while let Some(r) = rows.next()? {
        let (id, p): (i64, i64) = (r.get(0)?, r.get(1)?);
        match &mut head {
            None => head = Some((vec![id], p)),
            Some((ids, cur)) if *cur == p => ids.push(id),
            Some(_) => break,
        }
    }
    Ok(head)
}

fn assign_run(tx: &Transaction<'_>, run: RunId, ids: &[i64]) -> rusqlite::Result<()> {
    let mut stmt = tx.prepare("UPDATE core_messages SET run_id = ?1 WHERE id = ?2")?;
    for id in ids {
        stmt.execute(params![run.0, id])?;
    }
    Ok(())
}
