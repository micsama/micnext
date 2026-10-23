use std::collections::{BTreeMap, HashMap};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::sync::Arc;

use mic_message::{
    ImageData, ImageId, Message, MessageBody, MessageId, PersonId, ReplyBlock, SessionCommand,
    SessionId,
};
use mic_store::rusqlite::{self, Transaction};
use mic_store::{
    CallRecord, CredentialWrite, Dashboard, EndpointId, EndpointView, EndpointWrite, Identity,
    InputDisposition, JobId, MessagePage, ModelCallId, ModelCallPurpose, ModelId, ModelView,
    ModelWrite, NewInputPart, NewNotice, NewSession, Peers, PendingDelivery, Persona, PersonaId,
    PushId, PushPage, QueryResult, ReasoningEffort, RunId, Session, SessionCursor, SessionKind,
    SessionPage, SessionSummary, Settings, Store, StoreError, TableSchema, TraceRunPage,
    TraceSession,
};
use mic_tool::ToolSpec;
use tokio::sync::mpsc;

use crate::compaction::{self, Occupancy};
use crate::context_usage::{self, ContextBreakdown};
use crate::event::{EventReceiver, Events};
use crate::input::{validate, IncomingPart};
use crate::jobs::{JobOutput, JobView, StopJobOutcome};
use crate::limits::JOB_READ_DEFAULT;
use crate::provider::{resolve_key, resolve_reasoning, Factories};
use crate::push::Pushes;
use crate::request::SystemPrompt;
use crate::run::{now_ms, Engine};
use crate::{request, ChannelSetup, KernelError, ProviderKindView, WorkdirError};

/// 模块拿到的内核窄接口。Clone 廉价；核心表只经这些方法写。
#[derive(Clone)]
pub struct Kernel {
    titles: crate::title::TitleHandle,
    store: Store,
    owner: PersonId,
    events: Events,
    wake: mpsc::Sender<SessionId>,
    factories: Arc<Factories>,
    channel_setups: Arc<HashMap<&'static str, Arc<dyn ChannelSetup>>>,
    pushes: Arc<Pushes>,
    engine: Arc<Engine>,
    /// 入站文件根目录 `<data_dir>/files`。
    files: PathBuf,
}

/// 默认推送 Channel 设置。
#[derive(Debug, Clone, serde::Serialize)]
pub struct PushSettings {
    /// 可选的 IM Channel；Web 通知中心恒收全部推送，不在其列。
    pub channels: Vec<&'static str>,
    /// 用户所选；空 = 未设。
    pub chosen: Vec<String>,
    /// 省略 `via` 时实际发往的 IM；空 = 只进 Web 通知中心。
    pub effective: Vec<String>,
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
    pub context_window: Option<NonZeroU32>,
    pub images: bool,
    pub endpoint_id: EndpointId,
    pub name: String,
    pub config_json: String,
    pub price: mic_store::ModelPrice,
}

