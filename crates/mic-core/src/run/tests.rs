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
    BoxFuture, BoxStream, ConfigError, ProbeError, ProviderFactory, ReasoningOptions, SecretValue,
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
                usage: None,
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
    let engine = Arc::new(Engine {
        store: store.clone(),
        events: Events::new(),
        factories: Arc::new(factories),
        tools: Vec::new(),
        channel_prompts: Arc::new(HashMap::new()),
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
            .run(&task_session, "web", claimed)
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
        engine.run(&session, "web", claimed).await.unwrap(),
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
        engine.run(&session, "web", claimed).await.unwrap(),
        RunState::Completed
    );
    assert_eq!(
        *capture.efforts.lock().unwrap(),
        [Some(E::High), Some(E::High), Some(E::None), Some(E::Low)]
    );
    assert_eq!(capture.builds.load(Ordering::Relaxed), 3);
}
