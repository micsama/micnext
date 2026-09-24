//! 一轮 Query 的执行。契约：docs/blueprints/query-execution.md §4.3～4.5、§4.7、§4.8。

use std::future::poll_fn;
use std::path::PathBuf;
use std::sync::Arc;

use mic_message::{
    ExecFailureKind, ExecOutcome, Message, MessageAuthor, MessageContent, SessionEntry,
    SessionEntryId, ToolResultOutcome,
};
use mic_store::{
    FailureReason, ModelCallInput, ModelCallOutcome, ModelCallPurpose, OutputInput, Query,
    QueryState, Session, Store, StoreError, ToolScope, Usage,
};
use mic_tool::{ToolContext, ToolHandle};
use tokio::task::JoinSet;

use crate::event::{Events, KernelEventKind};
use crate::limits::{MAX_MODEL_ATTEMPTS, MAX_RETRY_WAIT, MAX_TURNS_WARN_PERCENT, RETRY_BASE};
use crate::{
    request, ModelEvent, ModelRequest, ModelResponse, Provider, ProviderError, StopReason,
};

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
    /// 执行一个已 claim 的 Query 并落盘终态。
    pub(crate) async fn run_query(
        &self,
        session: &Session,
        channel: &str,
        query: Query,
    ) -> Result<QueryState, StoreError> {
        self.events.emit(
            session.id,
            channel,
            KernelEventKind::QueryStarted { query_id: query.id },
        );
        tracing::info!(session = session.id.0, query = query.id.0, "query started");
        let tools = match &session.tool_scope {
            ToolScope::All => self.tools.clone(),
            ToolScope::Only(names) => self
                .tools
                .iter()
                .filter(|t| names.contains(t.name()))
                .cloned()
                .collect(),
        };
        let mut run = Run {
            engine: self,
            session,
            channel,
            query: &query,
            claimed_end: query.claimed_end_id,
            tools,
        };
        let state = run.execute().await?;
        self.store
            .finish_query(query.id, state.clone(), now_ms())
            .await?;
        tracing::info!(
            session = session.id.0,
            query = query.id.0,
            ?state,
            "query finished"
        );
        self.events.emit(
            session.id,
            channel,
            KernelEventKind::QueryFinished {
                query_id: query.id,
                state: state.clone(),
            },
        );
        Ok(state)
    }
}

struct Run<'a> {
    engine: &'a Engine,
    session: &'a Session,
    channel: &'a str,
    query: &'a Query,
    claimed_end: SessionEntryId,
    tools: Vec<ToolHandle>,
}

struct Call {
    id: String,
    name: String,
    args: serde_json::Value,
}

