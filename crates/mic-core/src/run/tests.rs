use std::pin::Pin;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;
use std::task::{Context, Poll};

use mic_message::{ReplyBlock, SessionCommand};
use mic_store::{
    CredentialWrite, EndpointWrite, InputDisposition, ModelWrite, NewInputPart, NewSession,
    ReasoningEffort as E, ToolScope,
};
use tokio::sync::Notify;

use super::*;
use crate::{
    BoxFuture, BoxStream, ConfigError, ModelRequest, ProbeError, ProviderFactory, ReasoningOptions,
    SecretValue,
};

#[derive(Default)]
struct Capture {
    efforts: Mutex<Vec<Option<E>>>,
    started: Notify,
    builds: AtomicUsize,
}

struct Factory(Arc<Capture>);

impl ProviderFactory for Factory {
    fn kind(&self) -> &'static str {
        "test"
    }
    fn display_name(&self) -> &'static str {
        "test"
    }
    fn check_endpoint(&self, json: &str) -> Result<String, ConfigError> {
        Ok(json.into())
    }
    fn check_model(&self, _: &str, _: &str, json: &str) -> Result<String, ConfigError> {
        Ok(json.into())
    }
    fn key_env(&self, _: &str) -> Option<&'static str> {
        None
    }
    fn reasoning_options(&self, _: &str, _: &str) -> Result<Option<ReasoningOptions>, ConfigError> {
        Ok(Some(ReasoningOptions {
            choices: vec![E::None, E::Low, E::High],
            default: E::Low,
        }))
    }
    fn build(
        &self,
        _: &str,
        _: Option<SecretValue>,
        _: &str,
        _: &str,
        effort: Option<E>,
    ) -> Result<Arc<dyn Provider>, ConfigError> {
        self.0.builds.fetch_add(1, Ordering::Relaxed);
        Ok(Arc::new(Mock {
            capture: self.0.clone(),
            effort,
        }))
    }
    fn list_models(
        &self,
        _: &str,
        _: Option<SecretValue>,
    ) -> BoxFuture<Result<Vec<String>, ProbeError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct Mock {
    capture: Arc<Capture>,
    effort: Option<E>,
}

struct OneEvent(Option<Result<ModelEvent, ProviderError>>);

impl futures_core::Stream for OneEvent {
    type Item = Result<ModelEvent, ProviderError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.take())
    }
}

impl Provider for Mock {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(&self, _: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        let mut efforts = self.capture.efforts.lock().unwrap();
        let first = efforts.is_empty();
        efforts.push(self.effort);
        let result = if first {
            self.capture.started.notify_one();
            Err(ProviderError::Transient {
                message: "retry".into(),
                retry_after: Some(std::time::Duration::from_millis(10)),
            })
        } else {
            Ok(ModelEvent::Finished(ModelResponse {
                blocks: vec![ReplyBlock::Text {
                    text: "summary/reply".into(),
                    phase: None,
                }],
                stop: StopReason::EndTurn,
            }))
        };
        Box::pin(OneEvent(Some(result)))
    }
}

#[tokio::test(flavor = "current_thread")]
async fn retries_keep_the_claimed_effort_and_compaction_uses_the_next_selection() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("test", 0).await.unwrap();
    let endpoint = store
        .create_endpoint(
            EndpointWrite {
                name: "test".into(),
                kind: "test".into(),
                config_json: "{}".into(),
                credential: CredentialWrite::Clear,
            },
            0,
        )
        .await
        .unwrap();
    let model = store
        .create_model(
            ModelWrite {
                endpoint_id: endpoint,
                name: "test".into(),
                config_json: "{}".into(),
                context_window: None,
                images: true,
                price: Default::default(),
            },
            0,
        )
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: "web".into(),
                chat: "test".into(),
            },
            parent_session_id: None,
            delivery_target: None,
            pwd: "/tmp".into(),
            tool_scope: ToolScope::All,
            created_at: 0,
            persona: None,
            model: Some(model),
            reasoning_effort: Some(E::High),
        })
        .await
        .unwrap();
    let capture = Arc::new(Capture::default());
    let mut factories = Factories::default();
    factories
        .insert(Box::new(Factory(capture.clone())))
        .unwrap();
    let events = Events::new();
    let engine = Arc::new(Engine {
        jobs: crate::jobs::Jobs::scratch(&store, &events),
        store: store.clone(),
        events,
        factories: Arc::new(factories),
        tools: Vec::new(),
        channel_prompts: Arc::new(HashMap::new()),
        control: Default::default(),
    });
    store
        .append_input(
            session.id,
            owner,
            vec![NewInputPart::Text("first".into())],
            InputDisposition::Pending,
            None,
            1,
        )
        .await
        .unwrap();
    let claimed = store.claim_next(session.id, 2).await.unwrap().unwrap();
    let (task_engine, task_session) = (engine.clone(), session.clone());
    let task = tokio::spawn(async move {
        task_engine
            .run(
                &task_session,
                "web",
                claimed,
                &task_engine.control.enter(task_session.id).unwrap(),
            )
            .await
            .unwrap()
    });
    capture.started.notified().await;
    store
        .set_session_model(session.id, model, Some(E::None))
        .await
        .unwrap();
    assert_eq!(task.await.unwrap(), RunState::Completed);
    store
        .append_command(session.id, owner, SessionCommand::Compact { text: None }, 3)
        .await
        .unwrap();
    let claimed = store.claim_next(session.id, 4).await.unwrap().unwrap();
    assert_eq!(
        engine
            .run(
                &session,
                "web",
                claimed,
                &engine.control.enter(session.id).unwrap(),
            )
            .await
            .unwrap(),
        RunState::Completed
    );
    store
        .set_session_model(session.id, model, None)
        .await
        .unwrap();
    store
        .append_input(
            session.id,
            owner,
            vec![NewInputPart::Text("next default".into())],
            InputDisposition::Pending,
            None,
            5,
        )
        .await
        .unwrap();
    let claimed = store.claim_next(session.id, 6).await.unwrap().unwrap();
    assert_eq!(
        engine
            .run(
                &session,
                "web",
                claimed,
                &engine.control.enter(session.id).unwrap(),
            )
            .await
            .unwrap(),
        RunState::Completed
    );
    assert_eq!(
        *capture.efforts.lock().unwrap(),
        [Some(E::High), Some(E::High), Some(E::None), Some(E::Low)]
    );
    assert_eq!(capture.builds.load(Ordering::Relaxed), 3);
}