/// 上下文接口的响应：压缩视图加当前占用。
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct ContextOverview {
    #[serde(flatten)]
    pub view: mic_store::ContextView,
    pub occupancy: Option<Occupancy>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopOutcome {
    /// 已发出停止；收尾结果经 `RunFinished` 推送。
    Stopping,
    NotRunning,
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsBackup {
    pub core: mic_store::SettingsExport,
    pub channels: BTreeMap<String, serde_json::Value>,
}

impl Kernel {
    pub fn channel_setups(&self) -> BTreeMap<&'static str, Arc<dyn ChannelSetup>> {
        self.channel_setups
            .iter()
            .map(|(&name, setup)| (name, setup.clone()))
            .collect()
    }

    pub async fn export_settings(&self) -> Result<mic_store::SettingsExport, KernelError> {
        Ok(self.store.export_settings().await?)
    }

    pub async fn import_settings(
        &self,
        mut s: mic_store::SettingsExport,
    ) -> Result<(), KernelError> {
        for endpoint in &mut s.endpoints {
            let factory = self
                .factories
                .get(&endpoint.kind)
                .map_err(|e| KernelError::BackupInvalid(e.to_string()))?;
            endpoint.config_json = factory
                .check_endpoint(&endpoint.config_json)
                .map_err(|e| KernelError::BackupInvalid(e.to_string()))?;
            for model in &mut endpoint.models {
                model.config_json = factory
                    .check_model(&endpoint.config_json, &model.name, &model.config_json)
                    .map_err(|e| KernelError::BackupInvalid(e.to_string()))?;
            }
        }
        if let Some(reference) = &s.preferences.title_model {
            if let Some(endpoint) = s.endpoints.iter().find(|e| e.name == reference.endpoint) {
                let options = self
                    .factories
                    .get(&endpoint.kind)?
                    .reasoning_options(&endpoint.config_json, &reference.model)
                    .map_err(|e| KernelError::BackupInvalid(e.to_string()))?;
                resolve_reasoning(options, s.preferences.title_reasoning_effort)
                    .map_err(|e| KernelError::BackupInvalid(e.to_string()))?;
            }
        }
        Ok(self.store.import_settings(s, now_ms()).await?)
    }

    pub fn seal_backup(&self, b: &SettingsBackup) -> String {
        let plain =
            mic_store::SecretValue::new(serde_json::to_string(b).expect("设置备份可序列化"));
        self.store.seal_export(&plain)
    }

    pub fn open_backup(&self, text: &str) -> Result<SettingsBackup, KernelError> {
        let plain = self
            .store
            .open_export(text)
            .map_err(|e| KernelError::BackupInvalid(e.to_string()))?;
        serde_json::from_str(plain.expose()).map_err(|e| {
            KernelError::BackupInvalid(format!(
                "备份字段不符（第 {} 行，第 {} 列）",
                e.line(),
                e.column()
            ))
        })
    }

    /// `run` 为 None 时停该会话执行中的 run；不等收尾。
    pub async fn stop_run(
        &self,
        session: SessionId,
        run: Option<RunId>,
    ) -> Result<StopOutcome, KernelError> {
        self.store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        self.store.ensure_session_active(session).await?;
        Ok(if self.engine.stop(session, run) {
            StopOutcome::Stopping
        } else {
            StopOutcome::NotRunning
        })
    }

    /// `session` 及其后代名下的后台任务；`include_old` 含 14 天前结束的。
    pub async fn jobs(
        &self,
        session: SessionId,
        include_old: bool,
    ) -> Result<Vec<JobView>, KernelError> {
        self.existing(session).await?;
        Ok(self.engine.jobs.views(session, include_old).await?)
    }

    /// `offset` 缺省读尾部；`limit` 缺省 16 KiB。
    pub async fn job_output(
        &self,
        session: SessionId,
        job: JobId,
        offset: Option<u64>,
        limit: Option<u64>,
    ) -> Result<JobOutput, KernelError> {
        self.existing(session).await?;
        Ok(self
            .engine
            .jobs
            .output(session, job, offset, limit.unwrap_or(JOB_READ_DEFAULT))
            .await?
            .1)
    }

    /// 用户终止后台任务，收尾完成后返回；owner 收到「已被用户终止」的回注。
    pub async fn stop_job(
        &self,
        session: SessionId,
        job: JobId,
    ) -> Result<StopJobOutcome, KernelError> {
        self.existing(session).await?;
        self.store.ensure_session_active(session).await?;
        Ok(self.engine.jobs.stop(session, job, true).await?)
    }

    async fn existing(&self, session: SessionId) -> Result<Session, KernelError> {
        self.store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)
    }

    /// 渠道收到 `/stop`：停当前 run，没有则回一条通知。
    pub async fn stop_command(&self, session: SessionId) -> Result<(), KernelError> {
        if self.stop_run(session, None).await? == StopOutcome::NotRunning {
            self.append_notification(
                session,
                crate::run::NOTIFICATION_SOURCE,
                "当前没有在执行的任务。".into(),
            )
            .await?;
        }
        Ok(())
    }

    pub async fn context_view(&self, session: SessionId) -> Result<ContextOverview, KernelError> {
        let session = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        Ok(ContextOverview {
            view: self.store.context_view(session.id).await?,
            occupancy: compaction::occupancy(&self.engine, &session).await?,
        })
    }

    pub async fn append_session_command(
        &self,
        session: SessionId,
        person: PersonId,
        command: SessionCommand,
    ) -> Result<MessageId, KernelError> {
        let session = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        self.store.ensure_session_active(session.id).await?;
        reject_task(&session)?;
        if let SessionCommand::RevokeCompaction { boundary_id } = &command {
            let view = self.store.context_view(session.id).await?;
            if !view
                .compactions
                .iter()
                .any(|v| v.boundary_id == *boundary_id)
            {
                return Err(mic_store::ContextCommandError::BoundaryNotFound.into());
            }
            if view.active_boundary != Some(*boundary_id) {
                return Err(mic_store::ContextCommandError::NotCurrentCompaction.into());
            }
        }
        let channel = session_channel(&self.store, &session).await?;
        let publish = self.events.publisher().await;
        let message = self
            .store
            .append_command(session.id, person, command, now_ms())
            .await?;
        let id = message.id;
        publish.appended(session.id, &channel, message);
        drop(publish);
        let _ = self.wake.send(session.id).await;
        Ok(id)
    }
    pub(crate) fn new(
        owner: PersonId,
        wake: mpsc::Sender<SessionId>,
        channel_setups: Arc<HashMap<&'static str, Arc<dyn ChannelSetup>>>,
        titles: crate::title::TitleHandle,
        engine: Arc<Engine>,
        files: PathBuf,
    ) -> Self {
        Self {
            files,
            titles,
            store: engine.store.clone(),
            owner,
            events: engine.events.clone(),
            wake,
            factories: engine.factories.clone(),
            pushes: Arc::new(Pushes::new(
                engine.store.clone(),
                engine.events.clone(),
                channel_setups.clone(),
                owner,
            )),
            channel_setups,
            engine,
        }
    }

    /// 配置声明的 owner person。单用户：Web 与 `-p` 都以它身份写入。
    pub fn owner(&self) -> PersonId {
        self.owner
    }

    pub fn channel_setup(&self, channel: &str) -> Option<Arc<dyn ChannelSetup>> {
        self.channel_setups.get(channel).cloned()
    }

    /// 按 Channel 外部身份取得 person 并刷新显示名；单用户，新身份归 owner。
    pub async fn resolve_identity(
        &self,
        identity: Identity,
        display_name: &str,
    ) -> Result<PersonId, KernelError> {
        Ok(self
            .store
            .resolve_identity(identity, display_name, self.owner, now_ms())
            .await?)
    }

    /// Channel 用：按 (channel, chat) 原子取得或新建 Root 会话。
    pub async fn resolve_root_session(
        &self,
        channel: &str,
        chat: &str,
        init: NewSession,
    ) -> Result<Session, KernelError> {
        self.check_reasoning(init.model, init.reasoning_effort)
            .await?;
        Ok(self.store.resolve_root_session(channel, chat, init).await?)
    }

    /// Task/Triggered 用。
    pub async fn create_session(&self, s: NewSession) -> Result<SessionId, KernelError> {
        self.check_reasoning(s.model, s.reasoning_effort).await?;
        Ok(self.store.create_session(s).await?.id)
    }

    /// 写入未认领的用户输入并唤醒该会话的调度（不等执行）。会话正在执行时，
    /// 新输入由当前 run 在下一个模型调用边界并入。内核已停止时只写不唤醒，下次启动处置为 held。
    pub async fn append_user_input(
        &self,
        session_id: SessionId,
        person: PersonId,
        parts: Vec<IncomingPart>,
    ) -> Result<MessageId, KernelError> {
        let session = self
            .store
            .session(session_id)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        self.store.ensure_session_active(session.id).await?;
        reject_task(&session)?;
        let input = validate(parts, &self.files, person).await?;
        let messages = append_input(
            &self.store,
            &self.events,
            session_id,
            person,
            input.parts,
            InputDisposition::Pending,
            None,
        )
        .await?;
        input.files.keep();
        // 调度循环已退出（停止中）时发送失败，按上面的约定忽略。
        let _ = self.wake.send(session_id).await;
        self.maybe_title(&messages[0]).await?;
        Ok(messages[0].id)
    }

    /// 只记录不执行的入站：输入以 held 落盘，与处置说明同事务，按 id 顺序发布，不唤醒。
    pub async fn append_recorded_input(
        &self,
        session_id: SessionId,
        person: PersonId,
        parts: Vec<IncomingPart>,
        source: &str,
        notice: String,
    ) -> Result<MessageId, KernelError> {
        let input = validate(parts, &self.files, person).await?;
        let messages = append_input(
            &self.store,
            &self.events,
            session_id,
            person,
            input.parts,
            InputDisposition::Held,
            Some(NewNotice {
                source: source.to_owned(),
                text: notice,
            }),
        )
        .await?;
        input.files.keep();
        self.maybe_title(&messages[0]).await?;
        Ok(messages[0].id)
    }

    /// Channel 通知落盘并发布，不唤醒模型。
    pub async fn append_notification(
        &self,
        session: SessionId,
        source: &str,
        text: String,
    ) -> Result<MessageId, KernelError> {
        let publish = self.events.publisher().await;
        let session = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        let channel = session_channel(&self.store, &session).await?;
        let message = self
            .store
            .append(
                session.id,
                None,
                MessageBody::Notification {
                    source: source.to_owned(),
                    text,
                },
                now_ms(),
            )
            .await?;
        let id = message.id;
        publish.appended(session.id, &channel, message);
        Ok(id)
    }

    pub async fn pending_deliveries(
        &self,
        channel: &str,
    ) -> Result<Vec<PendingDelivery>, KernelError> {
        Ok(self.store.pending_deliveries(channel).await?)
    }

    pub async fn mark_delivered(&self, id: MessageId) -> Result<(), KernelError> {
        self.store.mark_delivered(id, now_ms()).await?;
        self.delivery_changed(id).await
    }

    /// Channel 放弃投递；`reason` 给人看。
    pub async fn mark_delivery_failed(
        &self,
        id: MessageId,
        reason: String,
    ) -> Result<(), KernelError> {
        self.store.mark_delivery_failed(id, reason).await?;
        self.delivery_changed(id).await
    }

    async fn delivery_changed(&self, id: MessageId) -> Result<(), KernelError> {
        if let Some((push_id, source)) = self.store.push_of_delivery(id).await? {
            let session = self.store.session(source).await?.expect("来源会话存在");
            let channel = session_channel(&self.store, &session).await?;
            self.events.emit(
                source,
                &channel,
                crate::KernelEventKind::PushDeliveryChanged { push_id },
            );
        }
        Ok(())
    }

    /// 会话内的图片原件；id 可能来自外部，不属于该会话或不存在返回 `None`。
    pub async fn image(
        &self,
        session_id: SessionId,
        id: ImageId,
    ) -> Result<Option<ImageData>, KernelError> {
        Ok(self.store.image(session_id, id).await?)
    }

    pub async fn archive_session(&self, id: SessionId) -> Result<(), KernelError> {
        let session = self
            .store
            .session(id)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        if !matches!(session.kind, SessionKind::Root { .. }) {
            return Err(KernelError::SessionNotRoot);
        }
        self.store.archive_session(id, now_ms()).await?;
        self.titles.stop(id).await;
        self.engine.jobs.archive(id).await?;
        Ok(())
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

    pub async fn title_model(&self) -> Result<mic_store::TitleModelSelection, KernelError> {
        Ok(self.store.title_model().await?)
    }

    pub async fn set_title_model(
        &self,
        selection: mic_store::TitleModelSelection,
    ) -> Result<(), KernelError> {
        self.check_reasoning(selection.model_id, selection.reasoning_effort)
            .await?;
        Ok(self.store.set_title_model(selection).await?)
    }

    fn schedule_operations(&self) -> Result<&crate::schedule::Schedules, crate::ScheduleError> {
        self.engine
            .schedules
            .as_deref()
            .ok_or(crate::ScheduleError::Unavailable)
    }

    pub async fn schedules(&self) -> Result<Vec<crate::ScheduleView>, KernelError> {
        Ok(self.schedule_operations()?.list().await?)
    }

    pub async fn schedule_fires(
        &self,
        id: mic_store::ScheduleId,
        before: Option<mic_store::FireId>,
        limit: u32,
    ) -> Result<crate::ScheduleFirePage, KernelError> {
        Ok(self.schedule_operations()?.fires(id, before, limit).await?)
    }

    pub async fn update_schedule(
        &self,
        id: mic_store::ScheduleId,
        edit: crate::ScheduleUpdate,
    ) -> Result<crate::ScheduleView, KernelError> {
        let operations = self.schedule_operations()?;
        let schedule = operations.update(id, edit).await?;
        Ok(operations.view(schedule).await?)
    }

    pub async fn delete_schedule(&self, id: mic_store::ScheduleId) -> Result<(), KernelError> {
        Ok(self.schedule_operations()?.delete(id).await?)
    }

    /// 通知中心：新的在前。
    pub async fn pushes(
        &self,
        before: Option<PushId>,
        limit: u32,
    ) -> Result<PushPage, KernelError> {
        Ok(self.pushes.page(before, limit).await?)
    }

    /// 打开通知中心即视为看过：`through` 及更早的推送全部已读。
    pub async fn mark_pushes_read(&self, through: PushId) -> Result<(), KernelError> {
        Ok(self.pushes.mark_read(through).await?)
    }

    pub async fn push_settings(&self) -> Result<PushSettings, KernelError> {
        Ok(PushSettings {
            channels: self.pushes.channels(),
            chosen: self.store.push_channels().await?,
            effective: self.pushes.default_channels().await?,
        })
    }

    pub async fn set_push_channels(&self, channels: Vec<String>) -> Result<(), KernelError> {
        Ok(self.pushes.set_default_channels(channels).await?)
    }

    pub async fn title_generating(&self, session: SessionId) -> bool {
        self.titles.generating(session)
    }

    pub async fn generate_session_title(&self, session: SessionId) -> Result<(), KernelError> {
        self.store.ensure_session_active(session).await?;
        let session = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        if !matches!(session.kind, SessionKind::Root { .. }) {
            return Err(crate::TitleError::NotRoot.into());
        }
        let sources = self
            .store
            .title_sources(
                session.id,
                NonZeroU32::new(crate::limits::TITLE_INPUTS).unwrap(),
                crate::limits::TITLE_REPLIES,
            )
            .await?;
        if !sources
            .iter()
            .any(|m| matches!(m.body, MessageBody::UserInput { .. }))
        {
            return Err(crate::TitleError::EmptyInput.into());
        }
        self.titles.submit(session, sources)?;
        Ok(())
    }

    async fn maybe_title(&self, input: &Message) -> Result<(), KernelError> {
        let summary = self
            .store
            .session_summary(input.session_id)
            .await?
            .expect("input session exists");
        if !matches!(summary.session.kind, SessionKind::Root { .. }) || summary.title.is_some() {
            return Ok(());
        }
        let first = self.store.first_text_input(input.session_id).await?;
        let Some(first) = first.filter(|first| first.id == input.id) else {
            return Ok(());
        };
        let channel = session_channel(&self.store, &summary.session).await?;
        match self.titles.submit(summary.session, vec![first]) {
            Ok(()) | Err(crate::TitleError::Generating) => {}
            Err(error) => {
                self.store
                    .save_title_result(input.session_id, Err(error.to_string()), None)
                    .await?;
                self.events.emit(
                    input.session_id,
                    &channel,
                    crate::KernelEventKind::SessionTitleChanged,
                );
            }
        }
        Ok(())
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

    /// `before` 之前的一页，见 `Store::messages_page`。
    pub async fn messages_page(
        &self,
        session_id: SessionId,
        before: Option<MessageId>,
        rounds: NonZeroU32,
        cap: NonZeroU32,
    ) -> Result<MessagePage, KernelError> {
        Ok(self
            .store
            .messages_page(session_id, before, rounds, cap)
            .await?)
    }

    /// 该会话正在执行的 run。
    pub async fn executing_run(&self, session_id: SessionId) -> Result<Option<RunId>, KernelError> {
        Ok(self.store.executing_run(session_id).await?)
    }

    /// 内置 instructions 块（不可改），供设置页只读展示。
    pub fn base_prompt(&self) -> &'static str {
        request::base_prompt()
    }

    /// 该会话按当前配置下一次调用会用的系统提示。
    pub async fn system_prompt(&self, session: SessionId) -> Result<SystemPrompt, KernelError> {
        let s = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        self.current_system_prompt(&s).await
    }

    /// 该会话按当前配置发给模型的工具定义，顺序同请求。
    pub async fn session_tools(&self, session: SessionId) -> Result<Vec<ToolSpec>, KernelError> {
        let s = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        Ok(self.tools_of(&s))
    }

    fn tools_of(&self, s: &Session) -> Vec<ToolSpec> {
        self.engine
            .scoped_tools(s)
            .iter()
            .map(|t| t.spec().clone())
            .collect()
    }

    async fn current_system_prompt(&self, s: &Session) -> Result<SystemPrompt, KernelError> {
        let settings = self.store.run_settings(s.id).await?;
        let window = self.store.context_window(s.id).await?;
        Ok(request::system_prompt(
            &settings,
            s,
            self.engine.channel_prompt(s),
            &window,
        ))
    }

    /// 上次对话调用的上下文构成：消息与输出按调用记录，系统提示与工具按当前配置。
    /// 契约：docs/blueprints/context-breakdown.md。
    pub async fn context_breakdown(
        &self,
        session: SessionId,
    ) -> Result<Option<ContextBreakdown>, KernelError> {
        let s = self
            .store
            .session(session)
            .await?
            .ok_or(KernelError::SessionNotFound)?;
        let Some((occupancy, usage, images)) = compaction::usage(&self.engine, &s).await? else {
            return Ok(None);
        };
        let Some(record) = self.store.call_record(usage.call).await? else {
            return Ok(None);
        };
        let output = match record.reply {
            Some(id) => self.store.messages_by_ids(vec![id]).await?,
            None => Vec::new(),
        };
        let output: Vec<ReplyBlock> = output
            .into_iter()
            .flat_map(|m| match m.body {
                MessageBody::Reply { blocks, .. } => blocks,
                _ => Vec::new(),
            })
            .collect();
        let mut messages = self.input_messages(record).await?.unwrap_or_default();
        if !images {
            request::hide_images(&mut messages);
        }
        let tools = self.tools_of(&s);
        Ok(Some(context_usage::breakdown(
            occupancy,
            &usage,
            &self.current_system_prompt(&s).await?,
            &tools,
            &messages,
            &output,
        )))
    }

    /// 重建某次调用当时的输入消息；标题调用没有会话输入，返回 `None`。
    pub async fn call_input(&self, call: ModelCallId) -> Result<Option<Vec<Message>>, KernelError> {
        match self.store.call_record(call).await? {
            Some(record) => self.input_messages(record).await,
            None => Ok(None),
        }
    }

    async fn input_messages(
        &self,
        record: CallRecord,
    ) -> Result<Option<Vec<Message>>, KernelError> {
        if record.purpose == ModelCallPurpose::Title {
            let sources = record.context.instruction;
            if sources.is_empty() {
                return Ok(None);
            }
            let sources = self.store.messages_by_ids(sources).await?;
            return Ok(Some(crate::title::title_messages(
                record.session_id,
                &sources,
            )));
        }
        let context = record.context;
        let window = self
            .store
            .window_at(record.session_id, context.boundary_id, context.through)
            .await?
            .into_iter()
            .filter(|m| !context.excluded.contains(&m.id))
            .collect();
        let messages = match record.purpose {
            ModelCallPurpose::Compaction => {
                let mut messages = request::order(window);
                let keep_position = messages
                    .iter()
                    .find(|m| Some(m.id) == context.keep_from)
                    .map(|m| (m.created_at, m.id));
                let kept_replies = messages
                    .iter()
                    .filter(|m| matches!(m.body, MessageBody::Reply { .. }))
                    .filter(|m| keep_position.is_some_and(|k| (m.created_at, m.id) >= k))
                    .count();
                messages.drain(..context.skip as usize);
                let instruction = self.store.messages_by_ids(context.instruction).await?;
                let elide = self.store.messages_by_ids(context.elide).await?;
                messages.push(crate::compaction::instruction_note(
                    record.session_id,
                    crate::compaction::extra_text(&instruction),
                    kept_replies,
                    &elide,
                ));
                messages
            }
            _ => request::order(window),
        };
        Ok(Some(messages))
    }

    pub async fn session_usage(
        &self,
        session: SessionId,
    ) -> Result<mic_store::SessionUsageView, KernelError> {
        self.existing(session).await?;
        Ok(self.store.session_usage(session).await?)
    }

    pub async fn trace_sessions(&self) -> Result<Vec<TraceSession>, KernelError> {
        Ok(self.store.trace_sessions().await?)
    }

    pub async fn trace_runs(
        &self,
        session: SessionId,
        after: Option<RunId>,
        limit: u32,
    ) -> Result<TraceRunPage, KernelError> {
        self.existing(session).await?;
        Ok(self.store.trace_runs(session, after, limit).await?)
    }

    pub async fn message(&self, id: MessageId) -> Result<Option<Message>, KernelError> {
        Ok(self.store.message(id).await?)
    }

    pub async fn call_peers(&self) -> Result<Peers, KernelError> {
        Ok(self.store.call_peers().await?)
    }

    pub async fn dashboard(&self, days: u32) -> Result<Dashboard, KernelError> {
        Ok(self.store.dashboard(days).await?)
    }

    pub async fn settings(&self) -> Result<Settings, KernelError> {
        Ok(self.store.settings().await?)
    }

    /// 默认工作目录展开为 UTF-8 绝对路径，并确保存在。
    pub async fn default_workdir(&self) -> Result<String, KernelError> {
        let raw = self.store.settings().await?.default_workdir;
        let path = match raw.strip_prefix("~/") {
            Some(rest) => std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .map(|home| PathBuf::from(home).join(rest))
                .ok_or(WorkdirError::HomeMissing)?,
            None => PathBuf::from(raw),
        };
        let path = path
            .into_os_string()
            .into_string()
            .map_err(|path| WorkdirError::NonUtf8 { path: path.into() })?;
        tokio::fs::create_dir_all(&path)
            .await
            .map_err(|source| WorkdirError::Create {
                path: PathBuf::from(&path),
                source,
            })?;
        Ok(path)
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

    /// 临时明文：开启后新消息发给模型时不按规则脱敏，之前的仍脱敏；下一次模型调用生效。
    pub async fn set_session_plain(&self, session: SessionId, on: bool) -> Result<(), KernelError> {
        Ok(self.store.set_session_plain(session, on).await?)
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

    /// 模型目录与调用选择共用的思考档位来源。
    pub async fn model_reasoning(
        &self,
        model: &ModelView,
    ) -> Result<Option<crate::ReasoningOptions>, KernelError> {
        let endpoint = self
            .store
            .endpoint(model.endpoint_id)
            .await?
            .ok_or(mic_store::ModelSettingsError::EndpointNotFound)?;
        Ok(self
            .factories
            .get(&endpoint.kind)?
            .reasoning_options(&endpoint.config_json, &model.name)?)
    }

    async fn check_reasoning(
        &self,
        model: Option<ModelId>,
        effort: Option<ReasoningEffort>,
    ) -> Result<(), KernelError> {
        match model {
            Some(id) => {
                let model = self.store.selectable_model(id).await?;
                resolve_reasoning(self.model_reasoning(&model).await?, effort)?;
            }
            None if effort.is_some() => {
                return Err(crate::ConfigError {
                    field: "reasoning_effort".into(),
                    message: "未指定模型时不能指定思考档位".into(),
                }
                .into())
            }
            None => {}
        }
        Ok(())
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

    /// 会话下一轮用的模型；本轮已在认领时读定，不受影响。
    pub async fn set_session_model(
        &self,
        session: SessionId,
        model: ModelId,
        reasoning_effort: Option<ReasoningEffort>,
    ) -> Result<(), KernelError> {
        self.check_reasoning(Some(model), reasoning_effort).await?;
        Ok(self
            .store
            .set_session_model(session, model, reasoning_effort)
            .await?)
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
            context_window: draft.context_window,
            images: draft.images,
            endpoint_id: draft.endpoint_id,
            name: draft.name,
            config_json,
            price: draft.price,
        })
    }

    /// 订阅之后产生的事件（全部会话，按 `channel`/`session_id` 字段过滤）。
    pub fn subscribe(&self) -> EventReceiver {
        self.events.subscribe()
    }

    /// 订阅与历史切点同受发布锁保护；切点之后的稳定消息全部留在接收器中。
    pub async fn subscribe_from_now(
        &self,
        session: SessionId,
    ) -> Result<(EventReceiver, Option<MessageId>), KernelError> {
        let _publish = self.events.publisher().await;
        if self.store.session(session).await?.is_none() {
            return Err(KernelError::SessionNotFound);
        }
        let receiver = self.events.subscribe();
        let latest = self
            .store
            .messages_after(session, None)
            .await?
            .last()
            .map(|message| message.id);
        Ok((receiver, latest))
    }

    /// 只许访问本模块 `{name}_` 前缀的表；由模块单元测试把关。
    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, KernelError>
    where
        F: FnOnce(&Transaction<'_>) -> rusqlite::Result<R> + Send + 'static,
        R: Send + 'static,
    {
        Ok(self.store.with_module_tx(f).await?)
    }

    /// 开发者诊断：只读连接上的单条 SQL，凭据列读作 NULL。
    pub async fn diagnostic_query(&self, sql: String) -> Result<QueryResult, KernelError> {
        Ok(self.store.diagnostic_query(sql).await?)
    }

    pub async fn diagnostic_schema(&self) -> Result<Vec<TableSchema>, KernelError> {
        Ok(self.store.diagnostic_schema().await?)
    }
}

/// 写入已校验的用户输入并发 `MessageAppended`；`Kernel` 与 `-p` 共用。
/// 输入（及其说明）落盘后按 id 顺序发布；返回的首条为输入。
pub(crate) async fn append_input(
    store: &Store,
    events: &Events,
    session_id: SessionId,
    person: PersonId,
    parts: Vec<NewInputPart>,
    disposition: InputDisposition,
    notice: Option<NewNotice>,
) -> Result<Vec<Message>, StoreError> {
    let publish = events.publisher().await;
    // 会话不存在时由外键在写入处报错，之后必能读到。
    let messages = store
        .append_input(session_id, person, parts, disposition, notice, now_ms())
        .await?;
    let session = store
        .session(session_id)
        .await?
        .expect("刚写入消息的会话必然存在");
    let channel = session_channel(store, &session).await?;
    for message in &messages {
        publish.appended(session_id, &channel, message.clone());
    }
    Ok(messages)
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
            SessionKind::Task => {
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

/// 外部输入只进 Root/Triggered 会话；子 agent 只经 `agent` 工具追问。
fn reject_task(session: &Session) -> Result<(), KernelError> {
    match session.kind {
        SessionKind::Task => Err(KernelError::TaskSessionInput),
        _ => Ok(()),
    }
}
