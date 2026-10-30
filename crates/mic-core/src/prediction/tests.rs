use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Poll};

use mic_message::SessionCommand;
use mic_store::{
    CredentialWrite, EndpointWrite, InputDisposition, ModelWrite, NewInputPart, NewSession,
    ToolScope, UtilityModelSelection,
};

use super::*;
use crate::{
    BoxFuture, BoxStream, ConfigError, ModelEvent, ProbeError, Provider, ProviderError,
    ProviderFactory, ReasoningOptions, SecretValue,
};

/// 预测调用依次取的输出；`None` 为一直挂起。
type Script = Arc<Mutex<VecDeque<Option<&'static str>>>>;

struct Factory(Script);

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
        Ok(None)
    }
    fn build(
        &self,
        _: &str,
        _: Option<SecretValue>,
        _: &str,
        _: &str,
        _: Option<mic_store::ReasoningEffort>,
    ) -> Result<Arc<dyn Provider>, ConfigError> {
        Ok(Arc::new(Mock(self.0.clone())))
    }
    fn list_models(
        &self,
        _: &str,
        _: Option<SecretValue>,
    ) -> BoxFuture<Result<Vec<String>, ProbeError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct Mock(Script);

struct Reply(Option<Option<Result<ModelEvent, ProviderError>>>);

impl futures_core::Stream for Reply {
    type Item = Result<ModelEvent, ProviderError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match &mut self.0 {
            Some(item) => Poll::Ready(item.take()),
            None => Poll::Pending,
        }
    }
}

impl Provider for Mock {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(&self, req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        let text = if req.cache_key.starts_with("prediction-") {
            match self.0.lock().unwrap().pop_front().expect("预测脚本已用完") {
                Some(text) => text,
                None => return Box::pin(Reply(None)),
            }
        } else {
            "答复"
        };
        Box::pin(Reply(Some(Some(Ok(ModelEvent::Finished(ModelResponse {
            blocks: vec![ReplyBlock::Text {
                text: text.into(),
                phase: None,
            }],
            stop: StopReason::EndTurn,
        }))))))
    }
}

struct Harness {
    store: Store,
    engine: Arc<crate::run::Engine>,
    session: Session,
    owner: mic_message::PersonId,
    clock: i64,
}

impl Harness {
    async fn new(script: Script) -> Self {
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
                    starred: false,
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
        store
            .set_utility_model(UtilityModelSelection {
                model_id: Some(model),
                reasoning_effort: None,
            })
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
                reasoning_effort: None,
            })
            .await
            .unwrap();
        let mut factories = Factories::default();
        factories.insert(Box::new(Factory(script))).unwrap();
        let factories = Arc::new(factories);
        let events = Events::new();
        let (handle, runtime) = PredictionRuntime::new(
            store.clone(),
            factories.clone(),
            events.clone(),
            Arc::new(["web"].into_iter().collect()),
        );
        tokio::spawn(runtime.run());
        let engine = Arc::new(crate::run::Engine {
            jobs: crate::jobs::Jobs::scratch(&store, &events),
            store: store.clone(),
            events,
            factories,
            tools: Vec::new(),
            channel_prompts: Arc::default(),
            control: Default::default(),
            schedules: None,
            predictions: Some(handle),
            skills: crate::skills::Skills::new(Default::default()),
            env: Default::default(),
        });
        Self {
            store,
            engine,
            session,
            owner,
            clock: 1,
        }
    }

    async fn run_next(&mut self) -> RunId {
        self.clock += 1;
        let claimed = self
            .store
            .claim_next(self.session.id, self.clock)
            .await
            .unwrap()
            .unwrap();
        let run = claimed.run.id;
        let slot = self.engine.control.enter(self.session.id).unwrap();
        self.engine
            .run(&self.session, "web", claimed, &slot)
            .await
            .unwrap();
        run
    }

    async fn say(&mut self, text: &str) -> RunId {
        self.clock += 1;
        self.store
            .append_input(
                self.session.id,
                self.owner,
                vec![NewInputPart::Text(text.into())],
                InputDisposition::Pending,
                None,
                self.clock,
            )
            .await
            .unwrap();
        self.run_next().await
    }

    async fn prediction_calls(&self, run: RunId) -> Vec<mic_store::TraceCall> {
        let page = self
            .store
            .trace_runs(self.session.id, None, 20)
            .await
            .unwrap();
        page.runs
            .into_iter()
            .find(|r| r.id == run)
            .unwrap()
            .calls
            .into_iter()
            .filter(|c| c.purpose == ModelCallPurpose::Prediction)
            .collect()
    }

    async fn settled(&self, run: RunId) -> mic_store::TraceCall {
        for _ in 0..200 {
            if let Some(call) = self.prediction_calls(run).await.pop() {
                return call;
            }
            tokio::time::sleep(std::time::Duration::from_millis(5)).await;
        }
        panic!("预测调用没有落盘");
    }
}