// 每次实际请求与公开重建入口逐条比较，忽略临时指令的 id / 时间。
struct Scripted {
    requests: Mutex<Vec<ModelRequest>>,
    replies: Mutex<std::collections::VecDeque<Vec<Result<ModelEvent, ProviderError>>>>,
}
struct ScriptEvents(std::collections::VecDeque<Result<ModelEvent, ProviderError>>);
impl futures_core::Stream for ScriptEvents {
    type Item = Result<ModelEvent, ProviderError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        Poll::Ready(self.0.pop_front())
    }
}
impl Provider for Scripted {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(&self, req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        self.requests.lock().unwrap().push(req);
        Box::pin(ScriptEvents(
            self.replies
                .lock()
                .unwrap()
                .pop_front()
                .expect("unexpected call")
                .into(),
        ))
    }
}
fn scripted(replies: Vec<Vec<Result<ModelEvent, ProviderError>>>) -> Scripted {
    Scripted {
        requests: Mutex::default(),
        replies: Mutex::new(replies.into()),
    }
}
fn reply(text: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    vec![Ok(ModelEvent::Finished(ModelResponse {
        blocks: vec![ReplyBlock::Text {
            text: text.into(),
            phase: None,
        }],
        stop: StopReason::EndTurn,
    }))]
}
fn failed() -> Vec<Result<ModelEvent, ProviderError>> {
    vec![
        Ok(ModelEvent::TextDelta(String::new())),
        Ok(ModelEvent::ToolCallDelta),
        Ok(ModelEvent::Usage(mic_store::Usage {
            input_tokens: 12,
            output_tokens: 2,
            cache_read_tokens: Some(0),
            cache_write_tokens: Some(0),
            reasoning_tokens: None,
        })),
        Err(ProviderError::Transient {
            message: "try again".into(),
            retry_after: Some(Duration::ZERO),
        }),
    ]
}
struct Fixture {
    store: Store,
    engine: Arc<Engine>,
    kernel: crate::Kernel,
    session: Session,
    claimed: ClaimedRun,
    owner: mic_message::PersonId,
}
impl Fixture {
    async fn new() -> Self {
        let store = Store::open_in_memory(&[]).await.unwrap();
        let owner = store.ensure_person("test", now_ms()).await.unwrap();
        let endpoint = store
            .create_endpoint(
                EndpointWrite {
                    name: "test".into(),
                    kind: "test".into(),
                    config_json: "{}".into(),
                    credential: CredentialWrite::Clear,
                },
                now_ms(),
            )
            .await
            .unwrap();
        let model = store
            .create_model(
                ModelWrite {
                    endpoint_id: endpoint,
                    name: "test".into(),
                    config_json: "{}".into(),
                    context_window: None,
                    images: true,
                    price: mic_store::ModelPrice {
                        input: Some(1.0),
                        output: Some(2.0),
                        ..Default::default()
                    },
                },
                now_ms(),
            )
            .await
            .unwrap();
        let session = store
            .create_session(NewSession {
                kind: SessionKind::Root {
                    channel: "web".into(),
                    chat: "trace".into(),
                },
                parent_session_id: None,
                delivery_target: None,
                pwd: "/tmp".into(),
                tool_scope: ToolScope::All,
                created_at: now_ms(),
                persona: None,
                model: Some(model),
                reasoning_effort: None,
            })
            .await
            .unwrap();
        store
            .append_input(
                session.id,
                owner,
                vec![NewInputPart::Text("first".into())],
                InputDisposition::Pending,
                None,
                now_ms(),
            )
            .await
            .unwrap();
        let claimed = store
            .claim_next(session.id, now_ms())
            .await
            .unwrap()
            .unwrap();
        let events = Events::new();
        let engine = Arc::new(Engine {
            jobs: crate::jobs::Jobs::scratch(&store, &events),
            store: store.clone(),
            events,
            factories: Arc::new(Factories::default()),
            tools: vec![],
            channel_prompts: Arc::default(),
            control: Default::default(),
        });
        let (titles, _worker) = crate::title::TitleRuntime::new(
            store.clone(),
            engine.factories.clone(),
            engine.events.clone(),
        );
        let kernel = crate::Kernel::new(
            owner,
            tokio::sync::mpsc::channel(1).0,
            Arc::default(),
            titles,
            engine.clone(),
        );
        Self {
            store,
            engine,
            kernel,
            session,
            claimed,
            owner,
        }
    }
    fn exec(&self) -> Exec<'_> {
        Exec {
            engine: &self.engine,
            session: &self.session,
            channel: "web",
            run: &self.claimed.run,
            settings: &self.claimed.settings,
            tools: vec![],
            pending_inputs: match &self.claimed.input {
                RunInput::Conversation { message_ids } => message_ids.clone(),
                _ => unreachable!(),
            },
            model_id: match self.claimed.model {
                ClaimedModel::Selected { id, .. } => Some(id),
                _ => None,
            },
            effort: None,
            context_window: 100_000,
            images: true,
            stop: CancellationToken::new(),
        }
    }
    async fn verify(&self, provider: &Scripted) -> Vec<mic_store::TraceCall> {
        self.store
            .finish_run(self.claimed.run.id, RunState::Completed, None, now_ms())
            .await
            .unwrap();
        let page = self
            .store
            .trace_runs(self.session.id, None, 10)
            .await
            .unwrap();
        let calls = &page.runs[0].calls;
        let requests = provider.requests.lock().unwrap().clone();
        assert_eq!(calls.len(), requests.len());
        for (call, actual) in calls.iter().zip(requests) {
            let rebuilt = self.kernel.call_input(call.id).await.unwrap().unwrap();
            let bodies = |messages: Vec<mic_message::Message>| {
                messages
                    .into_iter()
                    .map(|m| serde_json::to_value(m.body).unwrap())
                    .collect::<Vec<_>>()
            };
            assert_eq!(
                bodies(rebuilt),
                bodies(actual.messages),
                "call {}",
                call.id.0
            );
        }
        calls.clone()
    }
}

