use std::num::NonZeroU32;

use mic_message::{ContentPart, MessageBody, SessionId};

use crate::{NewSession, SessionKind, Store, StoreError, ToolScope};

#[test]
fn unread_and_pins_follow_watermark_and_archive() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check());
}

async fn root(store: &Store, channel: &str, chat: &str) -> SessionId {
    let id = store
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
        .id;
    let person = store.ensure_person("owner", 0).await.unwrap();
    let input = MessageBody::UserInput {
        person,
        parts: vec![ContentPart::Text { text: "hi".into() }],
    };
    store.append(id, None, input, 1).await.unwrap();
    id
}

async fn notify(store: &Store, id: SessionId) -> mic_message::MessageId {
    let body = MessageBody::Notification {
        source: "t".into(),
        text: "到点了".into(),
    };
    store.append(id, None, body, 2).await.unwrap().id
}

async fn listed(store: &Store, channel: &str) -> Vec<SessionId> {
    let page = store
        .list_root_sessions(channel, None, NonZeroU32::new(50).unwrap())
        .await
        .unwrap();
    page.items.into_iter().map(|s| s.session.id).collect()
}

async fn unread(store: &Store, id: SessionId) -> bool {
    store.session_summary(id).await.unwrap().unwrap().unread
}

async fn check() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let web = root(&store, "web", "a").await;
    let wechat = root(&store, "wechat", "u").await;
    let other = root(&store, "web", "b").await;

    // 用户输入不算未读；只有 Web Root 计未读。
    assert!(!unread(&store, web).await);
    let first = notify(&store, web).await;
    let second = notify(&store, web).await;
    notify(&store, wechat).await;
    assert!(unread(&store, web).await);
    assert!(!unread(&store, wechat).await);

    // 水位只前移，返回值反映是否前移。
    assert!(store.mark_session_seen(web, second).await.unwrap());
    assert!(!unread(&store, web).await);
    assert!(!store.mark_session_seen(web, first).await.unwrap());
    assert!(!unread(&store, web).await);
    let other_msg = notify(&store, other).await;
    assert!(matches!(
        store.mark_session_seen(web, other_msg).await,
        Err(StoreError::MessageNotInSession)
    ));

    // 置顶跨渠道，不进普通列表；重复置顶保留原时间。
    store.set_session_pinned(wechat, Some(10)).await.unwrap();
    store.set_session_pinned(web, Some(20)).await.unwrap();
    store.set_session_pinned(wechat, Some(30)).await.unwrap();
    let pinned: Vec<_> = store
        .list_pinned_sessions()
        .await
        .unwrap()
        .into_iter()
        .map(|s| (s.session.id, s.pinned_at))
        .collect();
    assert_eq!(pinned, vec![(web, Some(20)), (wechat, Some(10))]);
    assert_eq!(listed(&store, "web").await, vec![other]);
    assert!(listed(&store, "wechat").await.is_empty());

    store.set_session_pinned(web, None).await.unwrap();
    assert_eq!(listed(&store, "web").await.len(), 2);

    // 归档清掉置顶，归档后不能再置顶。
    store.archive_session(wechat, 40).await.unwrap();
    assert!(store.list_pinned_sessions().await.unwrap().is_empty());
    assert!(matches!(
        store.set_session_pinned(wechat, Some(50)).await,
        Err(StoreError::SessionArchived)
    ));
}
