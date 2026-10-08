use std::num::NonZeroU32;
use std::sync::Arc;

use mic_message::{ContentPart, Message, MessageBody, MessageId, PersonId, SessionId};
use mic_store::rusqlite::{self, Transaction};
use mic_store::{
    CredentialWrite, EndpointId, EndpointView, EndpointWrite, ModelId, ModelView, ModelWrite,
    NewSession, Persona, PersonaId, RunId, Session, SessionCursor, SessionKind, SessionPage,
    SessionSummary, Settings, Store, StoreError,
};
use tokio::sync::mpsc;

use crate::event::{EventReceiver, Events};
use crate::provider::{resolve_key, Factories};
use crate::run::now_ms;
use crate::{request, KernelError, ProviderKindView};

/// 模块拿到的内核窄接口。Clone 廉价；核心表只经这些方法写。
#[derive(Clone)]
pub struct Kernel {
    store: Store,
    owner: PersonId,
    events: Events,
    wake: mpsc::Sender<SessionId>,
    factories: Arc<Factories>,
}

/// 创建/修改服务商的输入；`config_json` 由对应 kind 的工厂解析。
pub struct EndpointDraft {
    pub name: String,
    pub kind: String,
    pub config_json: String,
    pub credential: CredentialWrite,
}

/// 创建/修改模型的输入；`config_json` 是该 kind 的模型参数。
pub struct ModelDraft {
    pub endpoint_id: EndpointId,
    pub name: String,
    pub config_json: String,
}

