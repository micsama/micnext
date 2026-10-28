//! 一个 run 的执行。契约：docs/blueprints/run-execution.md §4.3～4.5、§4.7、§4.8。

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::Duration;

use mic_message::{
    ExecFailureKind, ExecOutcome, ImageData, ImageId, MessageBody, MessageId, ReplyBlock,
    SessionCommand, SessionId, ToolCallRef, ToolResultOutcome,
};
use mic_store::{
    ClaimedModel, ClaimedRun, ContextCommandError, ContextWindow, ModelCallContext,
    ModelCallOutcome, ModelCallPurpose, ModelId, ReasoningEffort, Run, RunId, RunInput,
    RunSettings, RunState, Session, SessionKind, Store, StoreError, ToolScope,
};
use mic_tool::{ToolContext, ToolHandle, ToolOutput};
use tokio::task::JoinSet;
use tokio_util::sync::CancellationToken;

use crate::ask::{ASK_USER, NOT_ALONE};
use crate::attempt::{stream_attempt, Caller, Facts};
use crate::control::{RunControl, Slot};
use crate::event::{Events, KernelEventKind};
use crate::jobs::{self, JobPort, Jobs, Origin};
use crate::limits::{
    AGENT_MAX_TURNS, AGENT_RUN_TIMEOUT, MAX_MODEL_ATTEMPTS, MAX_RETRY_WAIT, MAX_TURNS_WARN_PERCENT,
    RETRY_BASE, SCHEDULE_RUN_TIMEOUT,
};
use crate::provider::{BuiltProvider, Factories};
use crate::schedule::Schedules;
use crate::skills::{Skills, SkillsError};
use crate::{request, ModelEvent, ModelResponse, Provider, ProviderError, StopReason};

/// 框架通知的 `source`。
pub(crate) const NOTIFICATION_SOURCE: &str = "micnext";

const STOPPED_TOOL: &str =
    "Stopped by the user before this tool call finished; its side effects are unknown.";

/// 执行主路径的共享部分；调度与 `-p` 共用。
pub(crate) struct Engine {
    pub(crate) store: Store,
    pub(crate) events: Events,
    pub(crate) factories: Arc<Factories>,
    pub(crate) tools: Vec<ToolHandle>,
    pub(crate) jobs: Arc<Jobs>,
    /// 渠道名 → 渠道提示；只用于 Root 会话。
    pub(crate) channel_prompts: Arc<HashMap<&'static str, &'static str>>,
    /// 会话执行槽位与停止令牌，与 `jobs` 共用。
    pub(crate) control: Arc<RunControl>,
    /// 常驻时才有；`-p` 不建定时任务。
    pub(crate) schedules: Option<Arc<Schedules>>,
    pub(crate) skills: Skills,
}

impl Engine {
    /// 停该会话执行中的 run（给定 `run` 则只停它）；没有匹配返回 false。
    pub(crate) fn stop(&self, session: SessionId, run: Option<RunId>) -> bool {
        self.control.stop(session, run)
    }

    /// 会话可用的工具：登记工具加绑定本会话的 core 工具，按工具范围过滤。
    /// 任务会话不能派后台任务，只有登记工具。
    pub(crate) fn scoped_tools(&self, session: &Session) -> Vec<ToolHandle> {
        let core = match &session.kind {
            SessionKind::Root { .. } => {
                let mut tools = self.jobs.tools(session);
                tools.push(crate::ask::handle());
                if let Some(schedules) = &self.schedules {
                    tools.extend(schedules.tools(session.id));
                }
                tools
            }
            SessionKind::Task => self.jobs.tools(session),
            SessionKind::Triggered { .. } => Vec::new(),
        };
        let all = self.tools.iter().cloned().chain(core);
        match &session.tool_scope {
            ToolScope::All => all.collect(),
            ToolScope::Only(names) => all.filter(|t| names.contains(t.name())).collect(),
        }
    }

