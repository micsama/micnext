use std::num::NonZeroU32;

use mic_message::{ContentPart, Message, MessageBody, MessageId, PersonId, SessionId};
use mic_store::rusqlite::{self, Transaction};
use mic_store::{
    NewSession, RunId, Session, SessionCursor, SessionKind, SessionPage, Store, StoreError,
};
use tokio::sync::mpsc;

use crate::event::{EventReceiver, Events, KernelEventKind};
use crate::run::now_ms;
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

    /// 写入未认领的用户输入并唤醒该会话的调度（不等执行）。会话正在执行时，
    /// 新输入由当前 run 在下一个模型调用边界并入。内核已停止时只写不唤醒，下次启动补跑。
    pub async fn append_user_input(
        &self,
        session_id: SessionId,
        person: PersonId,
        parts: Vec<ContentPart>,
    ) -> Result<MessageId, KernelError> {
        let message =
            append_user_input(&self.store, &self.events, session_id, person, parts).await?;
        // 调度循环已退出（停止中）时发送失败，按上面的约定忽略。
        let _ = self.wake.send(session_id).await;
        Ok(message.id)
    }

    /// id 可能来自外部，不存在返回 `None`。
    pub async fn session(&self, id: SessionId) -> Result<Option<Session>, KernelError> {
        Ok(self.store.session(id).await?)
    }

    /// 列出 `channel` 下已有用户输入的 Root 会话，最近活跃在前。
    pub async fn list_root_sessions(
        &self,
        channel: &str,
        before: Option<SessionCursor>,
        limit: NonZeroU32,
    ) -> Result<SessionPage, KernelError> {
        Ok(self
            .store
            .list_root_sessions(channel, before, limit)
            .await?)
    }

    /// 稳定回放（含 `Boundary`）。
    pub async fn messages_after(
        &self,
        session_id: SessionId,
        after: Option<MessageId>,
    ) -> Result<Vec<Message>, KernelError> {
        Ok(self.store.messages_after(session_id, after).await?)
    }

    /// 该会话正在执行的 run。
    pub async fn executing_run(&self, session_id: SessionId) -> Result<Option<RunId>, KernelError> {
        Ok(self.store.executing_run(session_id).await?)
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

/// 写入用户输入并发 `MessageAppended`；`Kernel` 与 `-p` 共用。
pub(crate) async fn append_user_input(
    store: &Store,
    events: &Events,
    session_id: SessionId,
    person: PersonId,
    parts: Vec<ContentPart>,
) -> Result<Message, StoreError> {
    // 会话不存在时由外键在写入处报错，之后必能读到。
    let message = store
        .append(
            session_id,
            None,
            MessageBody::UserInput { person, parts },
            now_ms(),
        )
        .await?;
    let session = store
        .session(session_id)
        .await?
        .expect("刚写入消息的会话必然存在");
    let channel = session_channel(store, &session).await?;
    events.emit(
        session_id,
        &channel,
        KernelEventKind::MessageAppended(message.clone()),
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