impl Kernel {
    pub(crate) fn new(
        store: Store,
        owner: PersonId,
        events: Events,
        wake: mpsc::Sender<SessionId>,
        factories: Arc<Factories>,
    ) -> Self {
        Self {
            store,
            owner,
            events,
            wake,
            factories,
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

    /// id 可能来自外部，不存在返回 `None`。
    pub async fn session_summary(
        &self,
        id: SessionId,
    ) -> Result<Option<SessionSummary>, KernelError> {
        Ok(self.store.session_summary(id).await?)
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

    /// 系统提示词（不可改），供设置页只读展示。
    pub fn system_prompt(&self) -> &'static str {
        request::base_prompt()
    }

    pub async fn settings(&self) -> Result<Settings, KernelError> {
        Ok(self.store.settings().await?)
    }

    /// 下一轮 run 起生效。
    pub async fn update_settings(&self, s: Settings) -> Result<(), KernelError> {
        Ok(self.store.update_settings(s).await?)
    }

    /// 未删除的人设，内置在前。
    pub async fn personas(&self) -> Result<Vec<Persona>, KernelError> {
        Ok(self.store.personas().await?)
    }

    /// 含已删除。
    pub async fn persona(&self, id: PersonaId) -> Result<Option<Persona>, KernelError> {
        Ok(self.store.persona(id).await?)
    }

    pub async fn create_persona(
        &self,
        name: String,
        prompt: String,
    ) -> Result<PersonaId, KernelError> {
        Ok(self.store.create_persona(name, prompt, now_ms()).await?)
    }

    pub async fn update_persona(
        &self,
        id: PersonaId,
        name: String,
        prompt: String,
    ) -> Result<(), KernelError> {
        Ok(self
            .store
            .update_persona(id, name, prompt, now_ms())
            .await?)
    }

    pub async fn delete_persona(&self, id: PersonaId) -> Result<(), KernelError> {
        Ok(self.store.delete_persona(id, now_ms()).await?)
    }

    /// 会话下一轮用的人设；正在执行的 run 不受影响。
    pub async fn set_session_persona(
        &self,
        session: SessionId,
        persona: PersonaId,
    ) -> Result<(), KernelError> {
        Ok(self.store.set_session_persona(session, persona).await?)
    }

    /// 可创建的模型类型，来自各模型模块登记的工厂。
    pub fn provider_kinds(&self) -> Vec<ProviderKindView> {
        self.factories.kinds()
    }

    /// 未删除的服务商。
    pub async fn endpoints(&self) -> Result<Vec<EndpointView>, KernelError> {
        Ok(self.store.endpoints().await?)
    }

    /// 仅未删除的；id 可能来自外部，不存在返回 `None`。
    pub async fn endpoint(&self, id: EndpointId) -> Result<Option<EndpointView>, KernelError> {
        Ok(self.store.endpoint(id).await?)
    }

    /// 服务商未保存 key 时读的环境变量名（供界面标注）。
    pub fn key_env(&self, kind: &str, endpoint_json: &str) -> Option<&'static str> {
        self.factories.get(kind).ok()?.key_env(endpoint_json)
    }

    pub async fn create_endpoint(&self, draft: EndpointDraft) -> Result<EndpointId, KernelError> {
        let write = self.checked_endpoint(draft)?;
        Ok(self.store.create_endpoint(write, now_ms()).await?)
    }

    /// 下一轮 run 起生效；进行中的 run 不受影响。
    pub async fn update_endpoint(
        &self,
        id: EndpointId,
        draft: EndpointDraft,
    ) -> Result<(), KernelError> {
        let write = self.checked_endpoint(draft)?;
        Ok(self.store.update_endpoint(id, write, now_ms()).await?)
    }

    /// 连同其下全部模型一起删除。
    pub async fn delete_endpoint(&self, id: EndpointId) -> Result<(), KernelError> {
        Ok(self.store.delete_endpoint(id, now_ms()).await?)
    }

    /// 用表单当前值联网取模型列表；`Keep` 沿用 `existing` 已存的 key。不写任何状态。
    pub async fn test_endpoint(
        &self,
        draft: EndpointDraft,
        existing: Option<EndpointId>,
    ) -> Result<Vec<String>, KernelError> {
        let factory = self.factories.get(&draft.kind)?;
        let endpoint_json = factory.check_endpoint(&draft.config_json)?;
        let stored = match (draft.credential, existing) {
            (CredentialWrite::Set(key), _) => Some(key),
            (CredentialWrite::Clear, _) => None,
            (CredentialWrite::Keep, Some(id)) => self.store.endpoint_key(id).await?,
            (CredentialWrite::Keep, None) => {
                return Err(mic_store::ModelSettingsError::InvalidSecretEdit.into())
            }
        };
        let key = resolve_key(stored, factory.key_env(&endpoint_json));
        Ok(factory.list_models(&endpoint_json, key).await?)
    }

    /// 未删除的模型，可按服务商筛。
    pub async fn models(&self) -> Result<Vec<ModelView>, KernelError> {
        Ok(self.store.models().await?)
    }

    /// 仅未删除的；id 可能来自外部，不存在返回 `None`。
    pub async fn model(&self, id: ModelId) -> Result<Option<ModelView>, KernelError> {
        Ok(self.store.model(id).await?)
    }

    pub async fn default_model(&self) -> Result<Option<ModelId>, KernelError> {
        Ok(self.store.default_model().await?)
    }

    /// 首个模型自动成为默认。
    pub async fn create_model(&self, draft: ModelDraft) -> Result<ModelId, KernelError> {
        let write = self.checked_model(draft).await?;
        Ok(self.store.create_model(write, now_ms()).await?)
    }

    /// 下一轮 run 起生效；进行中的 run 不受影响。
    pub async fn update_model(&self, id: ModelId, draft: ModelDraft) -> Result<(), KernelError> {
        let write = self.checked_model(draft).await?;
        Ok(self.store.update_model(id, write, now_ms()).await?)
    }

    pub async fn delete_model(&self, id: ModelId) -> Result<(), KernelError> {
        Ok(self.store.delete_model(id, now_ms()).await?)
    }

    pub async fn set_default_model(&self, id: ModelId) -> Result<(), KernelError> {
        Ok(self.store.set_default_model(id).await?)
    }

    /// 会话下一轮用的模型；执行中拒绝。
    pub async fn set_session_model(
        &self,
        session: SessionId,
        model: ModelId,
    ) -> Result<(), KernelError> {
        Ok(self.store.set_session_model(session, model).await?)
    }

    fn checked_endpoint(&self, draft: EndpointDraft) -> Result<EndpointWrite, KernelError> {
        let config_json = self
            .factories
            .get(&draft.kind)?
            .check_endpoint(&draft.config_json)?;
        Ok(EndpointWrite {
            name: draft.name,
            kind: draft.kind,
            config_json,
            credential: draft.credential,
        })
    }

    async fn checked_model(&self, draft: ModelDraft) -> Result<ModelWrite, KernelError> {
        let endpoint = self
            .store
            .endpoint(draft.endpoint_id)
            .await?
            .ok_or(mic_store::ModelSettingsError::EndpointNotFound)?;
        let config_json = self.factories.get(&endpoint.kind)?.check_model(
            &endpoint.config_json,
            &draft.name,
            &draft.config_json,
        )?;
        Ok(ModelWrite {
            endpoint_id: draft.endpoint_id,
            name: draft.name,
            config_json,
        })
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
    let publish = events.publisher().await;
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
    publish.appended(session_id, &channel, message.clone());
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
