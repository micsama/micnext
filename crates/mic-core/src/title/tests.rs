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
    let kernel = Kernel::new(
        store.clone(),
        owner,
        events,
        wake,
        factories,
        Arc::new(HashMap::new()),
        handle.clone(),
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
