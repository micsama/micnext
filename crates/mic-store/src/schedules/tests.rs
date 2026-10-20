use mic_message::{ContentPart, MessageBody};

use crate::{FireOutcome, FireResult, NewSchedule, RunInput, RunState, ScheduleEdit, Store};

#[test]
fn fire_claims_exclusive_runs_and_derives_outcome() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(check());
}

async fn check() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("owner", 0).await.unwrap();
    let persona = store.personas().await.unwrap()[0].id;
    let s = store
        .create_schedule(NewSchedule {
            person: owner,
            name: "早报".into(),
            rule: "2030-01-01 08:00".into(),
            prompt: "汇总新闻".into(),
            pwd: "/tmp".into(),
            persona,
            model: None,
            reasoning_effort: None,
            at: 0,
        })
        .await
        .unwrap();
    let session = s.session_id;
    let found = store.schedule_of_session(session).await.unwrap().unwrap();
    assert_eq!(found.id, s.id);

    let FireResult::Fired(messages) = store
        .fire_schedule(s.id, 100, true, true, 100)
        .await
        .unwrap()
    else {
        panic!("空闲会话应触发");
    };
    assert_eq!(messages.len(), 2);
    let disabled = store.schedule(s.id).await.unwrap().unwrap();
    assert!(!disabled.enabled, "一次性触发后停用");
    let user = MessageBody::UserInput {
        person: owner,
        parts: vec![ContentPart::Text {
            text: "顺便查天气".into(),
        }],
    };
    store.append(session, None, user, 101).await.unwrap();

    // 压缩独占一个 run，不吸收后续输入。
    let compact = store.claim_next(session, 102).await.unwrap().unwrap();
    assert!(matches!(compact.input, RunInput::Command { .. }));
    assert!(store.absorb(compact.run.id).await.unwrap().is_empty());
    store
        .finish_run(compact.run.id, RunState::Completed, None, 103)
        .await
        .unwrap();

    // Trigger 独占认领，但其 run 吸收任何人的后续输入。
    let trigger = store.claim_next(session, 104).await.unwrap().unwrap();
    let RunInput::Conversation { message_ids } = &trigger.input else {
        panic!("Trigger 是对话输入");
    };
    assert_eq!(message_ids, &[messages[1].id]);
    assert_eq!(store.absorb(trigger.run.id).await.unwrap().len(), 1);
    let fire = store.fire_of_run(trigger.run.id).await.unwrap().unwrap();
    assert_eq!(fire.outcome, FireOutcome::Pending);

    // 执行中再到点只记「忙」。
    let enable = ScheduleEdit {
        enabled: Some(true),
        ..Default::default()
    };
    store.update_schedule(s.id, enable, 105).await.unwrap();
    assert!(matches!(
        store
            .fire_schedule(s.id, 106, false, false, 106)
            .await
            .unwrap(),
        FireResult::Busy
    ));

    store
        .finish_run(trigger.run.id, RunState::ProviderFailed, None, 107)
        .await
        .unwrap();
    let outcomes: Vec<_> = store
        .schedule_fires(s.id, 10)
        .await
        .unwrap()
        .iter()
        .map(|f| f.outcome)
        .collect();
    assert_eq!(outcomes, [FireOutcome::Busy, FireOutcome::Failed]);
}