    /// 开放了 `skill` 工具时按磁盘现状生成 Skill 目录。
    pub(crate) async fn skill_catalog(
        &self,
        tools: &[ToolHandle],
    ) -> Result<Option<String>, SkillsError> {
        if !tools.iter().any(|t| t.name() == crate::skills::SKILL) {
            return Ok(None);
        }
        Ok(self.skills.catalog(&self.skills.list().await?))
    }

    /// Root 按渠道、任务会话按触发模块取渠道说明。
    pub(crate) fn channel_prompt(&self, session: &Session) -> Option<&'static str> {
        let key = match &session.kind {
            SessionKind::Root { channel, .. } => channel,
            SessionKind::Triggered { module, .. } => module,
            SessionKind::Task => return None,
        };
        self.channel_prompts.get(key.as_str()).copied()
    }

    /// 以认领时定下的设置与模型执行一个 run 并落盘终态；模型不可用时以 ProviderFailed 收尾。
    /// 子 agent 的 run 另有轮次与墙钟上限，收尾时向父会话交付。
    pub(crate) async fn run(
        &self,
        session: &Session,
        channel: &str,
        claimed: ClaimedRun,
        slot: &Slot,
    ) -> Result<RunState, StoreError> {
        let ClaimedRun {
            run,
            mut settings,
            model,
            input,
        } = claimed;
        slot.set_run(run.id);
        let stop = slot.token();
        let parent = match session.kind {
            SessionKind::Task => Some(session.parent_session_id.expect("Task 会话必有父会话")),
            _ => None,
        };
        let tree = match parent {
            Some(_) => Some(jobs::tree(&self.store.session_root(session.id).await?)),
            None => None,
        };
        let limit = match session.kind {
            SessionKind::Task => Some(AGENT_RUN_TIMEOUT),
            SessionKind::Triggered { .. } => Some(SCHEDULE_RUN_TIMEOUT),
            SessionKind::Root { .. } => None,
        };
        let timed_out = Arc::new(AtomicBool::new(false));
        let timer = limit.map(|limit| {
            settings.max_turns = settings.max_turns.min(AGENT_MAX_TURNS);
            let (stop, timed_out) = (stop.clone(), timed_out.clone());
            tokio::spawn(async move {
                tokio::time::sleep(limit).await;
                timed_out.store(true, Ordering::Relaxed);
                stop.cancel();
            })
        });
        let inputs = match &input {
            RunInput::Conversation { message_ids } => message_ids.clone(),
            RunInput::Command { message_id, .. } => vec![*message_id],
        };
        self.events.emit(
            session.id,
            channel,
            KernelEventKind::InputsClaimed {
                run_id: run.id,
                inputs,
            },
        );
        if let (SessionKind::Triggered { .. }, Some(schedules)) = (&session.kind, &self.schedules) {
            if let Some(fire) = self.store.fire_of_run(run.id).await? {
                if let Some(schedule) = self.store.schedule(fire.schedule_id).await? {
                    schedules.publish_changed(&schedule);
                }
            }
        }
        if let Some(tree) = &tree {
            self.jobs.changed(tree);
        }
        tracing::info!(session = session.id.0, run = run.id.0, "run started");
        let tools = self.scoped_tools(session);
        let (model_id, context_window, images) = match &model {
            ClaimedModel::Selected(m) => (
                Some(m.id),
                m.context_window
                    .map(|v| v.get())
                    .unwrap_or(crate::limits::DEFAULT_CONTEXT_WINDOW),
                m.images,
            ),
            _ => (None, crate::limits::DEFAULT_CONTEXT_WINDOW, true),
        };
        let pending_inputs = match &input {
            RunInput::Conversation { message_ids } => message_ids.clone(),
            RunInput::Command {
                message_id,
                command: SessionCommand::Compact { text: Some(_) },
            } => vec![*message_id],
            _ => Vec::new(),
        };
        let mut exec = Exec {
            engine: self,
            session,
            channel,
            run: &run,
            settings: &settings,
            tools,
            pending_inputs,
            model_id,
            effort: None,
            skills: None,
            context_window,
            images,
            stop,
        };
        let state = match input {
            RunInput::Command {
                message_id,
                command: SessionCommand::Clear,
            } => {
                let publish = self.events.publisher().await;
                for message in self
                    .store
                    .clear_context(session.id, run.id, message_id, now_ms())
                    .await?
                {
                    publish.appended(session.id, channel, message);
                }
                RunState::Completed
            }
            RunInput::Command {
                command: SessionCommand::RevokeCompaction { boundary_id },
                ..
            } => {
                let publish = self.events.publisher().await;
                let result = self
                    .store
                    .revoke_compaction(session.id, run.id, boundary_id, now_ms())
                    .await;
                match result {
                    Ok(messages) => {
                        for message in messages {
                            publish.appended(session.id, channel, message);
                        }
                    }
                    Err(ContextCommandError::Store(error)) => return Err(error),
                    Err(error) => {
                        drop(publish);
                        exec.notify(format!("未撤回：{error}。")).await?;
                    }
                }
                RunState::Completed
            }
            input => {
                // 模型不可用与 skill 目录读不出都让 run 无法开始，同样以 ProviderFailed 收尾。
                let prepared = match self.provider(model) {
                    Ok(built) => match self.skill_catalog(&exec.tools).await {
                        Ok(skills) => Ok((built, skills)),
                        Err(error) => Err(format!("{error}。")),
                    },
                    Err(text) => Err(text),
                };
                match prepared {
                    Ok((BuiltProvider { provider, effort }, skills)) => {
                        exec.effort = effort;
                        exec.skills = skills;
                        match input {
                            RunInput::Command {
                                command: SessionCommand::Compact { text },
                                ..
                            } => match exec.compact(provider.as_ref()).await? {
                                Err(failed) => failed,
                                Ok(()) if text.is_some() => exec.execute(provider.as_ref()).await?,
                                Ok(()) => RunState::Completed,
                            },
                            RunInput::Conversation { .. } => {
                                match exec.tidy_if_idle(provider.as_ref()).await? {
                                    Ok(()) => exec.execute(provider.as_ref()).await?,
                                    Err(failed) => failed,
                                }
                            }
                            _ => unreachable!("非模型指令已处理"),
                        }
                    }
                    Err(text) => {
                        tracing::warn!(session = session.id.0, run = run.id.0, %text, "run cannot start");
                        exec.notify(text).await?;
                        RunState::ProviderFailed
                    }
                }
            }
        };
        if let Some(timer) = timer {
            timer.abort();
        }
        let state = match state {
            RunState::Stopped if timed_out.load(Ordering::Relaxed) => RunState::TimedOut,
            state => state,
        };
        match state {
            RunState::Stopped => exec.notify("已停止。".into()).await?,
            RunState::TimedOut => {
                let (what, limit) = match parent {
                    Some(_) => ("子任务", AGENT_RUN_TIMEOUT),
                    None => ("定时任务", SCHEDULE_RUN_TIMEOUT),
                };
                exec.notify(format!(
                    "{what}执行超过 {} 分钟，已停止。",
                    limit.as_secs() / 60
                ))
                .await?
            }
            _ => {}
        }
        let report = match parent {
            Some(parent) => jobs::delivery(
                state,
                &self.store.run_messages(run.id).await?,
                self.control.closing(session.id),
                parent,
                session.id,
            ),
            None => None,
        };
        let delivered = {
            let publish = self.events.publisher().await;
            let delivered = self
                .store
                .finish_run(run.id, state, report, now_ms())
                .await?;
            if let (Some(message), Some(tree)) = (&delivered, &tree) {
                publish.appended(message.session_id, &tree.channel, message.clone());
            }
            delivered
        };
        tracing::info!(
            session = session.id.0,
            run = run.id.0,
            ?state,
            "run finished"
        );
        let resume = self.store.resumable_run(session.id).await?;
        self.events.emit(
            session.id,
            channel,
            KernelEventKind::RunFinished {
                run_id: run.id,
                state,
                resume,
            },
        );
        if let Some(tree) = &tree {
            self.jobs.changed(tree);
        }
        if let (Some(message), Some(parent)) = (delivered, parent) {
            debug_assert_eq!(message.session_id, parent);
            self.jobs.wake(parent).await?;
        }
        if let (SessionKind::Triggered { .. }, Some(schedules)) = (&session.kind, &self.schedules) {
            schedules.settled(run.id).await?;
        }
        Ok(state)
    }
}

