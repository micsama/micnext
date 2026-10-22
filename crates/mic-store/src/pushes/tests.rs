use mic_message::MessageBody;

use crate::{NewPush, NewSession, PushDeliveryState, PushRoute, SessionKind, Store, ToolScope};

#[test]
fn push_records_deliveries_and_unread() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check());
}

async fn root(store: &Store, channel: &str, chat: &str) -> mic_message::SessionId {
    store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: channel.into(),
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
        })
        .await
        .unwrap()
        .id
}

fn push(
    store_person: mic_message::PersonId,
    source: mic_message::SessionId,
    routes: Vec<PushRoute>,
) -> NewPush {
    NewPush {
        person: store_person,
        source_session: source,
        source_run: None,
        source_label: "push from session".into(),
        text: "到点了".into(),
        routes,
    }
}

async fn check() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("owner", 0).await.unwrap();
    let source = root(&store, "web", "a").await;
    let dm = root(&store, "wechat", "u").await;

    let (delivered, notices) = store
        .create_push(
            push(
                owner,
                source,
                vec![PushRoute::Session {
                    channel: "wechat".into(),
                    session: dm,
                }],
            ),
            10,
        )
        .await
        .unwrap();
    assert_eq!(notices.len(), 1);
    assert_eq!(notices[0].session_id, dm);
    assert!(matches!(notices[0].body, MessageBody::Notification { .. }));
    store.mark_delivered(notices[0].id, 11).await.unwrap();

    let (failed, notices) = store
        .create_push(
            push(
                owner,
                source,
                vec![PushRoute::Unavailable {
                    channel: "wechat".into(),
                    reason: "未登录".into(),
                }],
            ),
            20,
        )
        .await
        .unwrap();
    assert!(notices.is_empty());

    let page = store.pushes(owner, None, 10).await.unwrap();
    assert_eq!(page.unread, 2, "不论投递去向都计未读");
    assert_eq!(page.pushes[0].id, failed);
    assert!(matches!(
        page.pushes[0].deliveries[..],
        [ref d] if d.channel == "wechat" && matches!(d.state, PushDeliveryState::Failed { .. })
    ));
    assert!(matches!(
        page.pushes[1].deliveries[0].state,
        PushDeliveryState::Delivered { at: 11 }
    ));

    let older = store.pushes(owner, Some(failed), 1).await.unwrap();
    assert_eq!(older.pushes[0].id, delivered);
    assert!(!older.has_more);

    store.mark_pushes_read(owner, delivered, 30).await.unwrap();
    let page = store.pushes(owner, None, 10).await.unwrap();
    assert_eq!(page.unread, 1);
    assert_eq!(page.pushes[0].read_at, None, "更新的不标");
    assert_eq!(page.pushes[1].read_at, Some(30));

    let (gave_up, notices) = store
        .create_push(
            push(
                owner,
                source,
                vec![PushRoute::Session {
                    channel: "wechat".into(),
                    session: dm,
                }],
            ),
            40,
        )
        .await
        .unwrap();
    store
        .mark_delivery_failed(notices[0].id, "微信请求超时".into())
        .await
        .unwrap();
    let page = store.pushes(owner, None, 1).await.unwrap();
    assert_eq!(page.pushes[0].id, gave_up);
    assert!(
        matches!(
            &page.pushes[0].deliveries[0].state,
            PushDeliveryState::Failed { reason } if reason == "微信请求超时"
        ),
        "Channel 放弃投递即终态"
    );
}
