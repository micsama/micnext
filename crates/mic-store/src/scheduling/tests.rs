use mic_message::{ContentPart, MessageBody, PersonId, SessionId};

use crate::{NewSession, RunState, SessionKind, Store, ToolScope};

#[test]
fn queued_inputs_follow_claims_and_holds() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check());
}

fn input(person: PersonId, text: &str) -> MessageBody {
    MessageBody::UserInput {
        person,
        parts: vec![ContentPart::Text { text: text.into() }],
    }
}

async fn check() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("owner", 0).await.unwrap();
    let session: SessionId = store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: "web".into(),
                chat: "queue".into(),
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
        .unwrap()
        .id;

    let first = store
        .append(session, None, input(owner, "一"), 1)
        .await
        .unwrap()
        .id;
    assert_eq!(store.queued_inputs(session).await.unwrap(), vec![first]);

    let run = store.claim_next(session, 2).await.unwrap().unwrap().run.id;
    assert!(
        store.queued_inputs(session).await.unwrap().is_empty(),
        "认领即出队"
    );

    let second = store
        .append(session, None, input(owner, "二"), 3)
        .await
        .unwrap()
        .id;
    assert_eq!(store.queued_inputs(session).await.unwrap(), vec![second]);
    assert_eq!(store.absorb(run).await.unwrap(), vec![second]);
    assert!(
        store.queued_inputs(session).await.unwrap().is_empty(),
        "并入即出队"
    );

    let third = store
        .append(session, None, input(owner, "三"), 4)
        .await
        .unwrap()
        .id;
    store
        .finish_run(run, RunState::Stopped, None, 5)
        .await
        .unwrap();
    assert_eq!(
        store.queued_inputs(session).await.unwrap(),
        vec![third],
        "停止后未认领的仍排队"
    );

    store.hold_unclaimed_inputs(6).await.unwrap();
    assert!(
        store.queued_inputs(session).await.unwrap().is_empty(),
        "重启遗留转为 held，不算排队"
    );
}