#[tokio::test]
async fn trace_rebuild_survives_later_holds_absorption_and_retries() {
    let f = Fixture::new().await;
    let excluded = f
        .store
        .append_input(
            f.session.id,
            f.owner,
            vec![NewInputPart::Text("later".into())],
            InputDisposition::Pending,
            None,
            now_ms(),
        )
        .await
        .unwrap();
    f.store
        .append(
            f.session.id,
            Some(f.claimed.run.id),
            MessageBody::HarnessNote {
                text: "after pending".into(),
            },
            now_ms(),
        )
        .await
        .unwrap();
    let provider = scripted(vec![failed(), reply("ok"), reply("after absorb")]);
    let mut exec = f.exec();
    assert!(exec.call_model(&provider, &[]).await.unwrap().is_ok());
    f.store.hold_unclaimed_inputs(now_ms()).await.unwrap();
    // held 改变之后重建第一次请求，仍不能混入该输入。
    let record_id = f
        .store
        .with_module_tx(|tx| {
            tx.query_row("SELECT min(id) FROM core_model_calls", [], |r| {
                r.get::<_, i64>(0)
            })
        })
        .await
        .unwrap();
    let rebuilt = f
        .kernel
        .call_input(mic_store::ModelCallId(record_id))
        .await
        .unwrap()
        .unwrap();
    assert!(!rebuilt.iter().any(|m| m.id == excluded[0].id));
    // 新输入在同 run 后续吸收，不能反向改变前一次调用。
    let next = f
        .store
        .append_input(
            f.session.id,
            f.owner,
            vec![NewInputPart::Text("absorbed".into())],
            InputDisposition::Pending,
            None,
            now_ms(),
        )
        .await
        .unwrap();
    assert!(exec.absorb().await.unwrap());
    assert!(exec.pending_inputs.contains(&next[0].id));
    assert!(exec.call_model(&provider, &[]).await.unwrap().is_ok());
    let calls = f.verify(&provider).await;
    assert_eq!(
        calls.iter().map(|c| c.attempt).collect::<Vec<_>>(),
        [1, 2, 1]
    );
    assert_eq!(calls[0].usage.unwrap().input_tokens, 12);
    assert!(calls[0].first_output_at.is_some());
    assert!(calls[0].reply_id.is_none());
    let board = f.store.dashboard(1).await.unwrap();
    assert_eq!(board.models[0].calls.calls, 3);
    assert_eq!(board.models[0].calls.failed_calls, 1);
    assert_eq!(board.models[0].calls.unknown_usage, 2);
    assert_eq!(board.models[0].calls.unknown_cost, 2);
    assert_eq!(board.models[0].calls.cost, 0.000016);
}

#[tokio::test]
async fn all_failed_attempts_keep_usage_without_reply() {
    let f = Fixture::new().await;
    let provider = scripted(vec![failed(), failed(), failed()]);
    assert!(matches!(
        f.exec().call_model(&provider, &[]).await.unwrap(),
        Err(RunState::ProviderFailed)
    ));
    let calls = f.verify(&provider).await;
    assert_eq!(calls.len(), 3);
    assert!(calls
        .iter()
        .all(|c| c.reply_id.is_none() && c.usage.is_some()));
}

#[tokio::test]
async fn stalled_call_fails_without_retry() {
    let f = Fixture::new().await;
    let provider = scripted(vec![vec![Err(ProviderError::Stalled {
        message: "300 秒没有收到数据".into(),
    })]]);
    assert!(matches!(
        f.exec().call_model(&provider, &[]).await.unwrap(),
        Err(RunState::ProviderFailed)
    ));
    assert_eq!(f.verify(&provider).await.len(), 1);
    let messages = f.store.run_messages(f.claimed.run.id).await.unwrap();
    assert!(messages.iter().any(|m| matches!(&m.body,
        MessageBody::Notification { text, .. } if text.starts_with("模型无响应"))));
}