impl Engine {
    /// 本轮 Provider 实例；失败文案面向用户，说明怎么修。
    fn provider(&self, model: ClaimedModel) -> Result<BuiltProvider, String> {
        match model {
            ClaimedModel::Missing => {
                Err("还没有可用的模型。请在网页 设置 → 模型 添加一个，然后重新发送。".into())
            }
            ClaimedModel::Deleted { name, .. } => Err(format!(
                "所选模型「{name}」已被删除。请在会话里重新选择模型后重新发送。"
            )),
            ClaimedModel::Selected(m) => {
                let name = m.name.clone();
                self.factories.build(m).map_err(|e| {
                    format!("模型「{name}」的调用配置有误（{e}）。请检查会话思考档位与模型设置后重新发送。")
                })
            }
        }
    }
}

pub(crate) struct Exec<'a> {
    pub(crate) engine: &'a Engine,
    pub(crate) session: &'a Session,
    pub(crate) channel: &'a str,
    pub(crate) run: &'a Run,
    pub(crate) settings: &'a RunSettings,
    pub(crate) tools: Vec<ToolHandle>,
    pub(crate) pending_inputs: Vec<MessageId>,
    pub(crate) model_id: Option<ModelId>,
    /// 实际生效的思考档位（已解析默认值）；Provider 建好后才有。
    pub(crate) effort: Option<ReasoningEffort>,
    /// run 开始时扫描的 Skill 目录，本 run 内各次请求共用。
    pub(crate) skills: Option<String>,
    pub(crate) context_window: u32,
    /// 模型是否支持图片；否则请求里的图片换成文字占位。
    pub(crate) images: bool,
    /// 用户停止：只在等待外部处响应，取消后不再认领输入、不再发起调用或工具。
    pub(crate) stop: CancellationToken,
}

