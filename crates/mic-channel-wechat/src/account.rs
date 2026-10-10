use std::fmt;
use std::time::{SystemTime, UNIX_EPOCH};

use mic_core::{ChannelConnection, Kernel, LinkedChannel};
use mic_message::{PersonId, SessionId};
use mic_store::rusqlite::{params, OptionalExtension};
use mic_store::{
    DeliveryTarget, Identity, Migration, NewSession, SecretColumn, SessionKind, ToolScope,
};
use reqwest::Url;
use serde::{Deserialize, Serialize};

use crate::client::{https_base, required, ClientError};
use crate::limits::{LOCAL_TOKEN_LIMIT, LONGPOLL_TIMEOUT_MS};

pub(crate) const MIGRATIONS: &[Migration] = &[Migration {
    module: "wechat",
    version: 1,
    sql: "
CREATE TABLE wechat_account (
  user_id TEXT PRIMARY KEY,
  bot_id TEXT NOT NULL,
  bot_token TEXT NOT NULL,
  base_url TEXT NOT NULL,
  person_id INTEGER NOT NULL REFERENCES core_persons(id),
  session_id INTEGER NOT NULL REFERENCES core_sessions(id),
  connection TEXT NOT NULL CHECK(connection IN ('connected', 'needs_login', 'faulted')),
  updated_at INTEGER NOT NULL
);
CREATE TABLE wechat_settings (
  id INTEGER PRIMARY KEY CHECK(id = 1),
  active_user_id TEXT REFERENCES wechat_account(user_id)
);
INSERT INTO wechat_settings(id) VALUES (1);
CREATE TABLE wechat_state (
  user_id TEXT PRIMARY KEY REFERENCES wechat_account(user_id),
  get_updates_buf TEXT NOT NULL,
  context_token TEXT,
  longpoll_timeout_ms INTEGER NOT NULL
);
CREATE TABLE wechat_inbound_batch (
  id INTEGER PRIMARY KEY,
  user_id TEXT NOT NULL REFERENCES wechat_account(user_id),
  snapshot_version INTEGER NOT NULL,
  snapshot TEXT NOT NULL,
  phase TEXT NOT NULL CHECK(phase IN ('importing', 'completed', 'interrupted')),
  created_at INTEGER NOT NULL,
  finished_at INTEGER
);
CREATE TABLE wechat_delivery (
  message_id INTEGER PRIMARY KEY REFERENCES core_messages(id),
  user_id TEXT NOT NULL REFERENCES wechat_account(user_id),
  plan_version INTEGER NOT NULL,
  plan TEXT NOT NULL,
  outcome TEXT NOT NULL CHECK(outcome IN ('sending', 'sent', 'skipped', 'interrupted')),
  finished_at INTEGER
);
CREATE TABLE wechat_delivery_attempt (
  message_id INTEGER NOT NULL REFERENCES wechat_delivery(message_id),
  chunk_index INTEGER NOT NULL CHECK(chunk_index >= 0),
  attempt_no INTEGER NOT NULL CHECK(attempt_no IN (1, 2)),
  client_id TEXT NOT NULL,
  started_at INTEGER NOT NULL,
  finished_at INTEGER,
  external_message_id TEXT,
  failure TEXT,
  PRIMARY KEY (message_id, chunk_index, attempt_no)
);
",
    secret_columns: &[
        SecretColumn {
            table: "wechat_account",
            column: "bot_token",
        },
        SecretColumn {
            table: "wechat_state",
            column: "context_token",
        },
    ],
}];

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub(crate) struct WechatUserId(pub String);

pub(crate) struct Token(String);

impl Token {
    pub fn new(value: String) -> Self {
        Self(value)
    }
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Token {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Token(..)")
    }
}