#[tokio::test]
async fn compaction_links_boundary_and_parse_failure_keeps_provider_success() {
    let f = Fixture::new().await;
    let provider = scripted(vec![reply("reply"), reply("summary"), reply("")]);
    let mut exec = f.exec();
    exec.call_model(&provider, &[]).await.unwrap().unwrap();
    let command = f
        .store
        .append_command(
            f.session.id,
            f.owner,
            SessionCommand::Compact {
                text: Some("focus".into()),
            },
            now_ms(),
        )
        .await
        .unwrap();
    exec.pending_inputs = vec![command.id];
    exec.compact(&provider).await.unwrap().unwrap();
    exec.pending_inputs.clear();
    assert!(exec.compact(&provider).await.unwrap().is_err());
    let calls = f.verify(&provider).await;
    assert!(calls[1].boundary_id.is_some());
    assert!(calls[2].boundary_id.is_none());
    assert!(calls[2].error.is_none());
    assert_eq!(calls[2].stop, Some(StopReason::EndTurn));
}

#[tokio::test]
async fn compaction_can_trim_every_message_and_rebuild_instruction_only() {
    let f = Fixture::new().await;
    let provider = scripted(vec![reply("history"), reply("summary")]);
    let mut exec = f.exec();
    exec.call_model(&provider, &[]).await.unwrap().unwrap();
    exec.context_window = 1;
    exec.compact(&provider).await.unwrap().unwrap();
    let calls = f.verify(&provider).await;
    let record = f.store.call_record(calls[1].id).await.unwrap().unwrap();
    assert!(record.context.skip > 0);
    assert_eq!(provider.requests.lock().unwrap()[1].messages.len(), 1);
}

struct TimedTool;
impl mic_tool::Tool for TimedTool {
    type Args = HashMap<String, u64>;
    fn name(&self) -> &str {
        "timed"
    }
    fn description(&self) -> &str {
        "test timing"
    }
    fn prompt_hint(&self) -> Option<&str> {
        None
    }
    async fn execute(
        &self,
        args: Self::Args,
        _: &ToolContext,
    ) -> Result<mic_tool::ToolOutput, mic_tool::ToolError> {
        let delay = args["delay"];
        tokio::time::sleep(Duration::from_millis(delay)).await;
        Ok(mic_tool::ToolOutput::Completed(vec![
            mic_message::ContentPart::Text {
                text: delay.to_string(),
            },
        ]))
    }
}

/// 派出一个永不结束的后台任务后自己也不返回：停止落在提交之后。
struct HangAfterDispatch;
impl mic_tool::Tool for HangAfterDispatch {
    type Args = HashMap<String, u64>;
    fn name(&self) -> &str {
        "hang"
    }
    fn description(&self) -> &str {
        "test dispatch"
    }
    fn prompt_hint(&self) -> Option<&str> {
        None
    }
    async fn execute(
        &self,
        _: Self::Args,
        ctx: &ToolContext,
    ) -> Result<mic_tool::ToolOutput, mic_tool::ToolError> {
        ctx.background()
            .dispatch(mic_tool::BackgroundTask {
                description: "hang".into(),
                timeout: None,
                run: Box::new(|_| Box::pin(std::future::pending())),
            })
            .await?;
        std::future::pending().await
    }
}

