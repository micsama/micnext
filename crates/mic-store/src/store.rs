use std::path::Path;
use std::sync::{Arc, Mutex};

use mic_message::{
    MessageAuthor, MessageContent, PersonId, SessionEntry, SessionEntryId, SessionId,
};
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::row::{self, ENTRY_COLS, QUERY_COLS, SESSION_COLS};
use crate::{
    BoundaryInput, CompletionInput, ContextWindow, FailureReason, Identity, Migration,
    ModelCallInput, ModelCallPurpose, NewSession, OutputInput, PendingDelivery, Person, Query,
    QueryId, QueryState, Session, StoreError, Usage, UserInput,
};

/// 可 claim 判别式，`e` 为 session_entries 别名。
const CLAIMABLE: &str = "e.entry_kind = 'message' AND (
       (e.author_kind = 'user' AND e.content_kind IN ('text', 'attachment'))
    OR e.content_kind = 'completion'
  )";

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
                "INSERT INTO person_identities
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
                        "UPDATE person_identities SET display_name = ?3
                         WHERE channel = ?1 AND external_id = ?2",
                        params![identity.channel, identity.external_id, display_name],
                    )?;
                    person
                }
                None => {
                    let name = format!("{}:{}", identity.channel, identity.external_id);
                    let person = ensure_person(&tx, &name, now)?;
                    tx.execute(
                        "INSERT INTO person_identities
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
                "SELECT id, name, created_at FROM persons WHERE id = ?1",
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
                        "SELECT {SESSION_COLS} FROM sessions
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
                    &format!("SELECT {SESSION_COLS} FROM sessions WHERE id = ?1"),
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
                "UPDATE sessions SET pwd = ?2 WHERE id = ?1",
                params![id.0, pwd],
            )?;
            Ok(())
        })
        .await
    }

    // ---- 写 entry ----

    pub async fn append_user_input(&self, input: UserInput) -> Result<SessionEntryId, StoreError> {
        self.call(move |conn| {
            insert_message(
                conn,
                input.session_id,
                &MessageAuthor::User {
                    id: input.person_id,
                },
                &input.content,
                Some(input.person_id),
                input.created_at,
            )
        })
        .await
    }

    pub async fn append_completion(
        &self,
        input: CompletionInput,
    ) -> Result<SessionEntryId, StoreError> {
        self.call(move |conn| {
            insert_message(
                conn,
                input.session_id,
                &MessageAuthor::Tool {
                    name: input.tool_name,
                },
                &MessageContent::Completion {
                    exec_id: input.exec_id,
                    outcome: input.outcome,
                },
                Some(input.person_id),
                input.created_at,
            )
        })
        .await
    }

    /// NOTE: author 不为 `User`——用户输入走 `append_user_input`。
    pub async fn append_output(&self, input: OutputInput) -> Result<SessionEntryId, StoreError> {
        self.call(move |conn| {
            insert_message(
                conn,
                input.session_id,
                &input.author,
                &input.content,
                None,
                input.created_at,
            )
        })
        .await
    }

    pub async fn append_boundary(
        &self,
        input: BoundaryInput,
    ) -> Result<SessionEntryId, StoreError> {
        self.call(move |conn| {
            let payload = serde_json::to_string(&input.boundary)?;
            conn.execute(
                "INSERT INTO session_entries (session_id, entry_kind, payload, created_at)
                 VALUES (?1, 'boundary', ?2, ?3)",
                params![input.session_id.0, payload, input.created_at],
            )?;
            Ok(SessionEntryId(conn.last_insert_rowid()))
        })
        .await
    }

    // ---- 调度 ----

    pub async fn claim_next(
        &self,
        session_id: SessionId,
        now: i64,
    ) -> Result<Option<Query>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let executing: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM queries WHERE session_id = ?1 AND state = 'executing')",
                [session_id.0],
                |r| r.get(0),
            )?;
            if executing {
                return Ok(None);
            }
            let lower: i64 = tx.query_row(
                "SELECT COALESCE(
                   (SELECT claimed_end_id FROM queries WHERE session_id = ?1
                    ORDER BY id DESC LIMIT 1), 0)",
                [session_id.0],
                |r| r.get(0),
            )?;

            // 最长同 person_id 连续前缀：(start, end, person)。
            let mut span: Option<(i64, i64, i64)> = None;
            {
                let mut stmt = tx.prepare(&format!(
                    "SELECT e.id, e.person_id FROM session_entries e
                     WHERE e.session_id = ?1 AND e.id > ?2 AND {CLAIMABLE}
                     ORDER BY e.id"
                ))?;
                let mut rows = stmt.query(params![session_id.0, lower])?;
                while let Some(r) = rows.next()? {
                    let (id, person): (i64, i64) = (r.get(0)?, r.get(1)?);
                    match &mut span {
                        None => span = Some((id, id, person)),
                        Some((_, end, p)) if *p == person => *end = id,
                        Some(_) => break,
                    }
                }
            }
            let Some((start, end, person)) = span else {
                return Ok(None);
            };

            tx.execute(
                "INSERT INTO queries
                   (session_id, person_id, claimed_start_id, claimed_end_id, state, created_at)
                 VALUES (?1, ?2, ?3, ?4, 'executing', ?5)",
                params![session_id.0, person, start, end, now],
            )?;
            let id = QueryId(tx.last_insert_rowid());
            tx.commit()?;
            Ok(Some(Query {
                id,
                session_id,
                person_id: PersonId(person),
                claimed_start_id: SessionEntryId(start),
                claimed_end_id: SessionEntryId(end),
                state: QueryState::Executing,
                created_at: now,
            }))
        })
        .await
    }

    pub async fn finish_query(
        &self,
        id: QueryId,
        state: QueryState,
        now: i64,
    ) -> Result<(), StoreError> {
        self.call(move |conn| {
            let (state, reason) = row::state_cols(&state)?;
            conn.execute(
                "UPDATE queries SET state = ?2, reason = ?3, finished_at = ?4 WHERE id = ?1",
                params![id.0, state, reason, now],
            )?;
            Ok(())
        })
        .await
    }

    /// 启动时把遗留 `Executing` 收尾为 `Failed{Interrupted}`，返回被收尾的 Query。
    pub async fn interrupt_stale_queries(&self, now: i64) -> Result<Vec<Query>, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut stale = tx
                .prepare(&format!(
                    "SELECT {QUERY_COLS} FROM queries WHERE state = 'executing' ORDER BY id"
                ))?
                .query_map([], row::query)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let state = QueryState::Failed {
                reason: FailureReason::Interrupted,
            };
            let (state_col, reason) = row::state_cols(&state)?;
            tx.execute(
                "UPDATE queries SET state = ?1, reason = ?2, finished_at = ?3
                 WHERE state = 'executing'",
                params![state_col, reason, now],
            )?;
            tx.commit()?;
            for q in &mut stale {
                q.state = state.clone();
            }
            Ok(stale)
        })
        .await
    }

    // ---- 读 ----

    pub async fn context_window(&self, session_id: SessionId) -> Result<ContextWindow, StoreError> {
        self.call(move |conn| {
            let boundary = conn
                .query_row(
                    &format!(
                        "SELECT {ENTRY_COLS} FROM session_entries e
                         WHERE e.session_id = ?1 AND e.entry_kind = 'boundary'
                         ORDER BY e.id DESC LIMIT 1"
                    ),
                    [session_id.0],
                    row::entry,
                )
                .optional()?;
            let (after, summary) = match boundary {
                Some(SessionEntry::Boundary(b)) => (
                    b.id.0,
                    match b.boundary {
                        mic_message::ContextBoundary::Compaction { summary } => Some(summary),
                        mic_message::ContextBoundary::UserClear => None,
                    },
                ),
                _ => (0, None),
            };
            let messages = conn
                .prepare(&format!(
                    "SELECT {ENTRY_COLS} FROM session_entries e
                     WHERE e.session_id = ?1 AND e.id > ?2 AND e.entry_kind = 'message'
                     ORDER BY e.id"
                ))?
                .query_map(params![session_id.0, after], row::message)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            Ok(ContextWindow { summary, messages })
        })
        .await
    }

    /// 稳定 entry 回放；Gateway 实时 chunk 的续传游标另由 Gateway B2 定义。
    pub async fn entries_after(
        &self,
        session_id: SessionId,
        after: Option<SessionEntryId>,
    ) -> Result<Vec<SessionEntry>, StoreError> {
        let after = after.map_or(0, |id| id.0);
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {ENTRY_COLS} FROM session_entries e
                     WHERE e.session_id = ?1 AND e.id > ?2
                     ORDER BY e.id"
                ))?
                .query_map(params![session_id.0, after], row::entry)?
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
                    "SELECT {ENTRY_COLS}, s.delivery_channel, s.delivery_version, s.delivery_payload
                     FROM session_entries e JOIN sessions s ON s.id = e.session_id
                     WHERE s.delivery_channel = ?1
                       AND e.delivered_at IS NULL
                       AND e.author_kind IN ('assistant', 'notification')
                       AND e.content_kind IN ('text', 'attachment')
                     ORDER BY e.id"
                ))?
                .query_map([channel], |r| {
                    let target = row::delivery_target(r, 8)?.ok_or_else(|| {
                        rusqlite::Error::InvalidColumnType(
                            8,
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

    pub async fn mark_delivered(&self, id: SessionEntryId, at: i64) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute(
                "UPDATE session_entries SET delivered_at = ?2 WHERE id = ?1",
                params![id.0, at],
            )?;
            Ok(())
        })
        .await
    }

    // ---- 用量 ----

    pub async fn record_model_call(&self, input: ModelCallInput) -> Result<(), StoreError> {
        self.call(move |conn| {
            let query_id = match input.purpose {
                ModelCallPurpose::Query(id) => Some(id.0),
                ModelCallPurpose::Compaction => None,
            };
            let outcome = serde_json::to_string(&input.outcome)?;
            let u = input.usage;
            conn.execute(
                "INSERT INTO model_calls
                   (session_id, query_id, model, input_tokens, output_tokens,
                    cache_read_tokens, cache_write_tokens, reasoning_tokens,
                    outcome, started_at, finished_at)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11)",
                params![
                    input.session_id.0,
                    query_id,
                    input.model,
                    u.input_tokens as i64,
                    u.output_tokens as i64,
                    u.cache_read_tokens as i64,
                    u.cache_write_tokens as i64,
                    u.reasoning_tokens as i64,
                    outcome,
                    input.started_at,
                    input.finished_at,
                ],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn session_usage(&self, session_id: SessionId) -> Result<Usage, StoreError> {
        self.call(move |conn| {
            Ok(conn.query_row(
                "SELECT COALESCE(SUM(input_tokens), 0), COALESCE(SUM(output_tokens), 0),
                        COALESCE(SUM(cache_read_tokens), 0), COALESCE(SUM(cache_write_tokens), 0),
                        COALESCE(SUM(reasoning_tokens), 0)
                 FROM model_calls WHERE session_id = ?1",
                [session_id.0],
                |r| {
                    Ok(Usage {
                        input_tokens: r.get::<_, i64>(0)? as u64,
                        output_tokens: r.get::<_, i64>(1)? as u64,
                        cache_read_tokens: r.get::<_, i64>(2)? as u64,
                        cache_write_tokens: r.get::<_, i64>(3)? as u64,
                        reasoning_tokens: r.get::<_, i64>(4)? as u64,
                    })
                },
            )?)
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
        "INSERT INTO persons (name, created_at) VALUES (?1, ?2) ON CONFLICT(name) DO NOTHING",
        params![name, now],
    )?;
    Ok(PersonId(tx.query_row(
        "SELECT id FROM persons WHERE name = ?1",
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
            "SELECT person_id FROM person_identities WHERE channel = ?1 AND external_id = ?2",
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
        "INSERT INTO sessions
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

fn insert_message(
    conn: &Connection,
    session_id: SessionId,
    author: &MessageAuthor,
    content: &MessageContent,
    person: Option<PersonId>,
    created_at: i64,
) -> Result<SessionEntryId, StoreError> {
    let (author_kind, author_ident) = row::author_cols(author);
    let payload = serde_json::to_string(content)?;
    conn.execute(
        "INSERT INTO session_entries
           (session_id, entry_kind, author_kind, author_ident, content_kind,
            person_id, payload, created_at)
         VALUES (?1, 'message', ?2, ?3, ?4, ?5, ?6, ?7)",
        params![
            session_id.0,
            author_kind,
            author_ident,
            row::content_kind(content),
            person.map(|p| p.0),
            payload,
            created_at,
        ],
    )?;
    Ok(SessionEntryId(conn.last_insert_rowid()))
}
