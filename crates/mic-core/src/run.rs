//! 一个 run 的执行。契约：docs/blueprints/run-execution.md §4.3～4.5、§4.7、§4.8。

use std::collections::HashMap;
use std::future::poll_fn;
use std::path::PathBuf;
use std::sync::Arc;

use mic_message::{
    ExecFailureKind, ExecOutcome, MessageBody, MessageId, ReplyBlock, SessionCommand,
    ToolResultOutcome,
};
use mic_store::{
    ClaimedModel, ClaimedRun, ContextCommandError, ModelCallContext, ModelCallOutcome, ModelId,
    NewModelCall, Run, RunInput, RunSettings, RunState, Session, SessionKind, Store, StoreError,
    ToolScope,
};
use mic_tool::{ToolContext, ToolHandle};
use tokio::task::JoinSet;

use crate::event::{Events, KernelEventKind};
use crate::limits::{MAX_MODEL_ATTEMPTS, MAX_RETRY_WAIT, MAX_TURNS_WARN_PERCENT, RETRY_BASE};
use crate::provider::{resolve_key, resolve_reasoning, Factories};
use crate::{
    request, ModelEvent, ModelRequest, ModelResponse, Provider, ProviderError, StopReason,
};

/// 框架通知的 `source`。
pub(crate) const NOTIFICATION_SOURCE: &str = "micnext";

/// 执行主路径的共享部分；调度与 `-p` 共用。
pub(crate) struct Engine {
    pub(crate) store: Store,
    pub(crate) events: Events,
    pub(crate) factories: Arc<Factories>,
    pub(crate) tools: Vec<ToolHandle>,
    /// 渠道名 → 渠道提示；只用于 Root 会话。
    pub(crate) channel_prompts: Arc<HashMap<&'static str, &'static str>>,
}

impl Engine {
    /// 以认领时定下的设置与模型执行一个 run 并落盘终态；模型不可用时以 ProviderFailed 收尾。
    pub(crate) async fn run(
        &self,
        session: &Session,
        channel: &str,
        claimed: ClaimedRun,
    ) -> Result<RunState, StoreError> {
        let ClaimedRun {
            run,
            settings,
            model,
            input,
        } = claimed;
        self.events.emit(
            session.id,
            channel,
            KernelEventKind::RunStarted { run_id: run.id },
        );
        tracing::info!(session = session.id.0, run = run.id.0, "run started");
        let tools = match &session.tool_scope {
            ToolScope::All => self.tools.clone(),
            ToolScope::Only(names) => self
                .tools
                .iter()
                .filter(|t| names.contains(t.name()))
                .cloned()
                .collect(),
        };
        let (model_id, context_window) = match &model {
            ClaimedModel::Selected {
                id, context_window, ..
            } => (
                Some(*id),
                context_window
                    .map(|v| v.get())
                    .unwrap_or(crate::limits::DEFAULT_CONTEXT_WINDOW),
            ),
            _ => (None, crate::limits::DEFAULT_CONTEXT_WINDOW),
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
            context_window,
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
                        exec.notify(format!("未撤回压缩：{error}。")).await?;
                    }
                }
                RunState::Completed
            }
            input => match self.provider(model) {
                Ok(provider) => match input {
                    RunInput::Command {
                        command: SessionCommand::Compact { text },
                        ..
                    } => match exec.compact(provider.as_ref(), text.clone()).await? {
                        Err(failed) => failed,
                        Ok(()) if text.is_some() => exec.execute(provider.as_ref()).await?,
                        Ok(()) => RunState::Completed,
                    },
                    RunInput::Conversation { .. } => exec.execute(provider.as_ref()).await?,
                    _ => unreachable!("非模型指令已处理"),
                },
                Err(text) => {
                    tracing::warn!(session = session.id.0, run = run.id.0, %text, "model unavailable");
                    exec.notify(text).await?;
                    RunState::ProviderFailed
                }
            },
        };
        self.store.finish_run(run.id, state, now_ms()).await?;
        tracing::info!(
            session = session.id.0,
            run = run.id.0,
            ?state,
            "run finished"
        );
        self.events.emit(
            session.id,
            channel,
            KernelEventKind::RunFinished {
                run_id: run.id,
                state,
            },
        );
        Ok(state)
    }
}

