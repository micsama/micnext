use std::num::NonZeroU32;

use mic_message::{ContentPart, MessageBody, Reasoning, ReplyBlock, SessionId};

use super::*;
use crate::{
    ModelCallContext, ModelCallOutcome, ModelCallPurpose, NewModelCall, NewSession, SessionKind,
    StopReason, TitleOutput, ToolScope,
};

#[test]
fn search_matches_titles_and_dialogue_newest_first() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check());
}

async fn root(store: &Store, chat: &str, at: i64) -> SessionId {
    store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: "web".into(),
                chat: chat.into(),
            },
            parent_session_id: None,
            delivery_target: None,
            pwd: "/tmp".into(),
            tool_scope: ToolScope::All,
            created_at: at,
            persona: None,
            model: None,
            reasoning_effort: None,
        })
        .await
        .unwrap()
        .id
}

async fn say(store: &Store, session: SessionId, text: &str, at: i64) -> MessageId {
    let person = store.ensure_person("owner", 0).await.unwrap();
    let body = MessageBody::UserInput {
        person,
        parts: vec![ContentPart::Text { text: text.into() }],
    };
    store.append(session, None, body, at).await.unwrap().id
}

async fn reply(store: &Store, session: SessionId, blocks: Vec<ReplyBlock>, at: i64) -> MessageId {
    let (_, reply) = store
        .record_model_call(NewModelCall {
            context_at: at,
            purpose: ModelCallPurpose::Conversation,
            context: ModelCallContext::default(),
            session_id: session,
            run_id: None,
            model: "test".into(),
            model_id: None,
            reasoning_effort: None,
            started_at: at,
            first_output_at: None,
            finished_at: at,
            stop: Some(StopReason::EndTurn),
            usage: None,
            outcome: ModelCallOutcome::Replied { blocks },
        })
        .await
        .unwrap();
    reply.unwrap().id
}

fn text(text: &str) -> ReplyBlock {
    ReplyBlock::Text {
        text: text.into(),
        phase: None,
    }
}

async fn search(
    store: &Store,
    query: &str,
    before: Option<SessionCursor>,
    limit: u32,
) -> SearchPage {
    store
        .search_sessions(
            SearchTerms::parse(query).unwrap(),
            before,
            NonZeroU32::new(limit).unwrap(),
        )
        .await
        .unwrap()
}

fn ids(page: &SearchPage) -> Vec<SessionId> {
    page.items.iter().map(|h| h.summary.session.id).collect()
}

async fn check() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let old = root(&store, "old", 0).await;
    say(&store, old, "帮我看看 Rust 的生命周期", 10).await;
    let old_reply = reply(
        &store,
        old,
        vec![text("生命周期标注 'a 表示引用的有效范围")],
        11,
    )
    .await;

    let mid = root(&store, "mid", 0).await;
    say(&store, mid, "微信登录失败", 20).await;
    let thinking = Reasoning::Visible {
        text: "生命周期 rust".into(),
        signature: None,
    };
    reply(
        &store,
        mid,
        vec![
            ReplyBlock::Reasoning(thinking),
            ReplyBlock::ToolCall {
                id: "c".into(),
                name: "exec".into(),
                args: serde_json::json!({ "cmd": "rust 生命周期" }),
            },
        ],
        21,
    )
    .await;

    let new = root(&store, "new", 0).await;
    let asked = say(&store, new, "rust 生命周期再讲一遍", 30).await;
    let title = TitleOutput {
        title: "生命周期复习".into(),
        tags: None,
    };
    store.save_title_result(new, Ok(title), None).await.unwrap();

    // 多词须同在一条消息里；英文不分大小写；思考与工具调用不搜。
    let page = search(&store, "RUST 生命周期", None, 10).await;
    assert_eq!(ids(&page), [new, old]);
    assert_eq!(page.next, None);
    let hit = &page.items[0];
    assert!(!hit.title_hit);
    assert_eq!((hit.hits[0].id, hit.hits[0].from_user), (asked, true));
    let older = &page.items[1];
    assert_eq!(older.hit_count, 1);
    assert!(older.hits[0].text.contains("Rust"));

    // 标题单独命中；回复里的词也算。
    let page = search(&store, "复习", None, 10).await;
    assert_eq!(ids(&page), [new]);
    assert!(page.items[0].title_hit && page.items[0].hits.is_empty());
    let page = search(&store, "有效范围", None, 10).await;
    assert_eq!(page.items[0].hits[0].id, old_reply);
    assert!(!page.items[0].hits[0].from_user);

    // 凑满一页即停，游标接着扫更早的会话。
    let first = search(&store, "生命周期", None, 1).await;
    assert_eq!(ids(&first), [new]);
    let second = search(&store, "生命周期", first.next, 1).await;
    assert_eq!(ids(&second), [old]);
    assert_eq!(second.next, None);
    assert!(search(&store, "不存在的词", None, 10)
        .await
        .items
        .is_empty());
    assert_eq!(SearchTerms::parse("  \n "), None);

    // 定位页：目标到 `before` 之间的全部消息加上之前一页。
    let page = store
        .messages_page_until(
            old,
            old_reply,
            MessageId(i64::MAX),
            NonZeroU32::new(1).unwrap(),
            NonZeroU32::new(10).unwrap(),
        )
        .await
        .unwrap();
    let got: Vec<_> = page.messages.iter().map(|m| m.id).collect();
    assert_eq!(got.last(), Some(&old_reply));
    assert_eq!(got.len(), 2);
    assert!(!page.has_earlier);
}