impl Run<'_> {
    async fn execute(&mut self) -> Result<QueryState, StoreError> {
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
                let reply = match self.call_model(&[]).await? {
                    Ok(reply) => reply,
                    Err(failed) => return Ok(failed),
                };
                self.persist_reply(reply).await?;
                return Ok(QueryState::Failed {
                    reason: FailureReason::MaxTurns { limit: max_turns },
                });
            }
            if calls == warn_at && warn_at < max_turns {
                self.note(format!(
                    "You have used {calls} of {max_turns} model calls allowed for this request. Wrap up soon: finish the essential steps and report to the user."
                ))
                .await?;
            }
            calls += 1;
            let reply = match self.call_model(&self.tools).await? {
                Ok(reply) => reply,
                Err(failed) => return Ok(failed),
            };
            let tool_calls = self.persist_reply(reply).await?;
            if !tool_calls.is_empty() {
                self.run_tools(tool_calls).await?;
                continue;
            }
            if self.absorb().await? {
                continue;
            }
            return Ok(QueryState::Completed);
        }
    }

    /// 把已到达的新输入并入本 Query；有并入返回 true。
    async fn absorb(&mut self) -> Result<bool, StoreError> {
        match self.engine.store.extend_claim(self.query.id).await? {
            Some(end) => {
                self.claimed_end = end;
                Ok(true)
            }
            None => Ok(false),
        }
    }

    /// `tools` 为空即不开放工具。成功返回回复（草稿仍未结束，由 `persist_reply` 收尾）；
    /// 失败已落盘通知，返回终态。
    async fn call_model(
        &self,
        tools: &[ToolHandle],
    ) -> Result<Result<ModelResponse, QueryState>, StoreError> {
        let window = self.engine.store.context_window(self.session.id).await?;
        let req = request::build(window, &self.session.pwd, tools, self.claimed_end);
        let mut attempt = 1;
        loop {
            let started_at = now_ms();
            let result = self.stream(req.clone()).await;
            let (model, usage, outcome) = match &result {
                Ok(reply) => (
                    reply.model.clone(),
                    reply.usage,
                    ModelCallOutcome::Completed,
                ),
                Err(e) => (
                    self.engine.model.clone(),
                    Usage::default(),
                    ModelCallOutcome::Failed {
                        message: e.to_string(),
                    },
                ),
            };
            self.engine
                .store
                .record_model_call(ModelCallInput {
                    session_id: self.session.id,
                    purpose: ModelCallPurpose::Query(self.query.id),
                    model,
                    usage,
                    outcome,
                    started_at,
                    finished_at: now_ms(),
                })
                .await?;
            let error = match result {
                Ok(reply) => return Ok(Ok(reply)),
                Err(e) => e,
            };
            self.emit(KernelEventKind::DraftEnded);
            match &error {
                ProviderError::Transient { retry_after, .. } if attempt < MAX_MODEL_ATTEMPTS => {
                    let wait = retry_after
                        .unwrap_or(RETRY_BASE * 2u32.pow(attempt - 1))
                        .min(MAX_RETRY_WAIT);
                    tracing::warn!(%error, attempt, ?wait, "model call failed, retrying");
                    tokio::time::sleep(wait).await;
                    attempt += 1;
                }
                _ => {
                    tracing::warn!(%error, attempt, "model call failed");
                    self.notify(provider_failure_text(&error, attempt)).await?;
                    return Ok(Err(QueryState::Failed {
                        reason: FailureReason::Provider {
                            message: error.to_string(),
                        },
                    }));
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

    /// 按顺序落盘回复各 entry，结束草稿，截断时通知；返回其中的工具调用。
    async fn persist_reply(&self, reply: ModelResponse) -> Result<Vec<Call>, StoreError> {
        let mut calls = Vec::new();
        for content in reply.content {
            if let MessageContent::ToolCall { id, name, args } = &content {
                calls.push(Call {
                    id: id.clone(),
                    name: name.clone(),
                    args: args.clone(),
                });
            }
            self.append(
                MessageAuthor::Assistant {
                    model: reply.model.clone(),
                },
                content,
            )
            .await?;
        }
        self.emit(KernelEventKind::DraftEnded);
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
            let (id, name, outcome) = match joined {
                Ok(done) => done,
                Err(e) if e.is_panic() => std::panic::resume_unwind(e.into_panic()),
                Err(e) => unreachable!("工具任务不会被单独取消：{e}"),
            };
            self.append(
                MessageAuthor::Tool { name },
                MessageContent::ToolResult {
                    tool_call_id: id,
                    outcome: ToolResultOutcome::Terminal(outcome),
                },
            )
            .await?;
        }
        Ok(())
    }

    /// 给模型看的框架备注。
    async fn note(&self, text: String) -> Result<(), StoreError> {
        self.append(
            MessageAuthor::HarnessNote,
            MessageContent::Text { content: text },
        )
        .await
    }

    /// 投递给用户的框架通知。
    async fn notify(&self, text: String) -> Result<(), StoreError> {
        append_output(
            &self.engine.store,
            &self.engine.events,
            self.session,
            self.channel,
            notification(),
            MessageContent::Text { content: text },
        )
        .await
    }

    async fn append(
        &self,
        author: MessageAuthor,
        content: MessageContent,
    ) -> Result<(), StoreError> {
        append_output(
            &self.engine.store,
            &self.engine.events,
            self.session,
            self.channel,
            author,
            content,
        )
        .await
    }

    fn emit(&self, kind: KernelEventKind) {
        self.engine.events.emit(self.session.id, self.channel, kind);
    }
}

/// 落盘一条产出并发 `EntryAppended`。
async fn append_output(
    store: &Store,
    events: &Events,
    session: &Session,
    channel: &str,
    author: MessageAuthor,
    content: MessageContent,
) -> Result<(), StoreError> {
    let created_at = now_ms();
    let id = store
        .append_output(OutputInput {
            session_id: session.id,
            author: author.clone(),
            content: content.clone(),
            created_at,
        })
        .await?;
    events.emit(
        session.id,
        channel,
        KernelEventKind::EntryAppended(SessionEntry::Message(Message {
            id,
            session_id: session.id,
            author,
            content,
            created_at,
            delivered_at: None,
        })),
    );
    Ok(())
}

pub(crate) fn notification() -> MessageAuthor {
    MessageAuthor::Notification {
        source: "micnext".into(),
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
