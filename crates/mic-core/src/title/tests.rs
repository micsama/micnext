use super::*;

#[tokio::test]
async fn held_first_text_is_eligible_but_commands_old_text_and_failed_first_are_not_retried() {
    use crate::{IncomingPart, Kernel};
    use mic_store::{InputDisposition, NewInputPart, NewSession, SessionKind, ToolScope};
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("test", 0).await.unwrap();
    let factories = Arc::new(Factories::default());
    let events = Events::new();
    let (handle, runtime) = TitleRuntime::new(store.clone(), factories.clone(), events.clone());
    let (wake, _receiver) = mpsc::channel(16);
    let engine = Arc::new(crate::run::Engine {
        jobs: crate::jobs::Jobs::scratch(&store, &events),
        store: store.clone(),
        events: events.clone(),
        factories: factories.clone(),
        tools: vec![],
        channel_prompts: Arc::default(),
        control: Default::default(),
        schedules: None,
    });
    let kernel = Kernel::new(
        owner,
        wake,
        Arc::new(HashMap::new()),
        handle.clone(),
        engine,
    );
    let make = |chat: &str| NewSession {
        kind: SessionKind::Root {
            channel: "web".into(),
            chat: chat.into(),
        },
        parent_session_id: None,
        delivery_target: None,
        pwd: "/tmp".into(),
        tool_scope: ToolScope::All,
        created_at: 0,
        persona: None,
        model: None,
        reasoning_effort: None,
    };
    let held = store.create_session(make("held")).await.unwrap();
    kernel
        .append_session_command(held.id, owner, mic_message::SessionCommand::Clear)
        .await
        .unwrap();
    assert!(!handle.generating(held.id));
    kernel
        .append_recorded_input(
            held.id,
            owner,
            vec![IncomingPart::Text("first held".into())],
            "test",
            "held".into(),
        )
        .await
        .unwrap();
    assert!(handle.generating(held.id));
    let old = store.create_session(make("old")).await.unwrap();
    store
        .append_input(
            old.id,
            owner,
            vec![NewInputPart::Text("old".into())],
            InputDisposition::Held,
            None,
            1,
        )
        .await
        .unwrap();
    kernel
        .append_user_input(old.id, owner, vec![IncomingPart::Text("new".into())])
        .await
        .unwrap();
    assert!(!handle.generating(old.id));
    // A missing model finishes the first job with a visible error.
    let task = tokio::spawn(runtime.run());
    tokio::time::timeout(std::time::Duration::from_secs(2), async {
        while handle.generating(held.id) {
            tokio::task::yield_now().await;
        }
    })
    .await
    .unwrap();
    assert!(store
        .session_summary(held.id)
        .await
        .unwrap()
        .unwrap()
        .title_error
        .is_some());
    kernel
        .append_user_input(
            held.id,
            owner,
            vec![IncomingPart::Text("second text".into())],
        )
        .await
        .unwrap();
    assert!(!handle.generating(held.id));
    task.abort();
    let _ = task.await;
}

struct Scripted {
    output: &'static str,
    requests: Arc<Mutex<Vec<ModelRequest>>>,
}

impl crate::Provider for Scripted {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(
        &self,
        req: ModelRequest,
    ) -> crate::BoxStream<Result<crate::ModelEvent, ProviderError>> {
        self.requests.lock().unwrap().push(req);
        Box::pin(Once(Some(Ok(crate::ModelEvent::Finished(ModelResponse {
            blocks: vec![ReplyBlock::Text {
                text: self.output.into(),
                phase: None,
            }],
            stop: StopReason::EndTurn,
        })))))
    }
}

struct Once(Option<Result<crate::ModelEvent, ProviderError>>);

impl futures_core::Stream for Once {
    type Item = Result<crate::ModelEvent, ProviderError>;
    fn poll_next(
        mut self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(self.0.take())
    }
}

