use mic_message::{Message, MessageAuthor, PersonId, SessionEntry, SessionEntryId, SessionId};
use mic_store::rusqlite::{self, Transaction};
use mic_store::{NewSession, Session, SessionKind, Store, StoreError, UserInput};
use tokio::sync::mpsc;

use crate::event::{EventReceiver, Events, KernelEventKind};
use crate::KernelError;

/// 模块拿到的内核窄接口。Clone 廉价；核心表只经这些方法写。
#[derive(Clone)]
pub struct Kernel {
    store: Store,
    owner: PersonId,
    events: Events,
    wake: mpsc::Sender<SessionId>,
}

impl Kernel {
    pub(crate) fn new(
        store: Store,
        owner: PersonId,
        events: Events,
        wake: mpsc::Sender<SessionId>,
    ) -> Self {
        Self {
            store,
            owner,
            events,
            wake,
        }
    }

    /// 配置声明的 owner person。单用户：Web 与 `-p` 都以它身份写入。
    pub fn owner(&self) -> PersonId {
        self.owner
    }

    /// Channel 用：按 (channel, chat) 原子取得或新建 Root 会话。
    pub async fn resolve_root_session(
        &self,
        channel: &str,
        chat: &str,
        init: NewSession,
    ) -> Result<Session, KernelError> {
        Ok(self.store.resolve_root_session(channel, chat, init).await?)
    }

    /// Task/Triggered 用。
    pub async fn create_session(&self, s: NewSession) -> Result<SessionId, KernelError> {
        Ok(self.store.create_session(s).await?.id)
    }

    /// 写入可 claim 的用户输入并唤醒该会话的调度（不等执行）。会话正在执行时，
    /// 新输入由当前 Query 在下一个模型调用边界吸收。内核已停止时只写不唤醒，下次启动补跑。
    pub async fn append_user_input(&self, input: UserInput) -> Result<SessionEntryId, KernelError> {
        let entry = append_user_input(&self.store, &self.events, input).await?;
        // 调度循环已退出（停止中）时发送失败，按上面的约定忽略。
        let _ = self.wake.send(entry.session_id).await;
        Ok(entry.id)
    }

    /// 订阅之后产生的事件（全部会话，按 `channel`/`session_id` 字段过滤）。
    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    /// 只许访问本模块 `{name}_` 前缀的表；由模块单元测试把关。
    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, KernelError>
    where
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        Ok(self.store.with_module_tx(f).await?)
    }
}

/// 写入用户输入并发 `EntryAppended`；`Kernel` 与 `-p` 共用。
pub(crate) async fn append_user_input(
    store: &Store,
    events: &Events,
    input: UserInput,
) -> Result<Message, StoreError> {
    let message = Message {
        id: SessionEntryId(0),
        session_id: input.session_id,
        author: MessageAuthor::User {
            id: input.person_id,
        },
        content: input.content.clone(),
        created_at: input.created_at,
        delivered_at: None,
    };
    // 会话不存在时由外键在写入处报错，之后必能读到。
    let id = store.append_user_input(input).await?;
    let message = Message { id, ..message };
    let session = store
        .session(message.session_id)
        .await?
        .expect("刚写入 entry 的会话必然存在");
    let channel = session_channel(store, &session).await?;
    events.emit(
        message.session_id,
        &channel,
        KernelEventKind::EntryAppended(SessionEntry::Message(message.clone())),
    );
    Ok(message)
}

/// 会话所属 Channel：Root 取自身，Task 取根会话；Triggered 不属于任何 Channel，取触发它的模块名。
pub(crate) async fn session_channel(
    store: &Store,
    session: &Session,
) -> Result<String, StoreError> {
    let mut current = session.clone();
    loop {
        match &current.kind {
            SessionKind::Root { channel, .. } => return Ok(channel.clone()),
            SessionKind::Triggered { module, .. } => return Ok(module.clone()),
            SessionKind::Task { .. } => {
                let parent = current
                    .parent_session_id
                    .expect("Task 会话必有父会话（store 保证）");
                current = store
                    .session(parent)
                    .await?
                    .expect("父会话必然存在（外键保证）");
            }
        }
    }
}