#[tokio::test]
async fn context_breakdown_explains_last_call_usage() {
    let f = Fixture::new().await;
    assert_eq!(
        f.kernel.context_breakdown(f.session.id).await.unwrap(),
        None
    );
    let usage = |input_tokens, output_tokens| {
        Ok(ModelEvent::Usage(mic_store::Usage {
            input_tokens,
            output_tokens,
            cache_read_tokens: None,
            cache_write_tokens: None,
            reasoning_tokens: None,
        }))
    };
    let provider = scripted(vec![
        vec![
            usage(900, 40),
            Ok(ModelEvent::Finished(ModelResponse {
                blocks: vec![ReplyBlock::ToolCall {
                    id: "t".into(),
                    name: "timed".into(),
                    args: serde_json::json!({ "delay": 0 }),
                }],
                stop: StopReason::ToolUse,
            })),
        ],
        std::iter::once(usage(1234, 56))
            .chain(reply("done"))
            .collect(),
    ]);
    let mut exec = f.exec();
    exec.tools = vec![ToolHandle::new(TimedTool)];
    let calls = exec
        .call_model(&provider, &exec.tools.clone())
        .await
        .unwrap()
        .unwrap();
    exec.run_tools(calls).await.unwrap();
    exec.call_model(&provider, &[]).await.unwrap().unwrap();
    let system = f.kernel.system_prompt(f.session.id).await.unwrap();
    assert_eq!(system.render(), provider.requests.lock().unwrap()[1].system);
    let b = f
        .kernel
        .context_breakdown(f.session.id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(b.used_tokens, 1290);
    assert_eq!(b.parts.iter().map(|p| p.tokens).sum::<u64>(), 1290);
    let conversation = b.parts.last().unwrap();
    let tool_tokens = conversation.tool_tokens.unwrap();
    assert!(tool_tokens > 0 && tool_tokens <= conversation.tokens);
    assert!(b.parts[0].tokens > 0);
}

#[tokio::test]
async fn parallel_tools_publish_in_persisted_order_and_recovery_has_no_start_time() {
    let f = Fixture::new().await;
    let tool_block = |id: &str, delay| ReplyBlock::ToolCall {
        id: id.into(),
        name: "timed".into(),
        args: serde_json::json!({ "delay": delay }),
    };
    let provider = scripted(vec![
        vec![Ok(ModelEvent::Finished(ModelResponse {
            blocks: vec![tool_block("slow", 30), tool_block("fast", 1)],
            stop: StopReason::ToolUse,
        }))],
        reply("done"),
    ]);
    let mut exec = f.exec();
    exec.tools = vec![ToolHandle::new(TimedTool)];
    let mut receiver = f.engine.events.subscribe();
    let calls = exec
        .call_model(&provider, &exec.tools.clone())
        .await
        .unwrap()
        .unwrap();
    f.store
        .append_input(
            f.session.id,
            f.owner,
            vec![NewInputPart::Text("interleaved held input".into())],
            InputDisposition::Held,
            None,
            now_ms(),
        )
        .await
        .unwrap();
    exec.run_tools(calls).await.unwrap();
    exec.call_model(&provider, &[]).await.unwrap().unwrap();
    let mut published = vec![];
    while let Some(event) = receiver.try_recv() {
        if let KernelEventKind::MessageAppended(m) = event.unwrap().kind {
            published.push(m.id);
        }
    }
    assert!(published.windows(2).all(|w| w[0] < w[1]));
    let messages = f.store.run_messages(f.claimed.run.id).await.unwrap();
    let results: Vec<_> = messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::ToolResult {
                tool_call_id,
                started_at,
                ..
            } => Some((tool_call_id.as_str(), started_at.unwrap(), m.created_at)),
            _ => None,
        })
        .collect();
    assert_eq!(
        results.iter().map(|r| r.0).collect::<Vec<_>>(),
        ["fast", "slow"]
    );
    assert!(results.iter().all(|r| r.1 <= r.2));
    f.verify(&provider).await;
    let peers = f.store.call_peers().await.unwrap();
    assert_eq!(peers.tools[0].samples, 2);
    let board = f.store.dashboard(1).await.unwrap();
    assert_eq!(board.tools[0].calls, 2);
    let recovery = Fixture::new().await;
    let provider = scripted(vec![vec![Ok(ModelEvent::Finished(ModelResponse {
        blocks: vec![tool_block("stale", 0)],
        stop: StopReason::ToolUse,
    }))]]);
    recovery
        .exec()
        .call_model(&provider, &[])
        .await
        .unwrap()
        .unwrap();
    assert_eq!(
        crate::recovery::recover(&recovery.store, std::env::temp_dir())
            .await
            .unwrap(),
        1
    );
    let messages = recovery
        .store
        .run_messages(recovery.claimed.run.id)
        .await
        .unwrap();
    assert!(messages.iter().any(|m| matches!(
        &m.body,
        MessageBody::ToolResult {
            started_at: None,
            outcome: ToolResultOutcome::Terminal(ExecOutcome::Cancelled { .. }),
            ..
        }
    )));
}

#[tokio::test]
async fn trace_paging_keeps_session_side_calls_out_of_runs() {
    let f = Fixture::new().await;
    let provider = scripted(vec![reply("first reply")]);
    f.exec().call_model(&provider, &[]).await.unwrap().unwrap();
    let calls = f.verify(&provider).await;
    let model = calls[0].model_id;
    let now = now_ms();
    f.store
        .record_model_call(mic_store::NewModelCall {
            session_id: f.session.id,
            run_id: None,
            purpose: ModelCallPurpose::Title,
            model: "test".into(),
            model_id: model,
            reasoning_effort: None,
            context: Default::default(),
            started_at: now,
            first_output_at: None,
            finished_at: now + 100,
            stop: Some(StopReason::EndTurn),
            usage: None,
            outcome: ModelCallOutcome::Replied { blocks: vec![] },
        })
        .await
        .unwrap();
    f.store
        .append_input(
            f.session.id,
            f.owner,
            vec![NewInputPart::Text("next".into())],
            InputDisposition::Pending,
            None,
            now,
        )
        .await
        .unwrap();
    let next = f
        .store
        .claim_next(f.session.id, now)
        .await
        .unwrap()
        .unwrap();
    let page = f.store.trace_runs(f.session.id, None, 1).await.unwrap();
    assert_eq!(page.runs[0].id, f.claimed.run.id); // 执行中的 next 不显示
    assert!(page.next_after.is_none());
    assert_eq!(page.title_calls.len(), 1);
    assert!(f
        .kernel
        .call_input(page.title_calls[0].id)
        .await
        .unwrap()
        .is_none());
    f.store
        .finish_run(next.run.id, RunState::Interrupted, None, now + 1)
        .await
        .unwrap();
    let page = f.store.trace_runs(f.session.id, None, 1).await.unwrap();
    assert_eq!(page.runs[0].id, f.claimed.run.id);
    assert_eq!(page.runs[0].calls.len(), 1);
    let later = f
        .store
        .trace_runs(f.session.id, page.next_after, 1)
        .await
        .unwrap();
    assert_eq!(later.runs[0].id, next.run.id);
    assert!(later.runs[0].calls.is_empty());
    assert!(later.title_calls.is_empty() && later.next_after.is_none());
    let sessions = f.store.trace_sessions().await.unwrap();
    assert_eq!(sessions[0].interrupted_runs, 1);
    let board = f.store.dashboard(1).await.unwrap();
    assert_eq!(board.days[0].runs, 2);
    assert_eq!(board.days[0].completed_runs, 1);
    assert_eq!(board.days[0].calls.calls, 2);
    assert_eq!(board.models.len(), 2); // 旁路按 purpose 独立聚合
    let peers = f.store.call_peers().await.unwrap();
    assert_eq!(peers.models.len(), 2);
    assert!(peers
        .models
        .iter()
        .any(|p| p.purpose == ModelCallPurpose::Title && p.median_ms == 100));
}

