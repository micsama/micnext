//! 一个 run 的执行。契约：docs/blueprints/run-execution.md §4.3～4.5、§4.7、§4.8。

use std::future::poll_fn;
use std::path::PathBuf;
use std::sync::Arc;

use mic_message::{
    ExecFailureKind, ExecOutcome, Message, MessageBody, ReplyBlock, ToolResultOutcome,
};
use mic_store::{
    ModelCallOutcome, NewModelCall, Run, RunState, Session, Store, StoreError, ToolScope,
};
use mic_tool::{ToolContext, ToolHandle};
use tokio::task::JoinSet;

use crate::event::{Events, KernelEventKind};
use crate::limits::{MAX_MODEL_ATTEMPTS, MAX_RETRY_WAIT, MAX_TURNS_WARN_PERCENT, RETRY_BASE};
use crate::{
    request, ModelEvent, ModelRequest, ModelResponse, Provider, ProviderError, StopReason,
};

/// 框架通知的 `source`。
pub(crate) const NOTIFICATION_SOURCE: &str = "micnext";

/// 执行主路径的共享部分；调度与 `-p` 共用。
pub(crate) struct Engine {
    pub(crate) store: Store,
    pub(crate) events: Events,
    pub(crate) provider: Arc<dyn Provider>,
    /// `[models] default` 的条目名。
    pub(crate) model: String,
    pub(crate) tools: Vec<ToolHandle>,
    pub(crate) max_turns: u32,
}

impl Engine {
    /// 执行一个已认领的 run 并落盘终态。
    pub(crate) async fn run(
        &self,
        session: &Session,
        channel: &str,
        run: Run,
    ) -> Result<RunState, StoreError> {
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
        let exec = Exec {
            engine: self,
            session,
            channel,
            run: &run,
            tools,
        };
        let state = exec.execute().await?;
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

struct Exec<'a> {
    engine: &'a Engine,
    session: &'a Session,
    channel: &'a str,
    run: &'a Run,
    tools: Vec<ToolHandle>,
}

struct Call {
    id: String,
    name: String,
    args: serde_json::Value,
}

impl Exec<'_> {
    async fn execute(&self) -> Result<RunState, StoreError> {
        let max_turns = self.engine.max_turns;
        let warn_at = (max_turns * MAX_TURNS_WARN_PERCENT / 100).max(1);
        let mut calls = 0;
        loop {
            self.absorb().await?;
            if calls == max_turns {
                self.note(format!(
                    "You have used all {max_turns} model calls allowed for this request, and tools are no longer available. Summarize what you have done and what remains unfinished, and tell the user they can reply to continue."
                ))
                .await?;
                return Ok(match self.call_model(&[]).await? {
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
            let tool_calls = match self.call_model(&self.tools).await? {
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
    async fn absorb(&self) -> Result<bool, StoreError> {
        self.engine.store.absorb(self.run.id).await
    }

    /// `tools` 为空即不开放工具。每次尝试落盘调用行，成功时连同 `Reply`；返回其中的工具调用。
    /// 失败已落盘通知，返回终态。
    async fn call_model(
        &self,
        tools: &[ToolHandle],
    ) -> Result<Result<Vec<Call>, RunState>, StoreError> {
        let window = self.engine.store.context_window(self.session.id).await?;
        let req = request::build(window, &self.session.pwd, tools);
        let mut attempt = 1;
        loop {
            let started_at = now_ms();
            let error = match self.stream(req.clone()).await {
                Ok(reply) => return self.persist_reply(started_at, reply).await.map(Ok),
                Err(e) => e,
            };
            self.record(
                started_at,
                ModelCallOutcome::Failed {
                    error: error.to_string(),
                },
            )
            .await?;
            self.emit(KernelEventKind::DraftDiscarded);
            match &error {
                ProviderError::Transient { retry_after, .. } if attempt < MAX_MODEL_ATTEMPTS => {
                    let wait = retry_after
                        .unwrap_or(RETRY_BASE * 2u32.pow(attempt - 1))
                        .min(MAX_RETRY_WAIT);
                    tracing::warn!(model = %self.engine.model, %error, attempt, ?wait, "model call failed, retrying");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                _ => {
                    tracing::warn!(model = %self.engine.model, %error, attempt, "model call failed");
                    self.notify(provider_failure_text(&error, attempt)).await?;
                    return Ok(Err(RunState::ProviderFailed));
                }
            }
        }
    }

    /// 一次尝试：转发增量，返回 `Finished` 的结果。
    async fn stream(&self, req: ModelRequest) -> Result<ModelResponse, ProviderError> {
        let mut stream = self.engine.provider.stream(req);
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
        started_at: i64,
        reply: ModelResponse,
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
        match self.record(started_at, outcome).await? {
            Some(message) => self.emit(KernelEventKind::MessageAppended(message)),
            None => self.emit(KernelEventKind::DraftDiscarded),
        }
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

    async fn record(
        &self,
        started_at: i64,
        outcome: ModelCallOutcome,
    ) -> Result<Option<Message>, StoreError> {
        let (_, reply) = self
            .engine
            .store
            .record_model_call(NewModelCall {
                session_id: self.session.id,
                run_id: Some(self.run.id),
                model: self.engine.provider.model().to_owned(),
                started_at,
                finished_at: now_ms(),
                outcome,
            })
            .await?;
        Ok(reply)
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
    async fn notify(&self, text: String) -> Result<(), StoreError> {
        self.append(MessageBody::Notification {
            source: NOTIFICATION_SOURCE.into(),
            text,
        })
        .await
    }

    /// 落盘一条本 run 的产出并发 `MessageAppended`。
    async fn append(&self, body: MessageBody) -> Result<(), StoreError> {
        let message = self
            .engine
            .store
            .append(self.session.id, Some(self.run.id), body, now_ms())
            .await?;
        self.emit(KernelEventKind::MessageAppended(message));
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
