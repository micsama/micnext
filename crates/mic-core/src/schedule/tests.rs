use std::collections::HashMap;
use std::future::Future;
use std::sync::Arc;
use std::task::Poll;

use mic_store::{FireOutcome, NewSchedule, Store};
use tokio::sync::mpsc;

use super::Schedules;
use crate::event::Events;
use crate::push::Pushes;

/// 立即执行开始后调用方被取消：触发照常提交并唤醒任务会话，计划不变。
#[tokio::test]
async fn run_now_completes_after_caller_cancels() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    let owner = store.ensure_person("owner", 0).await.unwrap();
    let persona = store.personas().await.unwrap()[0].id;
    let schedule = store
        .create_schedule(NewSchedule {
            person: owner,
            name: "早报".into(),
            rule: "0 8 * * *".into(),
            prompt: "汇总新闻".into(),
            pwd: "/tmp".into(),
            persona,
            model: None,
            reasoning_effort: None,
            next_run_at: i64::MAX,
            at: 0,
        })
        .await
        .unwrap();
    let events = Events::new();
    let pushes = Arc::new(Pushes::new(
        store.clone(),
        events.clone(),
        Arc::new(HashMap::new()),
        owner,
    ));
    let (wake, mut woken) = mpsc::channel(1);
    let schedules = Arc::new(Schedules::new(store.clone(), events, pushes, wake, owner));

    // 首次轮询即派出触发，随后丢弃调用方 future。
    let mut call = Box::pin(schedules.run_now(schedule.id));
    let first = std::future::poll_fn(|cx| Poll::Ready(call.as_mut().poll(cx))).await;
    assert!(first.is_pending(), "触发尚未完成时调用方已被取消");
    drop(call);

    assert_eq!(woken.recv().await, Some(schedule.session_id));
    let fires = store.schedule_fires(schedule.id, 10).await.unwrap();
    assert_eq!(fires.len(), 1);
    assert_eq!(fires[0].outcome, FireOutcome::Pending);
    let after = store.schedule(schedule.id).await.unwrap().unwrap();
    assert_eq!(after.next_run_at, Some(i64::MAX), "立即执行不动计划");
}