#[tokio::test]
async fn automatic_compaction_follows_the_reply_that_fills_the_window() {
    let f = Fixture::new().await;
    let mut answer = vec![Ok(ModelEvent::Usage(mic_store::Usage {
        input_tokens: 12,
        output_tokens: 2,
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        reasoning_tokens: None,
    }))];
    answer.extend(reply("answer"));
    let provider = scripted(vec![
        reply("history"),
        reply("manual summary"),
        answer,
        reply("automatic summary"),
    ]);
    let mut exec = f.exec();
    exec.call_model(&provider, &[]).await.unwrap().unwrap();
    let command = f
        .store
        .append_command(
            f.session.id,
            f.owner,
            SessionCommand::Compact {
                text: Some("focus next".into()),
            },
            now_ms(),
        )
        .await
        .unwrap();
    exec.pending_inputs = vec![command.id];
    exec.compact(&provider).await.unwrap().unwrap();
    // 压缩后尚无调用，不触发；回复用量 14 ≥ 15 × 默认 80% 后触发。
    exec.context_window = 15;
    assert_eq!(exec.execute(&provider).await.unwrap(), RunState::Completed);
    let calls = f.verify(&provider).await;
    assert_eq!(
        calls.iter().map(|c| c.purpose).collect::<Vec<_>>(),
        [
            ModelCallPurpose::Conversation,
            ModelCallPurpose::Compaction,
            ModelCallPurpose::Conversation,
            ModelCallPurpose::Compaction
        ]
    );
    let manual = f.store.call_record(calls[1].id).await.unwrap().unwrap();
    assert_eq!(manual.context.instruction, [command.id]);
    let automatic = f.store.call_record(calls[3].id).await.unwrap().unwrap();
    assert!(automatic.context.instruction.is_empty());
    assert_ne!(calls[1].boundary_id, calls[3].boundary_id);
    assert_eq!(f.store.context_usage(f.session.id).await.unwrap(), None);
}

#[tokio::test]
async fn compaction_right_after_tool_call_keeps_the_call_for_its_result() {
    let f = Fixture::new().await;
    let provider = scripted(vec![
        vec![
            Ok(ModelEvent::Usage(mic_store::Usage {
                input_tokens: 12,
                output_tokens: 2,
                cache_read_tokens: Some(0),
                cache_write_tokens: Some(0),
                reasoning_tokens: None,
            })),
            Ok(ModelEvent::Finished(ModelResponse {
                blocks: vec![ReplyBlock::ToolCall {
                    id: "t".into(),
                    name: "timed".into(),
                    args: serde_json::json!({ "delay": 0 }),
                }],
                stop: StopReason::ToolUse,
            })),
        ],
        reply("summary"),
        reply("done"),
    ]);
    let mut settings = f.claimed.settings.clone();
    settings.compaction.keep_percent = 0;
    let mut exec = f.exec();
    exec.settings = &settings;
    exec.tools = vec![ToolHandle::new(TimedTool)];
    exec.context_window = 15;
    assert_eq!(exec.execute(&provider).await.unwrap(), RunState::Completed);
    let calls = f.verify(&provider).await;
    assert_eq!(
        calls.iter().map(|c| c.purpose).collect::<Vec<_>>(),
        [
            ModelCallPurpose::Conversation,
            ModelCallPurpose::Compaction,
            ModelCallPurpose::Conversation
        ]
    );
    let last = provider.requests.lock().unwrap().last().unwrap().clone();
    let kinds: Vec<_> = last
        .messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::Reply { .. } => Some("reply"),
            MessageBody::ToolResult { .. } => Some("result"),
            _ => None,
        })
        .collect();
    assert_eq!(kinds, ["reply", "result"]);
}

// 先吐出给定事件，之后一直挂起，模拟卡住的上游。
struct Hanging(Mutex<std::collections::VecDeque<Result<ModelEvent, ProviderError>>>);
struct HangEvents(std::collections::VecDeque<Result<ModelEvent, ProviderError>>);
impl futures_core::Stream for HangEvents {
    type Item = Result<ModelEvent, ProviderError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.0.pop_front() {
            Some(event) => Poll::Ready(Some(event)),
            None => Poll::Pending,
        }
    }
}
impl Provider for Hanging {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(&self, _: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        Box::pin(HangEvents(std::mem::take(&mut *self.0.lock().unwrap())))
    }
}
fn hanging(events: Vec<Result<ModelEvent, ProviderError>>) -> Hanging {
    Hanging(Mutex::new(events.into()))
}

