use mic_message::{MessageBody, ReplyBlock, ReplyStats, ReplyUsage};

use crate::{
    DeliveryTarget, ModelCallContext, ModelCallOutcome, ModelCallPurpose, NewModelCall, NewSession,
    SessionKind, StopReason, Store, ToolScope, Usage,
};

#[test]
fn reply_stats_follow_the_producing_request() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check_reply_stats());
}

async fn check_reply_stats() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let session = store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: "web".into(),
                chat: "stats".into(),
            },
            parent_session_id: None,
            delivery_target: Some(DeliveryTarget {
                channel: "wechat".into(),
                payload: "u".into(),
            }),
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
    let call = NewModelCall {
        context_at: 0,
        purpose: ModelCallPurpose::Conversation,
        context: ModelCallContext::default(),
        session_id: session,
        run_id: None,
        model: "test".into(),
        model_id: None,
        reasoning_effort: None,
        started_at: 100,
        first_output_at: Some(150),
        finished_at: 300,
        stop: Some(StopReason::EndTurn),
        usage: None,
        outcome: ModelCallOutcome::Replied {
            blocks: vec![ReplyBlock::Text {
                text: "正文".into(),
                phase: None,
            }],
        },
    };
    // 失败尝试可有用量，但不属于后来产出回复的请求。
    let (_, failed_reply) = store
        .record_model_call(NewModelCall {
            context_at: 0,
            started_at: 0,
            finished_at: 50,
            usage: Some(Usage {
                input_tokens: 9000,
                output_tokens: 500,
                cache_read_tokens: Some(8000),
                cache_write_tokens: None,
                reasoning_tokens: None,
            }),
            outcome: ModelCallOutcome::Failed {
                error: "retry".into(),
            },
            stop: None,
            ..call.clone()
        })
        .await
        .unwrap();
    assert!(failed_reply.is_none());

    for (i, cache) in [Some(60), Some(0), None].into_iter().enumerate() {
        let usage = Usage {
            input_tokens: 100,
            output_tokens: 20,
            cache_read_tokens: cache,
            cache_write_tokens: None,
            reasoning_tokens: Some(10),
        };
        let (_, reply) = store
            .record_model_call(NewModelCall {
                context_at: 0,
                started_at: 1000 * i as i64 + 100,
                first_output_at: Some(1000 * i as i64 + 150),
                finished_at: 1000 * i as i64 + 300,
                usage: Some(usage),
                ..call.clone()
            })
            .await
            .unwrap();
        let reply = reply.unwrap();
        let expected = ReplyStats {
            model_ms: 200,
            first_output_ms: Some(50),
            usage: Some(ReplyUsage {
                input_tokens: 100,
                output_tokens: 20,
                cache_read_tokens: cache,
            }),
        };
        assert!(matches!(&reply.body, MessageBody::Reply { stats, .. } if stats == &expected));
        assert_eq!(
            store.messages_by_ids(vec![reply.id]).await.unwrap(),
            vec![reply.clone()]
        );
        assert_eq!(
            store
                .messages_page(
                    session,
                    None,
                    std::num::NonZeroU32::new(20).unwrap(),
                    std::num::NonZeroU32::new(100).unwrap()
                )
                .await
                .unwrap()
                .messages
                .last(),
            Some(&reply)
        );
        // 投递查询在消息列后追加目标列，统计列不得错位。
        let pending = store.pending_deliveries("wechat").await.unwrap();
        let last = pending.last().unwrap();
        assert_eq!((&last.message, last.target.payload.as_str()), (&reply, "u"));
        let serialized = serde_json::to_value(&reply).unwrap();
        assert_eq!(serialized["body"]["stats"]["model_ms"], 200);
    }
    // 停止后的部分正文没有用量，实时与回读都保留未知。
    let (_, reply) = store
        .record_model_call(NewModelCall {
            context_at: 0,
            stop: Some(StopReason::Stopped),
            ..call
        })
        .await
        .unwrap();
    let reply = reply.unwrap();
    assert!(matches!(&reply.body, MessageBody::Reply { stats, .. } if stats.usage.is_none()));
    assert_eq!(
        store.messages_by_ids(vec![reply.id]).await.unwrap(),
        vec![reply]
    );
    let duplicated = store.call(move |conn| {
        Ok(conn.query_row("SELECT count(*) FROM core_messages WHERE kind = 'Reply' AND (json_type(payload, '$.stats') IS NOT NULL OR json_type(payload, '$.model') IS NOT NULL)", [], |r| r.get::<_, i64>(0))?)
    }).await.unwrap();
    assert_eq!(duplicated, 0);
}