struct Call {
    reference: ToolCallRef,
    id: String,
    name: String,
    args: serde_json::Value,
}

impl Exec<'_> {
    async fn execute(&mut self, provider: &dyn Provider) -> Result<RunState, StoreError> {
        let tools = self.tools.clone();
        let max_turns = self.settings.max_turns;
        let warn_at = (max_turns * MAX_TURNS_WARN_PERCENT / 100).max(1);
        let mut calls = 0;
        loop {
            if self.stop.is_cancelled() {
                return Ok(RunState::Stopped);
            }
            self.absorb().await?;
            if calls == max_turns {
                self.note(format!(
                    "You have used all {max_turns} model calls allowed for this request, and tools are no longer available. Summarize what you have done and what remains unfinished, and tell the user they can reply to continue."
                ))
                .await?;
                if let Err(failed) = self.call_model(provider, &[]).await? {
                    return Ok(failed);
                }
                return Ok(match self.compact_if_full(provider).await? {
                    Ok(()) => RunState::MaxTurns,
                    Err(failed) => failed,
                });
            }
            if calls == warn_at && warn_at < max_turns {
                self.note(format!(
                    "You have used {calls} of {max_turns} model calls allowed for this request. Wrap up soon: finish the essential steps and report to the user."
                ))
                .await?;
            }
            calls += 1;
            let tool_calls = match self.call_model(provider, &tools).await? {
                Ok(tool_calls) => tool_calls,
                Err(failed) => return Ok(failed),
            };
            // 先闭合本步工具调用再压缩，压缩失败或停止也不留无结果的调用。
            let has_tools = !tool_calls.is_empty();
            let asked = has_tools && self.run_tools(tool_calls).await?;
            if let Err(failed) = self.compact_if_full(provider).await? {
                return Ok(failed);
            }
            if asked {
                return Ok(RunState::Completed);
            }
            if has_tools {
                continue;
            }
            if self.absorb().await? {
                continue;
            }
            return Ok(if self.stop.is_cancelled() {
                RunState::Stopped
            } else {
                RunState::Completed
            });
        }
    }

    /// 把已到达的新输入并入本 run；有并入返回 true。已停止则不认领。
    async fn absorb(&mut self) -> Result<bool, StoreError> {
        if self.stop.is_cancelled() {
            return Ok(false);
        }
        let inputs = self.engine.store.absorb(self.run.id).await?;
        if inputs.is_empty() {
            return Ok(false);
        }
        self.pending_inputs.extend(&inputs);
        self.engine.events.emit(
            self.session.id,
            self.channel,
            KernelEventKind::InputsClaimed {
                run_id: self.run.id,
                inputs,
            },
        );
        Ok(true)
    }

    /// `tools` 为空即不开放工具。每次尝试落盘调用行，成功时连同 `Reply`；返回其中的工具调用。
    /// 失败已落盘通知，返回终态。
    async fn call_model(
        &mut self,
        provider: &dyn Provider,
        tools: &[ToolHandle],
    ) -> Result<Result<Vec<Call>, RunState>, StoreError> {
        let context_at = now_ms();
        let mut window = self.engine.store.context_window(self.session.id).await?;
        let context = ModelCallContext {
            boundary_id: window.boundary_id,
            through: window.messages.iter().map(|m| m.id).max(),
            excluded: window.filtered.clone(),
            ..Default::default()
        };
        let images = self.window_images(&mut window).await?;
        let channel_prompt = self.engine.channel_prompt(self.session);
        let req = request::build(
            self.session,
            window,
            images,
            channel_prompt,
            self.skills.as_deref(),
            tools,
            self.settings,
        );
        let caller = self.caller(provider);
        let mut attempt = 1;
        loop {
            let mut facts = Facts::start();
            facts.context_at = context_at;
            let mut partial = String::new();
            let streamed = tokio::select! {
                result = stream_attempt(provider, req.clone(), &mut facts, |event| {
                    if let ModelEvent::TextDelta(text) = event {
                        partial.push_str(text);
                    }
                    self.forward(event)
                }) => Some(result),
                () = self.stop.cancelled() => None,
            };
            let Some(streamed) = streamed else {
                facts.finished_at = now_ms();
                facts.usage = None;
                self.pending_inputs.clear();
                let blocks = if partial.is_empty() {
                    Vec::new()
                } else {
                    vec![ReplyBlock::Text {
                        text: partial,
                        phase: None,
                    }]
                };
                self.record(
                    &caller,
                    &facts,
                    Some(StopReason::Stopped),
                    ModelCallOutcome::Replied { blocks },
                    context,
                )
                .await?;
                return Ok(Err(RunState::Stopped));
            };
            let error = match streamed {
                Ok(reply) => {
                    self.pending_inputs.clear();
                    return self
                        .persist_reply(&caller, &facts, reply, context)
                        .await
                        .map(Ok);
                }
                Err(e) => e,
            };
            self.record(
                &caller,
                &facts,
                None,
                ModelCallOutcome::Failed {
                    error: error.to_string(),
                },
                context.clone(),
            )
            .await?;
            match self.retry_wait("conversation", provider, &error, attempt) {
                Some(wait) => {
                    if self.sleep(wait).await {
                        return Ok(Err(RunState::Stopped));
                    }
                    attempt += 1;
                }
                None => {
                    self.notify(provider_failure_text(&error, attempt)).await?;
                    return Ok(Err(RunState::ProviderFailed));
                }
            }
        }
    }

    /// 请求要带的图片数据；模型不支持图片时先把窗口里的图片换成占位。
    pub(crate) async fn window_images(
        &self,
        window: &mut ContextWindow,
    ) -> Result<HashMap<ImageId, ImageData>, StoreError> {
        if !self.images {
            request::hide_images(&mut window.messages);
        }
        self.engine
            .store
            .images(self.session.id, request::image_ids(window))
            .await
    }

    pub(crate) fn caller(&self, provider: &dyn Provider) -> Caller {
        Caller::new(
            self.session.id,
            Some(self.run.id),
            provider,
            self.model_id,
            self.effort,
        )
    }

    /// 把流上的正文与推理增量转成内核事件。
    fn forward(&self, event: &ModelEvent) {
        match event {
            ModelEvent::TextDelta(text) => self.emit(KernelEventKind::TextDelta(text.clone())),
            ModelEvent::ReasoningDelta(text) => {
                self.emit(KernelEventKind::ReasoningDelta(text.clone()))
            }
            _ => {}
        }
    }

    /// 落盘调用行与 `Reply`，结束草稿，截断时通知；返回其中的工具调用。
    async fn persist_reply(
        &self,
        caller: &Caller,
        facts: &Facts,
        reply: ModelResponse,
        context: ModelCallContext,
    ) -> Result<Vec<Call>, StoreError> {
        let calls = reply
            .blocks
            .iter()
            .enumerate()
            .filter_map(|(index, b)| match b {
                ReplyBlock::ToolCall { id, name, args } => {
                    Some((index as u32, id.clone(), name.clone(), args.clone()))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        let stop = reply.stop;
        let outcome = ModelCallOutcome::Replied {
            blocks: reply.blocks,
        };
        let message = self
            .record(caller, facts, Some(stop), outcome, context)
            .await?;
        let calls = calls
            .into_iter()
            .map(|(index, id, name, args)| Call {
                reference: ToolCallRef {
                    reply: message.expect("含工具的回复已落盘"),
                    index,
                },
                id,
                name,
                args,
            })
            .collect();
        match stop {
            StopReason::MaxTokens => {
                self.notify("回复达到输出长度上限，内容不完整。".into())
                    .await?
            }
            StopReason::ContentFilter => {
                self.notify("回复被上游内容审核截断，内容不完整。".into())
                    .await?
            }
            StopReason::EndTurn | StopReason::ToolUse | StopReason::Stopped => {}
        }
        Ok(calls)
    }

    /// 落盘调用行并结束草稿：有 `Reply` 即发布它，否则作废。
    async fn record(
        &self,
        caller: &Caller,
        facts: &Facts,
        stop: Option<StopReason>,
        outcome: ModelCallOutcome,
        context: ModelCallContext,
    ) -> Result<Option<MessageId>, StoreError> {
        let call = facts.row(
            caller,
            ModelCallPurpose::Conversation,
            context,
            stop,
            outcome,
        );
        let publish = self.engine.events.publisher().await;
        let (_, reply) = self.engine.store.record_model_call(call).await?;
        let id = reply.as_ref().map(|m| m.id);
        match reply {
            Some(message) => publish.appended(self.session.id, self.channel, message),
            None => self.emit(KernelEventKind::DraftDiscarded),
        }
        Ok(id)
    }

    /// 本批调用并行执行，结果按返回顺序由本 run 落盘，故消息 id 只表示落盘顺序。
    /// 停止时中止未返回的任务（shell 随之杀进程组）：已登记后台任务的记 `Dispatched`，其余补记已取消。
    /// 同批有 `ask_user` 时其余调用不执行；返回提问是否成功。
    async fn run_tools(&self, calls: Vec<Call>) -> Result<bool, StoreError> {
        let mut running = JoinSet::new();
        let mut started = HashMap::new();
        let asking = calls.iter().any(|c| c.name == ASK_USER);
        let mut asked = false;
        for call in calls {
            let rejected = asking && call.name != ASK_USER;
            let handle = self.tools.iter().find(|t| t.name() == call.name).cloned();
            let port = Arc::new(JobPort::new(
                self.engine.jobs.clone(),
                !matches!(self.session.kind, SessionKind::Triggered { .. }),
                Origin {
                    session: self.session.id,
                    run: self.run.id,
                    call: call.reference,
                    tool_name: call.name.clone(),
                },
            ));
            let ctx = ToolContext::new(PathBuf::from(&self.session.pwd), port.clone());
            let running_ctx = ctx.clone();
            let (name, args) = (call.name.clone(), call.args.clone());
            let started_at = now_ms();
            let task = running.spawn(async move {
                if rejected {
                    return ToolResultOutcome::Terminal(ExecOutcome::Failed {
                        kind: ExecFailureKind::Business,
                        message: NOT_ALONE.into(),
                    });
                }
                match handle {
                    None => ToolResultOutcome::Terminal(ExecOutcome::Failed {
                        kind: ExecFailureKind::Input,
                        message: format!("unknown tool `{name}`"),
                    }),
                    Some(tool) => match tool.invoke(&args, &running_ctx).await {
                        Ok(ToolOutput::Terminal(outcome)) => ToolResultOutcome::Terminal(outcome),
                        Ok(ToolOutput::Dispatched { exec_id }) => {
                            ToolResultOutcome::Dispatched { exec_id }
                        }
                        Err(e) => ToolResultOutcome::Terminal(ExecOutcome::Failed {
                            kind: e.kind,
                            message: e.message,
                        }),
                    },
                }
            });
            started.insert(task.id(), (call, started_at, port, ctx));
        }
        let mut stopping = false;
        let result = async {
            loop {
                let joined = tokio::select! {
                    joined = running.join_next_with_id() => match joined {
                        Some(joined) => joined,
                        None => return Ok(()),
                    },
                    () = self.stop.cancelled(), if !stopping => {
                        stopping = true;
                        for (_, _, _, ctx) in started.values() { ctx.cancel(); }
                        running.abort_all();
                        continue;
                    }
                };
                let (id, outcome) = match joined {
                    Ok(done) => done,
                    Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
                    Err(e) => {
                        let (_, _, port, ctx) = &started[&e.id()];
                        ctx.wait_commits().await;
                        let outcome = match port.registered().await {
                            Some(job) => ToolResultOutcome::Dispatched {
                                exec_id: job.0.to_string(),
                            },
                            None => ToolResultOutcome::Terminal(ExecOutcome::Cancelled {
                                message: STOPPED_TOOL.into(),
                            }),
                        };
                        (e.id(), outcome)
                    }
                };
                let returned_at = now_ms();
                let (call, started_at, _, _) = started.remove(&id).expect("任务登记过");
                if let ToolResultOutcome::Terminal(ExecOutcome::Failed {
                    kind: ExecFailureKind::Dependency,
                    message,
                }) = &outcome
                {
                    tracing::warn!(
                        session = self.session.id.0,
                        run = self.run.id.0,
                        tool = call.name,
                        error = message,
                        "tool dependency failed"
                    );
                }
                asked |= call.name == ASK_USER
                    && matches!(
                        outcome,
                        ToolResultOutcome::Terminal(ExecOutcome::Completed { .. })
                    );
                let publish = self.engine.events.publisher().await;
                let message = self
                    .engine
                    .store
                    .append(
                        self.session.id,
                        Some(self.run.id),
                        MessageBody::ToolResult {
                            call: call.reference,
                            tool_name: call.name,
                            tool_call_id: call.id,
                            outcome,
                            started_at: Some(started_at),
                        },
                        returned_at,
                    )
                    .await?;
                publish.appended(self.session.id, self.channel, message);
            }
        }
        .await;
        for (_, _, _, ctx) in started.values() {
            ctx.cancel();
        }
        running.abort_all();
        let mut panic = None;
        while let Some(joined) = running.join_next().await {
            match joined {
                Err(error) if error.is_panic() => panic = Some(error.into_panic()),
                _ => {}
            }
        }
        for (_, _, _, ctx) in started.values() {
            ctx.wait_commits().await;
        }
        if let Some(panic) = panic {
            std::panic::resume_unwind(panic);
        }
        result.map(|()| asked)
    }

    /// 重试前的等待；被停止返回 true。
    pub(crate) async fn sleep(&self, wait: Duration) -> bool {
        tokio::select! {
            () = tokio::time::sleep(wait) => false,
            () = self.stop.cancelled() => true,
        }
    }

    /// 模型调用失败的处置日志与重试判定：返回重试前的等待，`None` 即放弃。
    pub(crate) fn retry_wait(
        &self,
        purpose: &'static str,
        provider: &dyn Provider,
        error: &ProviderError,
        attempt: u32,
    ) -> Option<Duration> {
        let wait = match error {
            ProviderError::Transient { retry_after, .. } if attempt < MAX_MODEL_ATTEMPTS => Some(
                retry_after
                    .unwrap_or(RETRY_BASE * 2u32.pow(attempt - 1))
                    .min(MAX_RETRY_WAIT),
            ),
            _ => None,
        };
        tracing::warn!(
            session = self.session.id.0,
            run = self.run.id.0,
            purpose,
            model = provider.model(),
            attempt,
            wait = wait.map(|w| format!("{}s", w.as_secs())),
            %error,
            "model call failed"
        );
        if let Some(wait) = wait {
            self.emit(KernelEventKind::ModelRetrying {
                attempt,
                delay_ms: wait.as_millis() as u64,
            });
        }
        wait
    }

    /// 给模型看的框架备注。
    async fn note(&self, text: String) -> Result<(), StoreError> {
        self.append(MessageBody::HarnessNote { text }).await
    }

    /// 投递给用户的框架通知。
    pub(crate) async fn notify(&self, text: String) -> Result<(), StoreError> {
        self.append(MessageBody::Notification {
            source: NOTIFICATION_SOURCE.into(),
            text,
            files: Vec::new(),
        })
        .await
    }

    /// 落盘一条本 run 的产出并发 `MessageAppended`。
    async fn append(&self, body: MessageBody) -> Result<(), StoreError> {
        let publish = self.engine.events.publisher().await;
        let message = self
            .engine
            .store
            .append(self.session.id, Some(self.run.id), body, now_ms())
            .await?;
        publish.appended(self.session.id, self.channel, message);
        Ok(())
    }

    fn emit(&self, kind: KernelEventKind) {
        self.engine.events.emit(self.session.id, self.channel, kind);
    }
}

fn provider_failure_text(error: &ProviderError, attempts: u32) -> String {
    match error {
        ProviderError::Account { .. } => format!("{error}。请检查模型 key、权限或余额后重新发送。"),
        ProviderError::Rejected { .. } => {
            format!("{error}。重试不会成功，可以调整请求内容，或开一个新会话。")
        }
        ProviderError::Transient { .. } => {
            format!("{error}。已尝试 {attempts} 次，请稍后重新发送。")
        }
        ProviderError::Stalled { .. } => format!("{error}。请稍后重新发送，或换一个模型。"),
        ProviderError::Protocol { .. } => {
            format!("{error}。这是 micnext 与模型服务不匹配的问题，需要修复。")
        }
    }
}

pub(crate) fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_millis() as i64
}

#[cfg(test)]
mod tests;