async fn stop_soon(stop: &CancellationToken) {
    tokio::time::sleep(Duration::from_millis(30)).await;
    stop.cancel();
}

#[tokio::test]
async fn stop_mid_stream_keeps_partial_text_only() {
    let f = Fixture::new().await;
    let provider = hanging(vec![
        Ok(ModelEvent::ReasoningDelta("thinking".into())),
        Ok(ModelEvent::TextDelta("half ".into())),
        Ok(ModelEvent::TextDelta("answer".into())),
    ]);
    let mut exec = f.exec();
    let stop = exec.stop.clone();
    let (state, ()) = tokio::join!(exec.execute(&provider), stop_soon(&stop));
    assert_eq!(state.unwrap(), RunState::Stopped);
    let messages = f.store.run_messages(f.claimed.run.id).await.unwrap();
    let replies: Vec<_> = messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::Reply { blocks, .. } => Some(blocks.clone()),
            _ => None,
        })
        .collect();
    assert_eq!(
        replies,
        [vec![ReplyBlock::Text {
            text: "half answer".into(),
            phase: None
        }]]
    );
    f.store
        .finish_run(f.claimed.run.id, RunState::Stopped, None, now_ms())
        .await
        .unwrap();
    let page = f.store.trace_runs(f.session.id, None, 10).await.unwrap();
    assert_eq!(page.runs[0].state, "stopped");
    assert_eq!(page.runs[0].calls[0].stop, Some(StopReason::Stopped));
    assert!(page.runs[0].calls[0].usage.is_none());
}

#[tokio::test]
async fn stop_mid_tools_keeps_finished_results_and_cancels_the_rest() {
    let f = Fixture::new().await;
    let tool_block = |id: &str, delay| ReplyBlock::ToolCall {
        id: id.into(),
        name: "timed".into(),
        args: serde_json::json!({ "delay": delay }),
    };
    let provider = scripted(vec![vec![Ok(ModelEvent::Finished(ModelResponse {
        blocks: vec![
            tool_block("slow", 60_000),
            tool_block("fast", 1),
            ReplyBlock::ToolCall {
                id: "bg".into(),
                name: "hang".into(),
                args: serde_json::json!({}),
            },
        ],
        stop: StopReason::ToolUse,
    }))]]);
    let mut exec = f.exec();
    exec.tools = vec![
        ToolHandle::new(TimedTool),
        ToolHandle::new(HangAfterDispatch),
    ];
    let stop = exec.stop.clone();
    let (state, ()) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(5), exec.execute(&provider)),
        stop_soon(&stop)
    );
    assert_eq!(state.expect("停止应立即生效").unwrap(), RunState::Stopped);
    let messages = f.store.run_messages(f.claimed.run.id).await.unwrap();
    let results: Vec<_> = messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::ToolResult {
                tool_call_id,
                outcome: ToolResultOutcome::Terminal(outcome),
                started_at,
                ..
            } => Some((tool_call_id.as_str(), outcome.clone(), started_at.is_some())),
            _ => None,
        })
        .collect();
    assert_eq!(results.len(), 2);
    assert!(matches!(
        results[0],
        ("fast", ExecOutcome::Completed { .. }, true)
    ));
    assert!(matches!(
        &results[1],
        ("slow", ExecOutcome::Cancelled { message }, true) if message == STOPPED_TOOL
    ));
    // 已提交的派出一定落 Dispatched，任务本身不受停止影响。
    let job = f
        .store
        .job_by_origin(f.claimed.run.id, "bg")
        .await
        .unwrap()
        .unwrap();
    assert!(messages.iter().any(|m| matches!(
        &m.body,
        MessageBody::ToolResult { tool_call_id, outcome: ToolResultOutcome::Dispatched { exec_id }, .. }
            if tool_call_id == "bg" && *exec_id == job.0.to_string()
    )));
    assert_eq!(
        f.store
            .job(job)
            .await
            .unwrap()
            .unwrap()
            .tool_state()
            .unwrap(),
        mic_store::JobState::Running
    );
    f.engine.jobs.shutdown().await;
}

#[tokio::test]
async fn stop_during_retry_wait_or_compaction_ends_at_once() {
    let f = Fixture::new().await;
    let provider = scripted(vec![vec![Err(ProviderError::Transient {
        message: "busy".into(),
        retry_after: Some(Duration::from_secs(60)),
    })]]);
    let mut exec = f.exec();
    let stop = exec.stop.clone();
    let (state, ()) = tokio::join!(
        tokio::time::timeout(Duration::from_secs(5), exec.execute(&provider)),
        stop_soon(&stop)
    );
    assert_eq!(state.expect("停止应立即生效").unwrap(), RunState::Stopped);

    let f = Fixture::new().await;
    let exec = f.exec();
    let stop = exec.stop.clone();
    let provider = hanging(vec![Ok(ModelEvent::TextDelta("partial summary".into()))]);
    let (state, ()) = tokio::join!(exec.compact(&provider), stop_soon(&stop));
    assert_eq!(state.unwrap(), Err(RunState::Stopped));
    let messages = f.store.run_messages(f.claimed.run.id).await.unwrap();
    assert!(!messages
        .iter()
        .any(|m| matches!(m.body, MessageBody::Boundary { .. })));
}

