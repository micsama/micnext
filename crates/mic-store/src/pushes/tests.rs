use mic_message::MessageBody;

use crate::{
    NewPush, NewSession, PushDeliveryState, PushRoute, SessionKind, Store, ToolScope, WEB_CHANNEL,
};

#[test]
fn push_records_deliveries_and_web_unread() {
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

    let (only_im, notices) = store
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

    let (with_web, notices) = store
        .create_push(
            push(
                owner,
                source,
                vec![
                    PushRoute::Web,
                    PushRoute::Unavailable {
                        channel: "wechat".into(),
                        reason: "未登录".into(),
                    },
                ],
            ),
            20,
        )
        .await
        .unwrap();
    assert!(notices.is_empty());

    let page = store.pushes(owner, None, 10).await.unwrap();
    assert_eq!(page.unread, 1, "只有发往 Web 的计未读");
    assert_eq!(page.pushes[0].id, with_web);
    let states: Vec<_> = page.pushes[0]
        .deliveries
        .iter()
        .map(|d| (d.channel.as_str(), &d.state))
        .collect();
    assert!(matches!(
        states[..],
        [
            (WEB_CHANNEL, PushDeliveryState::Shown),
            ("wechat", PushDeliveryState::Failed { .. })
        ]
    ));
    assert!(matches!(
        page.pushes[1].deliveries[0].state,
        PushDeliveryState::Delivered { at: 11 }
    ));

    let older = store.pushes(owner, Some(with_web), 1).await.unwrap();
    assert_eq!(older.pushes[0].id, only_im);
    assert!(!older.has_more);

    store
        .mark_pushes_read(owner, vec![only_im, with_web], 30)
        .await
        .unwrap();
    let page = store.pushes(owner, None, 10).await.unwrap();
    assert_eq!(page.unread, 0);
    assert_eq!(page.pushes[0].read_at, Some(30));
    assert_eq!(page.pushes[1].read_at, None, "未发往 Web 的不标已读");
}