/// 正常完成后生成并推送；指令 run 不预测且让候选作废；预测不计入 run 的执行统计。
#[tokio::test]
async fn completed_run_predicts_and_newer_run_invalidates() {
    let script: Script = Arc::new(Mutex::new([Some("好的，继续")].into()));
    let mut h = Harness::new(script).await;
    let mut events = h.engine.events.subscribe();
    let run = h.say("第一句").await;
    let text = loop {
        if let KernelEventKind::PredictionReady { run_id, text } = events.recv().await.unwrap().kind
        {
            assert_eq!(run_id, run);
            break text;
        }
    };
    assert_eq!(text, "好的，继续");
    assert_eq!(
        h.store.current_prediction(h.session.id).await.unwrap(),
        Some(mic_store::Prediction {
            run_id: run,
            text: text.clone()
        })
    );
    let page = h.store.trace_runs(h.session.id, None, 20).await.unwrap();
    let traced = page.runs.iter().find(|r| r.id == run).unwrap();
    assert_eq!(traced.calls.len(), 2);
    assert_eq!(traced.stats.requests, 1);
    assert_eq!(traced.stats.steps, 1);

    h.clock += 1;
    h.store
        .append_command(h.session.id, h.owner, SessionCommand::Clear, h.clock)
        .await
        .unwrap();
    let clear = h.run_next().await;
    assert_eq!(
        h.store.current_prediction(h.session.id).await.unwrap(),
        None
    );
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    assert!(h.prediction_calls(clear).await.is_empty());
}

/// 新 run 开始时取消在途预测并记为失败；超长输出原文可见但不成为候选。
#[tokio::test]
async fn new_run_cancels_and_overlong_output_is_not_a_candidate() {
    let overlong: &'static str = "长".repeat(PREDICTION_CHARS + 1).leak();
    let script: Script = Arc::new(Mutex::new([None, Some(overlong)].into()));
    let mut h = Harness::new(script).await;
    let first = h.say("第一句").await;
    // 等预测发出请求后再开新 run。
    for _ in 0..200 {
        if h.engine
            .predictions
            .as_ref()
            .unwrap()
            .active
            .lock()
            .unwrap()
            .contains_key(&h.session.id)
        {
            break;
        }
        tokio::task::yield_now().await;
    }
    tokio::time::sleep(std::time::Duration::from_millis(20)).await;
    let second = h.say("第二句").await;
    let cancelled = h.settled(first).await;
    assert_eq!(cancelled.error.as_deref(), Some("已取消"));

    let failed = h.settled(second).await;
    assert!(failed.error.is_some());
    let side = h
        .store
        .message(failed.side_id.expect("原文存为旁路输出"))
        .await
        .unwrap()
        .unwrap();
    assert!(matches!(side.body, MessageBody::SideOutput { text, .. } if text == overlong));
    assert_eq!(
        h.store.current_prediction(h.session.id).await.unwrap(),
        None
    );
}

/// 最后一条回复还带工具调用（被工具打断、等用户）时不预测。
#[test]
fn tool_interrupted_run_has_no_sources() {
    let message = |id, body| Message {
        id: mic_message::MessageId(id),
        session_id: SessionId(1),
        body,
        created_at: 0,
        delivered_at: None,
    };
    let input = message(
        1,
        MessageBody::UserInput {
            person: mic_message::PersonId(1),
            parts: vec![ContentPart::Text { text: "问".into() }],
        },
    );
    let reply = |id, blocks| {
        message(
            id,
            MessageBody::Reply {
                model: "test".into(),
                blocks,
                stats: mic_message::ReplyStats {
                    model_ms: 0,
                    first_output_ms: None,
                    usage: None,
                },
            },
        )
    };
    let asked = reply(
        2,
        vec![ReplyBlock::ToolCall {
            id: "c".into(),
            name: crate::ASK_USER.into(),
            args: serde_json::json!({}),
        }],
    );
    assert!(sources(vec![input.clone(), asked]).is_none());
    let answered = reply(
        2,
        vec![ReplyBlock::Text {
            text: "答".into(),
            phase: None,
        }],
    );
    assert_eq!(sources(vec![input, answered]).unwrap().len(), 2);
}