#[tokio::test]
async fn stopped_run_leaves_queued_input_for_the_next_run() {
    let f = Fixture::new().await;
    let provider = scripted(vec![reply("done")]);
    let mut exec = f.exec();
    exec.call_model(&provider, &[]).await.unwrap().unwrap();
    f.store
        .append_input(
            f.session.id,
            f.owner,
            vec![NewInputPart::Text("queued".into())],
            InputDisposition::Pending,
            None,
            now_ms(),
        )
        .await
        .unwrap();
    exec.stop.cancel();
    assert!(!exec.absorb().await.unwrap());
    assert_eq!(exec.execute(&provider).await.unwrap(), RunState::Stopped);
    f.store
        .finish_run(f.claimed.run.id, RunState::Stopped, None, now_ms())
        .await
        .unwrap();
    let next = f.store.claim_next(f.session.id, now_ms()).await.unwrap();
    assert!(matches!(
        next.map(|c| c.input),
        Some(RunInput::Conversation { message_ids }) if message_ids.len() == 1
    ));
    assert_eq!(
        f.kernel.stop_run(f.session.id, None).await.unwrap(),
        crate::StopOutcome::NotRunning
    );
}

#[tokio::test]
async fn text_only_model_sees_image_placeholders() {
    let f = Fixture::new().await;
    let image = mic_message::ImageData {
        format: mic_message::ImageFormat::Png,
        bytes: Arc::from(&b"png"[..]),
    };
    f.store
        .append_input(
            f.session.id,
            f.owner,
            vec![NewInputPart::Image(image)],
            InputDisposition::Held,
            None,
            now_ms(),
        )
        .await
        .unwrap();
    let mut exec = f.exec();
    let parts = |w: &mic_store::ContextWindow| match &w.messages.last().unwrap().body {
        MessageBody::UserInput { parts, .. } => parts.clone(),
        _ => unreachable!(),
    };
    let mut window = f.store.context_window(f.session.id).await.unwrap();
    assert_eq!(exec.window_images(&mut window).await.unwrap().len(), 1);
    exec.images = false;
    let mut window = f.store.context_window(f.session.id).await.unwrap();
    assert!(exec.window_images(&mut window).await.unwrap().is_empty());
    assert!(matches!(
        &parts(&window)[..],
        [mic_message::ContentPart::Text { text }] if text.contains("不支持查看")
    ));
}

struct BigTool;
impl mic_tool::Tool for BigTool {
    type Args = HashMap<String, usize>;
    fn name(&self) -> &str {
        "big"
    }
    fn description(&self) -> &str {
        "test output size"
    }
    fn prompt_hint(&self) -> Option<&str> {
        None
    }
    async fn execute(
        &self,
        args: Self::Args,
        _: &ToolContext,
    ) -> Result<mic_tool::ToolOutput, mic_tool::ToolError> {
        Ok(mic_tool::ToolOutput::Completed(vec![
            mic_message::ContentPart::Text {
                text: "x".repeat(args["n"]),
            },
        ]))
    }
}

fn call_big(id: &str) -> Vec<Result<ModelEvent, ProviderError>> {
    vec![Ok(ModelEvent::Finished(ModelResponse {
        blocks: vec![ReplyBlock::ToolCall {
            id: id.into(),
            name: "big".into(),
            args: serde_json::json!({ "n": 9000 }),
        }],
        stop: StopReason::ToolUse,
    }))]
}

#[tokio::test]
async fn compaction_elides_old_outputs_quotes_inputs_and_keeps_elision_across_compactions() {
    let f = Fixture::new().await;
    let provider = scripted(vec![
        call_big("a"),
        call_big("b"),
        reply("done"),
        failed(),
        reply("summary"),
        reply("summary 2"),
    ]);
    let mut exec = f.exec();
    exec.tools = vec![ToolHandle::new(BigTool)];
    assert_eq!(exec.execute(&provider).await.unwrap(), RunState::Completed);
    exec.pending_inputs.clear();
    exec.compact(&provider).await.unwrap().unwrap();

    let window = f.store.context_window(f.session.id).await.unwrap();
    let results: Vec<_> = window
        .messages
        .iter()
        .filter(|m| matches!(m.body, MessageBody::ToolResult { .. }))
        .collect();
    assert_eq!(results.len(), 2);
    for m in &results {
        let Some(mic_message::ModelView::Tool { output, .. }) = m.model_view() else {
            panic!()
        };
        assert!(
            matches!(&output[..], [mic_message::ContentPart::Text { text }] if text.contains("omitted at compaction (9000 chars)"))
        );
    }
    let ids: Vec<_> = results.iter().map(|m| m.id).collect();
    assert_eq!(window.quoted.len(), 1);
    let view = f.store.context_view(f.session.id).await.unwrap();
    assert_eq!(view.compactions[0].result.elided, ids);

    exec.compact(&provider).await.unwrap().unwrap();
    let view = f.store.context_view(f.session.id).await.unwrap();
    assert_eq!(view.compactions[1].result.elided, ids);
    assert!(view.compactions[1].result.quoted.is_empty());

    let calls = f.verify(&provider).await;
    let elide = |i: usize| {
        let id = calls[i].id;
        let store = f.store.clone();
        async move { store.call_record(id).await.unwrap().unwrap().context.elide }
    };
    assert_eq!(elide(3).await, ids, "失败的压缩调用同样记下");
    assert_eq!(elide(4).await, ids);
    assert!(elide(5).await.is_empty(), "沿用的替换不再要求提炼");
}