impl Engine {
    /// 本轮 Provider 实例；失败文案面向用户，说明怎么修。
    fn provider(&self, model: ClaimedModel) -> Result<Arc<dyn Provider>, String> {
        match model {
            ClaimedModel::Missing => {
                Err("还没有可用的模型。请在网页 设置 → 模型 添加一个，然后重新发送。".into())
            }
            ClaimedModel::Deleted { name, .. } => Err(format!(
                "所选模型「{name}」已被删除。请在会话里重新选择模型后重新发送。"
            )),
            ClaimedModel::Selected {
                name,
                kind,
                endpoint_json,
                model_name,
                model_json,
                reasoning_effort,
                key,
                ..
            } => self
                .factories
                .get(&kind)
                .and_then(|f| {
                    let key = resolve_key(key, f.key_env(&endpoint_json));
                    let effort = resolve_reasoning(
                        f.reasoning_options(&endpoint_json, &model_name)?,
                        reasoning_effort,
                    )?;
                    f.build(&endpoint_json, key, &model_name, &model_json, effort)
                })
                .map_err(|e| {
                    format!("模型「{name}」的调用配置有误（{e}）。请检查会话思考档位与模型设置后重新发送。")
                }),
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
    pub(crate) context_window: u32,
}

struct Call {
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
            self.absorb().await?;
            if calls == max_turns {
                self.note(format!(
                    "You have used all {max_turns} model calls allowed for this request, and tools are no longer available. Summarize what you have done and what remains unfinished, and tell the user they can reply to continue."
                ))
                .await?;
                return Ok(match self.call_model(provider, &[]).await? {
                    Ok(_) => RunState::MaxTurns,
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
            if !tool_calls.is_empty() {
                self.run_tools(tool_calls).await?;
                continue;
            }
            if self.absorb().await? {
                continue;
            }
            return Ok(RunState::Completed);
        }
    }

    /// 把已到达的新输入并入本 run；有并入返回 true。
    async fn absorb(&mut self) -> Result<bool, StoreError> {
        let inputs = self.engine.store.absorb(self.run.id).await?;
        let absorbed = !inputs.is_empty();
        self.pending_inputs.extend(inputs);
        Ok(absorbed)
    }

    /// `tools` 为空即不开放工具。每次尝试落盘调用行，成功时连同 `Reply`；返回其中的工具调用。
    /// 失败已落盘通知，返回终态。
    async fn call_model(
        &mut self,
        provider: &dyn Provider,
        tools: &[ToolHandle],
    ) -> Result<Result<Vec<Call>, RunState>, StoreError> {
        if self.needs_compaction().await? {
            let extra = self.pending_text().await?;
            if let Err(failed) = self.compact(provider, extra).await? {
                return Ok(Err(failed));
            }
        }
        let window = self.engine.store.context_window(self.session.id).await?;
        let context = ModelCallContext {
            boundary_id: window.boundary_id,
            through: window.messages.last().map(|m| m.id),
        };
        let images = self
            .engine
            .store
            .images(self.session.id, request::image_ids(&window))
            .await?;
        let channel_prompt = match &self.session.kind {
            SessionKind::Root { channel, .. } => {
                self.engine.channel_prompts.get(channel.as_str()).copied()
            }
            SessionKind::Task { .. } | SessionKind::Triggered { .. } => None,
        };
        let mut req = request::build(
            self.session.id,
            window,
            images,
            &self.session.pwd,
            channel_prompt,
            tools,
            self.settings,
        );
        req.messages
            .sort_by_key(|message| self.pending_inputs.contains(&message.id));
        let mut attempt = 1;
        loop {
            let started_at = now_ms();
            let error = match self.stream(provider, req.clone()).await {
                Ok(reply) => {
                    self.pending_inputs.clear();
                    return self
                        .persist_reply(provider, started_at, reply, context)
                        .await
                        .map(Ok);
                }
                Err(e) => e,
            };
            self.record(
                provider,
                started_at,
                ModelCallOutcome::Failed {
                    error: error.to_string(),
                },
                context,
            )
            .await?;
            match &error {
                ProviderError::Transient { retry_after, .. } if attempt < MAX_MODEL_ATTEMPTS => {
                    let wait = retry_after
                        .unwrap_or(RETRY_BASE * 2u32.pow(attempt - 1))
                        .min(MAX_RETRY_WAIT);
                    tracing::warn!(model = provider.model(), %error, attempt, ?wait, "model call failed, retrying");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                _ => {
                    tracing::warn!(model = provider.model(), %error, attempt, "model call failed");
                    self.notify(provider_failure_text(&error, attempt)).await?;
                    return Ok(Err(RunState::ProviderFailed));
                }
            }
        }
    }

    /// 一次尝试：转发增量，返回 `Finished` 的结果。
    async fn stream(
        &self,
        provider: &dyn Provider,
        req: ModelRequest,
    ) -> Result<ModelResponse, ProviderError> {
        let mut stream = provider.stream(req);
        loop {
            match poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
                Some(Ok(ModelEvent::TextDelta(text))) => {
                    self.emit(KernelEventKind::TextDelta(text))
                }
                Some(Ok(ModelEvent::ReasoningDelta(text))) => {
                    self.emit(KernelEventKind::ReasoningDelta(text))
                }
                Some(Ok(ModelEvent::Finished(reply))) => return Ok(reply),
                Some(Err(e)) => return Err(e),
                None => {
                    return Err(ProviderError::Protocol {
                        message: "stream ended without Finished".into(),
                    })
                }
            }
        }
    }

    /// 落盘调用行与 `Reply`，结束草稿，截断时通知；返回其中的工具调用。
    async fn persist_reply(
        &self,
        provider: &dyn Provider,
        started_at: i64,
        reply: ModelResponse,
        context: ModelCallContext,
    ) -> Result<Vec<Call>, StoreError> {
        let calls = reply
            .blocks
            .iter()
            .filter_map(|b| match b {
                ReplyBlock::ToolCall { id, name, args } => Some(Call {
                    id: id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                }),
                _ => None,
            })
            .collect();
        let outcome = ModelCallOutcome::Replied {
            usage: reply.usage,
            blocks: reply.blocks,
        };
        self.record(provider, started_at, outcome, context).await?;
        match reply.stop {
            StopReason::MaxTokens => {
                self.notify("回复达到输出长度上限，内容不完整。".into())
                    .await?
            }
            StopReason::ContentFilter => {
                self.notify("回复被上游内容审核截断，内容不完整。".into())
                    .await?
            }
            StopReason::EndTurn | StopReason::ToolUse => {}
        }
        Ok(calls)
    }

    /// 落盘调用行并结束草稿：有 `Reply` 即发布它，否则作废。
    async fn record(
        &self,
        provider: &dyn Provider,
        started_at: i64,
        outcome: ModelCallOutcome,
        context: ModelCallContext,
    ) -> Result<(), StoreError> {
        let publish = self.engine.events.publisher().await;
        let (_, reply) = self
            .engine
            .store
            .record_model_call(NewModelCall {
                purpose: mic_store::ModelCallPurpose::Conversation,
                context,
                session_id: self.session.id,
                run_id: Some(self.run.id),
                model: provider.model().to_owned(),
                started_at,
                finished_at: now_ms(),
                outcome,
            })
            .await?;
        match reply {
            Some(message) => publish.appended(self.session.id, self.channel, message),
            None => self.emit(KernelEventKind::DraftDiscarded),
        }
        Ok(())
    }

    /// 本批调用并行执行，结果按完成先后落盘。
    async fn run_tools(&self, calls: Vec<Call>) -> Result<(), StoreError> {
        let mut running = JoinSet::new();
        for call in calls {
            let handle = self.tools.iter().find(|t| t.name() == call.name).cloned();
            let ctx = ToolContext::new(PathBuf::from(&self.session.pwd));
            running.spawn(async move {
                let outcome = match handle {
                    None => ExecOutcome::Failed {
                        kind: ExecFailureKind::Input,
                        message: format!("unknown tool `{}`", call.name),
                    },
                    Some(tool) => match tool.invoke(&call.args, &ctx).await {
                        Ok(output) => ExecOutcome::Completed { output },
                        Err(e) => ExecOutcome::Failed {
                            kind: e.kind,
                            message: e.message,
                        },
                    },
                };
                (call.id, call.name, outcome)
            });
        }
        while let Some(joined) = running.join_next().await {
            let (tool_call_id, tool_name, outcome) = match joined {
                Ok(done) => done,
                Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
                Err(e) => unreachable!("工具任务不会被单独取消：{e}"),
            };
            self.append(MessageBody::ToolResult {
                tool_name,
                tool_call_id,
                outcome: ToolResultOutcome::Terminal(outcome),
            })
            .await?;
        }
        Ok(())
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
