use std::num::NonZeroU32;

use mic_message::{Message, MessageBody, MessageId, PersonId, SessionId};

use crate::messages::cut_page;

/// `u` = UserInput，`.` = 其它；按 id 升序，返回倒序的末尾 `cap + 1` 条。
fn newest_first(shape: &str, cap: u32) -> Vec<Message> {
    let mut v: Vec<Message> = shape
        .chars()
        .enumerate()
        .map(|(i, c)| Message {
            id: MessageId(i as i64 + 1),
            session_id: SessionId(1),
            body: if c == 'u' {
                MessageBody::UserInput {
                    person: PersonId(1),
                    parts: Vec::new(),
                }
            } else {
                MessageBody::HarnessNote {
                    text: String::new(),
                }
            },
            created_at: 0,
            delivered_at: None,
        })
        .collect();
    v.reverse();
    v.truncate(cap as usize + 1);
    v
}

fn page(shape: &str, rounds: u32, cap: u32) -> (Vec<i64>, bool) {
    let p = cut_page(
        newest_first(shape, cap),
        NonZeroU32::new(rounds).unwrap(),
        NonZeroU32::new(cap).unwrap(),
    );
    (p.messages.iter().map(|m| m.id.0).collect(), p.has_earlier)
}

#[test]
fn starts_at_round_head() {
    assert_eq!(page("u..u.u..", 2, 10), (vec![4, 5, 6, 7, 8], true));
}

#[test]
fn cap_cuts_inside_round() {
    assert_eq!(page("u.......", 2, 3), (vec![6, 7, 8], true));
}

#[test]
fn whole_session_when_short() {
    assert_eq!(page(".u.u.", 5, 10), (vec![1, 2, 3, 4, 5], false));
    assert_eq!(page("u..", 1, 3), (vec![1, 2, 3], false));
    assert_eq!(page("..u", 2, 3), (vec![1, 2, 3], false));
}

#[test]
fn earliest_round_exact() {
    assert_eq!(page("u.u.", 2, 10), (vec![1, 2, 3, 4], false));
}

#[test]
fn empty() {
    assert_eq!(page("", 2, 3), (vec![], false));
}

#[test]
fn reasoning_is_per_session_and_fixed_when_claimed() {
    use crate::{
        ClaimedModel, CredentialWrite, EndpointWrite, InputDisposition, ModelWrite, NewInputPart,
        NewSession, ReasoningEffort as E, RunState, SessionKind, Store, ToolScope,
    };
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let store = Store::open_in_memory(&[]).await.unwrap();
            let owner = store.ensure_person("test", 1).await.unwrap();
            let endpoint = store
                .create_endpoint(
                    EndpointWrite {
                        name: "test".into(),
                        kind: "openai".into(),
                        config_json: "{}".into(),
                        credential: CredentialWrite::Clear,
                    },
                    1,
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
                    1,
                )
                .await
                .unwrap();
            let mut sessions = Vec::new();
            for (chat, effort) in [("a", Some(E::High)), ("b", Some(E::None))] {
                let session = store
                    .create_session(NewSession {
                        kind: SessionKind::Root {
                            channel: "web".into(),
                            chat: chat.into(),
                        },
                        parent_session_id: None,
                        delivery_target: None,
                        pwd: "/tmp".into(),
                        tool_scope: ToolScope::All,
                        created_at: 1,
                        persona: None,
                        model: Some(model),
                        reasoning_effort: effort,
                    })
                    .await
                    .unwrap();
                assert_eq!(
                    store
                        .session_summary(session.id)
                        .await
                        .unwrap()
                        .unwrap()
                        .session
                        .reasoning_effort,
                    effort
                );
                sessions.push(session);
            }
            let a = sessions[0].id;
            store
                .append_input(
                    a,
                    owner,
                    vec![NewInputPart::Text("first".into())],
                    InputDisposition::Pending,
                    None,
                    2,
                )
                .await
                .unwrap();
            let claimed = store.claim_next(a, 3).await.unwrap().unwrap();
            store.set_session_model(a, model, None).await.unwrap();
            assert_eq!(claimed.settings.reasoning_effort, Some(E::High));
            assert!(matches!(
                claimed.model,
                ClaimedModel::Selected {
                    reasoning_effort: Some(E::High),
                    ..
                }
            ));
            let run_id = claimed.run.id;
            assert_eq!(
                store
                    .session(sessions[1].id)
                    .await
                    .unwrap()
                    .unwrap()
                    .reasoning_effort,
                Some(E::None)
            );
            store
                .finish_run(run_id, RunState::Completed, None, 4)
                .await
                .unwrap();
            store
                .append_input(
                    a,
                    owner,
                    vec![NewInputPart::Text("next".into())],
                    InputDisposition::Pending,
                    None,
                    5,
                )
                .await
                .unwrap();
            assert_eq!(
                store
                    .claim_next(a, 6)
                    .await
                    .unwrap()
                    .unwrap()
                    .settings
                    .reasoning_effort,
                None
            );
        });
}