struct Factory(&'static str, Arc<Mutex<Vec<ModelRequest>>>);

impl crate::ProviderFactory for Factory {
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
    fn reasoning_options(
        &self,
        _: &str,
        _: &str,
    ) -> Result<Option<crate::ReasoningOptions>, ConfigError> {
        Ok(None)
    }
    fn build(
        &self,
        _: &str,
        _: Option<crate::SecretValue>,
        _: &str,
        _: &str,
        _: Option<mic_store::ReasoningEffort>,
    ) -> Result<Arc<dyn crate::Provider>, ConfigError> {
        Ok(Arc::new(Scripted {
            output: self.0,
            requests: self.1.clone(),
        }))
    }
    fn list_models(
        &self,
        _: &str,
        _: Option<crate::SecretValue>,
    ) -> crate::BoxFuture<Result<Vec<String>, crate::ProbeError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

/// 取材只认收尾回复正文；生成时的输入与 trace 重建逐字一致；原文存为旁路且不进聊天。
#[tokio::test]
async fn manual_title_records_sources_and_side_output() {
    use mic_message::AssistantPhase;
    use mic_store::{
        CredentialWrite, EndpointWrite, ModelWrite, NewModelCall, NewSession, SessionKind,
        TitleModelSelection, ToolScope,
    };
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("test", 0).await.unwrap();
    let requests = Arc::new(Mutex::new(Vec::new()));
    let mut factories = Factories::default();
    factories
        .insert(Box::new(Factory("两行\n标题", requests.clone())))
        .unwrap();
    let factories = Arc::new(factories);
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
    store
        .set_title_model(TitleModelSelection {
            model_id: Some(model),
            reasoning_effort: None,
        })
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: "web".into(),
                chat: "title".into(),
            },
            parent_session_id: None,
            delivery_target: None,
            pwd: "/tmp".into(),
            tool_scope: ToolScope::All,
            created_at: 0,
            persona: None,
            model: None,
            reasoning_effort: None,
        })
        .await
        .unwrap();
    let input = |text: &str| {
        store.append(
            session.id,
            None,
            MessageBody::UserInput {
                person: owner,
                parts: vec![ContentPart::Text { text: text.into() }],
            },
            0,
        )
    };
    let reply = |blocks: Vec<ReplyBlock>| {
        store.record_model_call(NewModelCall {
            context_at: 0,
            session_id: session.id,
            run_id: None,
            purpose: ModelCallPurpose::Conversation,
            model: "test".into(),
            model_id: Some(model),
            reasoning_effort: None,
            context: Default::default(),
            started_at: 0,
            first_output_at: None,
            finished_at: 0,
            stop: Some(StopReason::EndTurn),
            usage: None,
            outcome: ModelCallOutcome::Replied { blocks },
        })
    };
    let text = |text: &str, phase| ReplyBlock::Text {
        text: text.into(),
        phase,
    };
    let first = input("问题一").await.unwrap();
    let plain = reply(vec![text("无阶段的回答", None)])
        .await
        .unwrap()
        .1
        .unwrap();
    input("问题二").await.unwrap();
    let long = "长".repeat(600);
    let answer = reply(vec![
        text("过程说明", Some(AssistantPhase::Commentary)),
        text(&long, Some(AssistantPhase::FinalAnswer)),
    ])
    .await
    .unwrap()
    .1
    .unwrap();
    reply(vec![
        text("调用工具前", None),
        ReplyBlock::ToolCall {
            id: "c".into(),
            name: "t".into(),
            args: serde_json::json!({}),
        },
    ])
    .await
    .unwrap();
    reply(vec![text("只有过程", Some(AssistantPhase::Commentary))])
        .await
        .unwrap();
    let last = input("问题三").await.unwrap();

    let sources = store
        .title_sources(
            session.id,
            std::num::NonZeroU32::new(8).unwrap(),
            crate::limits::TITLE_REPLIES,
        )
        .await
        .unwrap();
    let ids: Vec<_> = sources.iter().map(|m| m.id).collect();
    assert_eq!(ids.len(), 5);
    assert_eq!(
        (ids[0], ids[1], ids[3], ids[4]),
        (first.id, plain.id, answer.id, last.id)
    );
    let messages = title_messages(session.id, &sources);
    assert_eq!(messages.len(), 6);
    assert!(matches!(&messages[0].body, MessageBody::UserInput { .. }));
    assert!(
        matches!(&messages[1].body, MessageBody::Reply { blocks, .. }
        if matches!(&blocks[..], [ReplyBlock::Text { text, .. }] if text == "无阶段的回答"))
    );
    assert!(
        matches!(&messages[3].body, MessageBody::Reply { blocks, .. }
        if matches!(&blocks[..], [ReplyBlock::Text { text, .. }] if text == &"长".repeat(TITLE_REPLY_CHARS)))
    );
    assert!(
        matches!(&messages[5].body, MessageBody::HarnessNote { text }
        if text.starts_with("为以上对话生成简短标题"))
    );

    generate(
        &store,
        &factories,
        &Job {
            session: session.clone(),
            sources,
            stop: tokio_util::sync::CancellationToken::new(),
            _done: tokio::sync::watch::channel(()).0,
        },
    )
    .await
    .unwrap();
    let summary = store.session_summary(session.id).await.unwrap().unwrap();
    assert!(summary.title.is_none() && summary.title_error.is_some());
    let page = store.trace_runs(session.id, None, 10).await.unwrap();
    let call = &page.title_calls[0];
    let side = store.message(call.side_id.unwrap()).await.unwrap().unwrap();
    assert!(matches!(side.body, MessageBody::SideOutput { ref text, .. } if text == "两行\n标题"));

    let (wake, _receiver) = mpsc::channel(1);
    let engine = Arc::new(crate::run::Engine {
        jobs: crate::jobs::Jobs::scratch(&store, &Events::new()),
        store: store.clone(),
        events: Events::new(),
        factories: factories.clone(),
        tools: vec![],
        channel_prompts: Arc::default(),
        control: Default::default(),
        schedules: None,
    });
    let (handle, _runtime) = TitleRuntime::new(store.clone(), factories, Events::new());
    let kernel = crate::Kernel::new(owner, wake, Arc::new(HashMap::new()), handle, engine);
    let rebuilt = kernel.call_input(call.id).await.unwrap().unwrap();
    let actual = requests.lock().unwrap()[0].messages.clone();
    let bodies = |messages: Vec<Message>| {
        messages
            .into_iter()
            .map(|m| serde_json::to_value(m.body).unwrap())
            .collect::<Vec<_>>()
    };
    assert_eq!(bodies(rebuilt), bodies(actual));

    let chat = store.messages_after(session.id, None).await.unwrap();
    assert!(chat
        .iter()
        .all(|m| !matches!(m.body, MessageBody::SideOutput { .. })));
    let page = store
        .messages_page(
            session.id,
            None,
            std::num::NonZeroU32::new(100).unwrap(),
            std::num::NonZeroU32::new(100).unwrap(),
        )
        .await
        .unwrap();
    assert!(page
        .messages
        .iter()
        .all(|m| !matches!(m.body, MessageBody::SideOutput { .. })));
}
