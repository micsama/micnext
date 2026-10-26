use super::*;

fn summary(kind: SessionKind) -> SessionSummary {
    SessionSummary {
        session: Session {
            id: SessionId(1),
            kind,
            parent_session_id: None,
            delivery_target: None,
            pwd: "/tmp".into(),
            tool_scope: ToolScope::All,
            created_at: 0,
            persona_id: PersonaId(1),
            model_id: None,
            reasoning_effort: None,
            archived_at: None,
            plain_after: None,
        },
        title: None,
        title_error: None,
        last_activity_at: 0,
        preview: None,
        pinned_at: None,
        unread: false,
        running: false,
        tag_ids: Vec::new(),
    }
}

#[test]
fn session_source_keeps_channels_and_triggers_distinct() {
    for channel in ["web", "wechat", "schedule"] {
        let item = item(summary(SessionKind::Root {
            channel: channel.into(),
            chat: "a".into(),
        }))
        .unwrap();
        let encoded = serde_json::to_value(&item).unwrap();
        assert_eq!(
            encoded["source"],
            serde_json::json!({"kind": "channel", "channel": channel})
        );
        assert_eq!(item.writable, channel == "web");
        assert!(encoded.get("channel").is_none());
    }
    let scheduled = item(summary(SessionKind::Triggered {
        module: mic_store::SCHEDULE_MODULE.into(),
        ref_id: "deleted-task".into(),
    }))
    .unwrap();
    assert!(matches!(scheduled.source, SessionSource::Schedule));
    assert!(scheduled.schedule.is_none());
    assert!(scheduled.writable);
    assert!(item(summary(SessionKind::Triggered {
        module: "other".into(),
        ref_id: "a".into()
    }))
    .is_none());
    assert!(item(summary(SessionKind::Task)).is_none());
}
