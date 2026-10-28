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
            next_run_at: 100,
            at: 0,
        })
        .await
        .unwrap();
    let session = s.session_id;
    let title = store.session_summary(session).await.unwrap().unwrap().title;
    assert_eq!(title.as_deref(), Some("早报"), "会话标题即任务名");

    // next_run_at 已不是该时刻（被改动）则不触发。
    assert!(matches!(
        store.fire_schedule(s.id, 99, None, 100).await.unwrap(),
        FireResult::Gone
    ));
    let FireResult::Fired(fired) = store.fire_schedule(s.id, 100, None, 100).await.unwrap() else {
        panic!("空闲会话应触发");
    };
    let disabled = store.schedule(s.id).await.unwrap().unwrap();
    assert!(!disabled.enabled(), "一次性触发后停用");
    let user = MessageBody::UserInput {
        person: owner,
        parts: vec![ContentPart::Text {
            text: "顺便查天气".into(),
        }],
    };
    store.append(session, None, user, 101).await.unwrap();

    // Trigger 独占认领，但其 run 吸收任何人的后续输入。
    let trigger = store.claim_next(session, 104).await.unwrap().unwrap();
    let RunInput::Conversation { message_ids } = &trigger.input else {
        panic!("Trigger 是对话输入");
    };
    assert_eq!(message_ids, &[fired.id]);
    assert_eq!(store.absorb(trigger.run.id).await.unwrap().len(), 1);
    let fire = store.fire_of_run(trigger.run.id).await.unwrap().unwrap();
    assert_eq!(fire.outcome, FireOutcome::Pending);

    // 执行中再到点只记「忙」。
    let enable = ScheduleEdit {
        next_run_at: Some(Some(106)),
        ..Default::default()
    };
    let enabled = store
        .update_schedule(s.id, enable, 105)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(enabled.enabled_at, 105, "重新启用记启用时刻");
    assert!(matches!(
        store
            .fire_schedule(s.id, 106, Some(166), 106)
            .await
            .unwrap(),
        FireResult::Busy
    ));
    let missed = store
        .record_missed(s.id, 166, 286, 3, Some(346))
        .await
        .unwrap()
        .unwrap();
    assert_eq!(missed.outcome, FireOutcome::Missed { count: 3 });
    let after = store.schedule(s.id).await.unwrap().unwrap();
    assert_eq!(after.next_run_at, Some(346), "触发与错过都前移下次时刻");

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
    assert_eq!(
        outcomes,
        [
            FireOutcome::Missed { count: 3 },
            FireOutcome::Busy,
            FireOutcome::Failed
        ]
    );
}