pub(crate) struct Credentials {
    pub user_id: WechatUserId,
    pub bot_id: String,
    pub token: Token,
    pub base_url: Url,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct WechatTarget {
    pub user_id: WechatUserId,
}

pub(crate) struct Account {
    pub credentials: Credentials,
    pub person: PersonId,
    pub view: LinkedChannel,
}

struct RawAccount {
    user_id: String,
    bot_id: String,
    token: String,
    base_url: String,
    person: PersonId,
    session: SessionId,
    connection: String,
}

#[derive(Debug, thiserror::Error)]
pub(crate) enum AccountError {
    #[error(transparent)]
    Kernel(#[from] mic_core::KernelError),
    #[error(transparent)]
    Protocol(#[from] ClientError),
    #[error("微信磁盘数据无效：{0}")]
    Invalid(&'static str),
}

pub(crate) async fn active(kernel: &Kernel) -> Result<Option<Account>, AccountError> {
    let user = kernel
        .with_module_tx(|tx| {
            tx.query_row(
                "SELECT active_user_id FROM wechat_settings WHERE id = 1",
                [],
                |row| row.get::<_, Option<String>>(0),
            )
        })
        .await?;
    match user {
        Some(user) => Ok(Some(
            find(kernel, &user)
                .await?
                .ok_or(AccountError::Invalid("启用对象不存在"))?,
        )),
        None => Ok(None),
    }
}

async fn find(kernel: &Kernel, user: &str) -> Result<Option<Account>, AccountError> {
    let user = user.to_owned();
    let raw = kernel
        .with_module_tx(move |tx| {
            tx.query_row(
                "SELECT user_id, bot_id, bot_token, base_url, person_id, session_id, connection
             FROM wechat_account WHERE user_id = ?1",
                [user],
                |row| {
                    Ok(RawAccount {
                        user_id: row.get(0)?,
                        bot_id: row.get(1)?,
                        token: row.get(2)?,
                        base_url: row.get(3)?,
                        person: PersonId(row.get(4)?),
                        session: SessionId(row.get(5)?),
                        connection: row.get(6)?,
                    })
                },
            )
            .optional()
        })
        .await?;
    let Some(raw) = raw else { return Ok(None) };
    let connection = match raw.connection.as_str() {
        "connected" => ChannelConnection::Connected,
        "needs_login" => ChannelConnection::NeedsLogin,
        "faulted" => ChannelConnection::Faulted,
        _ => return Err(AccountError::Invalid("连接状态未知")),
    };
    let credentials = Credentials {
        user_id: WechatUserId(required(Some(raw.user_id), "磁盘 user_id 为空")?),
        bot_id: required(Some(raw.bot_id), "磁盘 bot_id 为空")?,
        token: Token::new(required(Some(raw.token), "磁盘 token 为空")?),
        base_url: https_base(&raw.base_url)?,
    };
    let session = kernel
        .session(raw.session)
        .await?
        .ok_or(AccountError::Invalid("会话不存在"))?;
    if !matches!(&session.kind, SessionKind::Root { channel, chat }
        if channel == "wechat" && chat == &credentials.user_id.0)
    {
        return Err(AccountError::Invalid("会话归属不一致"));
    }
    let target = session
        .delivery_target
        .ok_or(AccountError::Invalid("缺少投递目标"))?;
    if target.channel != "wechat" || target.version != 1 {
        return Err(AccountError::Invalid("投递目标版本或渠道不匹配"));
    }
    let target: WechatTarget = serde_json::from_str(&target.payload)
        .map_err(|_| AccountError::Invalid("投递目标字段不匹配"))?;
    if target.user_id != credentials.user_id {
        return Err(AccountError::Invalid("投递目标归属不一致"));
    }
    Ok(Some(Account {
        view: LinkedChannel {
            account_id: credentials.user_id.0.clone(),
            user_id: credentials.user_id.0.clone(),
            session_id: raw.session,
            connection,
        },
        credentials,
        person: raw.person,
    }))
}

pub(crate) async fn local_tokens(kernel: &Kernel) -> Result<Vec<Token>, AccountError> {
    let tokens = kernel
        .with_module_tx(|tx| {
            tx.prepare("SELECT bot_token FROM wechat_account ORDER BY updated_at DESC LIMIT ?1")?
                .query_map([LOCAL_TOKEN_LIMIT as i64], |row| row.get::<_, String>(0))?
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    tokens
        .into_iter()
        .map(|token| Ok(Token::new(required(Some(token), "磁盘 token 为空")?)))
        .collect()
}

pub(crate) async fn save(
    kernel: &Kernel,
    credentials: Credentials,
) -> Result<LinkedChannel, AccountError> {
    let user = &credentials.user_id.0;
    let person = kernel
        .resolve_identity(
            Identity {
                channel: "wechat".into(),
                external_id: user.clone(),
            },
            user,
        )
        .await?;
    let existing = find(kernel, user).await?;
    let session_id = match existing {
        Some(existing) => {
            if existing.person != person {
                return Err(AccountError::Invalid("身份归属不一致"));
            }
            existing.view.session_id
        }
        None => {
            let target = WechatTarget {
                user_id: credentials.user_id.clone(),
            };
            let payload = serde_json::to_string(&target).expect("投递目标只含字符串");
            kernel
                .resolve_root_session(
                    "wechat",
                    user,
                    NewSession {
                        kind: SessionKind::Root {
                            channel: "wechat".into(),
                            chat: user.clone(),
                        },
                        parent_session_id: None,
                        delivery_target: Some(DeliveryTarget {
                            channel: "wechat".into(),
                            version: 1,
                            payload,
                        }),
                        pwd: kernel.default_workdir().await?,
                        tool_scope: ToolScope::All,
                        created_at: now_ms(),
                        persona: None,
                        model: None,
                    },
                )
                .await?
                .id
        }
    };
    let view = LinkedChannel {
        account_id: user.clone(),
        user_id: user.clone(),
        session_id,
        connection: ChannelConnection::Connected,
    };
    kernel.with_module_tx(move |tx| {
        tx.execute(
            "INSERT INTO wechat_account(user_id, bot_id, bot_token, base_url, person_id, session_id, connection, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'connected', ?7)
             ON CONFLICT(user_id) DO UPDATE SET bot_id=excluded.bot_id, bot_token=excluded.bot_token,
             base_url=excluded.base_url, connection=excluded.connection, updated_at=excluded.updated_at",
            params![credentials.user_id.0, credentials.bot_id, credentials.token.expose(), credentials.base_url.as_str(), person.0, session_id.0, now_ms()],
        )?;
        tx.execute(
            "INSERT INTO wechat_state(user_id, get_updates_buf, context_token, longpoll_timeout_ms)
             VALUES (?1, '', NULL, ?2) ON CONFLICT(user_id) DO UPDATE SET
             get_updates_buf='', context_token=NULL, longpoll_timeout_ms=excluded.longpoll_timeout_ms",
            params![credentials.user_id.0, LONGPOLL_TIMEOUT_MS],
        )?;
        tx.execute("UPDATE wechat_settings SET active_user_id = ?1 WHERE id = 1", [credentials.user_id.0])?;
        Ok(())
    }).await?;
    Ok(view)
}

pub(crate) struct State {
    pub cursor: String,
    pub context_token: Option<String>,
    pub timeout_ms: u32,
}

pub(crate) async fn state(kernel: &Kernel, user: &WechatUserId) -> Result<State, AccountError> {
    let user = user.0.clone();
    let state = kernel.with_module_tx(move |tx| {
        tx.query_row("SELECT get_updates_buf, context_token, longpoll_timeout_ms FROM wechat_state WHERE user_id = ?1",
            [user], |row| Ok(State { cursor: row.get(0)?, context_token: row.get(1)?, timeout_ms: row.get(2)? }))
    }).await?;
    if state.timeout_ms == 0
        || state
            .context_token
            .as_ref()
            .is_some_and(|token| token.trim().is_empty())
    {
        return Err(AccountError::Invalid("入站状态字段无效"));
    }
    Ok(state)
}

pub(crate) async fn set_connection(
    kernel: &Kernel,
    user: &WechatUserId,
    connection: ChannelConnection,
) -> Result<(), AccountError> {
    let connection = match connection {
        ChannelConnection::Connected => "connected",
        ChannelConnection::NeedsLogin => "needs_login",
        ChannelConnection::Faulted => "faulted",
    };
    let user = user.0.clone();
    kernel
        .with_module_tx(move |tx| {
            tx.execute(
                "UPDATE wechat_account SET connection = ?2, updated_at = ?3 WHERE user_id = ?1",
                params![user, connection, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

pub(crate) async fn persist_batch(
    kernel: &Kernel,
    user: &WechatUserId,
    updates: &crate::client::Updates,
    timeout_ms: u32,
) -> Result<Option<i64>, AccountError> {
    let snapshot = match updates.snapshot.msgs.as_ref() {
        Some(messages) if !messages.is_empty() => {
            Some(serde_json::to_string(&updates.snapshot).expect("完整响应可序列化"))
        }
        _ => None,
    };
    let user = user.0.clone();
    let cursor = updates.cursor.clone();
    Ok(kernel.with_module_tx(move |tx| {
        let batch = match snapshot {
            Some(snapshot) => {
                tx.execute("INSERT INTO wechat_inbound_batch(user_id, snapshot_version, snapshot, phase, created_at) VALUES (?1, 1, ?2, 'importing', ?3)", params![user, snapshot, now_ms()])?;
                Some(tx.last_insert_rowid())
            }
            None => None,
        };
        tx.execute("UPDATE wechat_state SET get_updates_buf = ?2, longpoll_timeout_ms = ?3 WHERE user_id = ?1", params![user, cursor, timeout_ms])?;
        Ok(batch)
    }).await?)
}

pub(crate) async fn remember_context(
    kernel: &Kernel,
    user: &WechatUserId,
    context_token: &str,
) -> Result<(), AccountError> {
    let user = user.0.clone();
    let context_token = context_token.to_owned();
    kernel
        .with_module_tx(move |tx| {
            tx.execute(
                "UPDATE wechat_state SET context_token = ?2 WHERE user_id = ?1",
                params![user, context_token],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

pub(crate) async fn finish_batch(kernel: &Kernel, id: i64) -> Result<(), AccountError> {
    kernel.with_module_tx(move |tx| {
        tx.execute("UPDATE wechat_inbound_batch SET phase = 'completed', finished_at = ?2 WHERE id = ?1", params![id, now_ms()])?;
        tx.execute("DELETE FROM wechat_inbound_batch WHERE id = ?1 AND phase = 'completed'", [id])?;
        Ok(())
    }).await?;
    Ok(())
}

pub(crate) async fn recover_batches(kernel: &Kernel) -> Result<(), AccountError> {
    let batches = kernel.with_module_tx(|tx| {
        tx.prepare("SELECT user_id, snapshot_version, snapshot FROM wechat_inbound_batch WHERE phase = 'importing'")?
            .query_map([], |row| Ok((row.get::<_, String>(0)?, row.get::<_, u32>(1)?, row.get::<_, String>(2)?)))?
            .collect::<Result<Vec<_>, _>>()
    }).await?;
    for (user, version, snapshot) in batches {
        if version != 1 {
            return Err(AccountError::Invalid("入站 snapshot 版本不匹配"));
        }
        let snapshot = serde_json::from_str(&snapshot)
            .map_err(|_| AccountError::Invalid("入站 snapshot 字段不匹配"))?;
        crate::client::parse_updates(snapshot, &WechatUserId(user))?;
    }
    kernel.with_module_tx(|tx| {
        tx.execute("UPDATE wechat_inbound_batch SET phase = 'interrupted', finished_at = ?1 WHERE phase = 'importing'", [now_ms()])?;
        Ok(())
    }).await?;
    Ok(())
}

pub(crate) fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟在 epoch 之后")
        .as_millis() as i64
}
