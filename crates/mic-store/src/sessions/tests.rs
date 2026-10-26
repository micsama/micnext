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
        .list_root_sessions(channel, None, None, NonZeroU32::new(50).unwrap())
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
        .list_pinned_sessions(None)
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
    assert!(store.list_pinned_sessions(None).await.unwrap().is_empty());
    assert!(matches!(
        store.set_session_pinned(wechat, Some(50)).await,
        Err(StoreError::SessionArchived)
    ));
}

#[test]
fn running_summary_covers_the_whole_run() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let store = Store::open_in_memory(&[]).await.unwrap();
            let id = root(&store, "web", "running").await;
            let run = store.claim_next(id, 3).await.unwrap().unwrap().run;
            notify(&store, id).await;
            let page = store
                .list_root_sessions("web", None, None, NonZeroU32::new(50).unwrap())
                .await
                .unwrap();
            assert!(page.items[0].running);
            store.set_session_pinned(id, Some(4)).await.unwrap();
            assert!(store.list_pinned_sessions(None).await.unwrap()[0].running);
            store
                .finish_run(run.id, crate::RunState::Completed, None, 5)
                .await
                .unwrap();
            let summary = store.session_summary(id).await.unwrap().unwrap();
            assert!(!summary.running);
            assert!(summary.unread);
        });
}

#[test]
fn tags_filter_lists_and_respect_manual_choice() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            use crate::{TagError, TagName, TitleOutput};
            let store = Store::open_in_memory(&[]).await.unwrap();
            let work = store
                .create_tag(TagName::parse(" 工作 ").unwrap(), 0)
                .await
                .unwrap();
            let life = store
                .create_tag(TagName::parse("生活").unwrap(), 0)
                .await
                .unwrap();
            assert!(matches!(
                store.create_tag(TagName::parse("工作").unwrap(), 0).await,
                Err(TagError::NameTaken(_))
            ));
            assert!(matches!(TagName::parse("工 作"), Err(TagError::Separator)));
            let ids = three_roots(&store).await;
            let [a, b, c] = ids[..] else { unreachable!() };
            let auto = |tags| {
                Ok(TitleOutput {
                    title: "t".into(),
                    tags: Some(tags),
                })
            };
            store
                .save_title_result(a, auto(vec![work]), None)
                .await
                .unwrap();
            store
                .save_title_result(b, auto(vec![work, life]), None)
                .await
                .unwrap();
            store
                .save_title_result(c, auto(vec![life]), None)
                .await
                .unwrap();

            // 按标签过滤时分页游标照常工作。
            let first = store
                .list_root_sessions("web", Some(work), None, NonZeroU32::new(1).unwrap())
                .await
                .unwrap();
            assert_eq!(first.items[0].session.id, b);
            assert_eq!(first.items[0].tag_ids, vec![work, life]);
            let rest = store
                .list_root_sessions("web", Some(work), first.next, NonZeroU32::new(1).unwrap())
                .await
                .unwrap();
            assert_eq!(
                rest.items.iter().map(|s| s.session.id).collect::<Vec<_>>(),
                vec![a]
            );
            assert!(rest.next.is_none());

            // 手动定过的标签，之后的生成只改标题。
            store.set_session_tags(c, vec![work]).await.unwrap();
            store
                .save_title_result(c, auto(vec![life]), None)
                .await
                .unwrap();
            assert!(store.title_tag_choices(c).await.unwrap().is_empty());
            let summary = store.session_summary(c).await.unwrap().unwrap();
            assert_eq!(summary.tag_ids, vec![work]);

            // 删除标签连带去掉会话上的标签。
            store.delete_tag(work).await.unwrap();
            assert_eq!(
                store.session_summary(b).await.unwrap().unwrap().tag_ids,
                vec![life]
            );
            assert_eq!(store.tags().await.unwrap()[0].sessions, 1);
        });
}

async fn three_roots(store: &Store) -> Vec<SessionId> {
    let mut ids = Vec::new();
    for chat in ["a", "b", "c"] {
        ids.push(root(store, "web", chat).await);
    }
    ids
}
