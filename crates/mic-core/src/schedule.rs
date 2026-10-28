//! 定时任务：调度循环、结局通知与自动暂停。契约：docs/blueprints/scheduled-tasks.md P2。

mod operations;
mod rule;
pub use operations::{ScheduleError, ScheduleFirePage, ScheduleUpdate, ScheduleView};
mod tools;

use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;

use mic_message::{MessageBody, PersonId, ReplyBlock, SessionId};
use mic_store::{
    Fire, FireOutcome, FireResult, RunId, Schedule, Store, StoreError, SCHEDULE_MODULE,
};
use mic_tool::ToolHandle;
use tokio::sync::{mpsc, Notify};

use crate::event::Events;
use crate::limits::{
    SCHEDULE_COMPACT_PERCENT, SCHEDULE_FAIL_PAUSE, SCHEDULE_LATE_GRACE, SCHEDULE_MISSED_SCAN,
    SCHEDULE_RUN_TIMEOUT, SCHEDULE_TICK_MAX,
};
use crate::push::Pushes;
use crate::run::{now_ms, Engine};
use crate::RunError;
pub(crate) use rule::Rule;

/// 任务会话的渠道说明。
pub(crate) const CHANNEL_PROMPT: &str = "This session belongs to a scheduled task. A trigger message is a run of the task: the user is probably not watching and your replies do not reach them, so deliver anything they should know with push, written to stand on its own; if nothing needs reporting, finish briefly. If necessary information is missing, do not guess or act on it; ask via push instead. Earlier runs stay in this session; act on the latest trigger. Messages from the user are the user talking to you here; answer them as usual.";

/// 错过次数触到扫描上限后，只在醒来前这段时间里找最后一次。
const MISSED_TAIL_MS: i64 = 24 * 3600 * 1000;

/// 连续失败判定往回看的触发数（「忙」「已停止」不计入）。
const STREAK_SCAN: u32 = 50;

pub(crate) struct Schedules {
    store: Store,
    events: Events,
    pushes: Arc<Pushes>,
    wake: mpsc::Sender<SessionId>,
    owner: PersonId,
    changed: Notify,
}

impl Schedules {
    pub(crate) fn new(
        store: Store,
        events: Events,
        pushes: Arc<Pushes>,
        wake: mpsc::Sender<SessionId>,
        owner: PersonId,
    ) -> Self {
        Self {
            store,
            events,
            pushes,
            wake,
            owner,
            changed: Notify::new(),
        }
    }

    /// `schedule`、`schedules`，只给 Root 会话。
    pub(crate) fn tools(self: &Arc<Self>, session: SessionId) -> [ToolHandle; 2] {
        tools::handles(self, session)
    }

    /// 任务增改删后唤醒调度循环。
    fn changed(&self) {
        self.changed.notify_one();
    }

    /// 任务会话的 run 收尾后：触发 run 的系统级失败即通知。
    pub(crate) async fn settled(&self, run: RunId) -> Result<(), StoreError> {
        if let Some(fire) = self.store.fire_of_run(run).await? {
            if fire.outcome.is_failure() {
                self.report(fire.clone()).await?;
            }
            if let Some(schedule) = self.store.schedule(fire.schedule_id).await? {
                self.publish_changed(&schedule);
            }
        }
        Ok(())
    }

    /// 框架推送「定时任务 X：结局与原因」；连续失败达到上限则一并暂停。
    async fn report(&self, fire: Fire) -> Result<(), StoreError> {
        let Some(schedule) = self.store.schedule(fire.schedule_id).await? else {
            return Ok(());
        };
        let mut text = format!(
            "定时任务「{}」{}。",
            schedule.name,
            self.reason(&fire).await?
        );
        let streak = self
            .store
            .schedule_fires(schedule.id, STREAK_SCAN)
            .await?
            .iter()
            .filter(|f| f.due_at >= schedule.enabled_at && !f.outcome.is_neutral())
            .take(SCHEDULE_FAIL_PAUSE)
            .filter(|f| f.outcome.is_failure())
            .count();
        if streak == SCHEDULE_FAIL_PAUSE && schedule.enabled() {
            self.store
                .pause_schedule(
                    schedule.id,
                    format!("连续 {SCHEDULE_FAIL_PAUSE} 次运行失败"),
                    now_ms(),
                )
                .await?;
            self.changed();
            self.publish_changed(&schedule);
            text.push_str(&format!(
                "已连续 {SCHEDULE_FAIL_PAUSE} 次失败，任务已暂停；排除问题后可在对话里让 agent 重新启用。"
            ));
        }
        self.pushes.notify(schedule.session_id, text).await?;
        Ok(())
    }

    async fn reason(&self, fire: &Fire) -> Result<String, StoreError> {
        Ok(match fire.outcome {
            FireOutcome::Failed => {
                let run = fire.run_id.expect("失败的触发有 run");
                let notice = self
                    .store
                    .run_messages(run)
                    .await?
                    .into_iter()
                    .rev()
                    .find_map(|m| match m.body {
                        MessageBody::Notification { text, .. } => Some(text),
                        _ => None,
                    });
                match notice {
                    Some(notice) => format!("运行失败：{}", notice.trim_end_matches('。')),
                    None => "运行失败".into(),
                }
            }
            FireOutcome::TimedOut => format!(
                "运行超过 {} 分钟，已停止",
                SCHEDULE_RUN_TIMEOUT.as_secs() / 60
            ),
            FireOutcome::Interrupted => "运行被服务重启中断".into(),
            FireOutcome::Missed { count: 1 } => {
                format!("在 {} 到点时服务未运行，已错过", rule::show(fire.due_at))
            }
            FireOutcome::Missed { count } if count >= SCHEDULE_MISSED_SCAN => format!(
                "在服务停机期间错过至少 {count} 次运行（最近一次 {}）",
                rule::show(fire.due_at)
            ),
            FireOutcome::Missed { count } => format!(
                "在服务停机期间错过 {count} 次运行（最近一次 {}）",
                rule::show(fire.due_at)
            ),
            outcome => unreachable!("非系统级失败不通知：{outcome:?}"),
        })
    }

    /// 到点：上下文占用高时先压缩；忙则只记一行。`next_run_at` 前移到 `next`。
    async fn fire(
        &self,
        engine: &Engine,
        schedule: &Schedule,
        due: i64,
        next: Option<i64>,
    ) -> Result<(), StoreError> {
        let session = self
            .store
            .session(schedule.session_id)
            .await?
            .expect("任务会话存在（外键保证）");
        let compact = crate::compaction::occupancy(engine, &session)
            .await?
            .is_some_and(|o| o.used_tokens * 100 > u64::from(o.window) * SCHEDULE_COMPACT_PERCENT);
        let publish = self.events.publisher().await;
        let result = self
            .store
            .fire_schedule(schedule.id, due, next, compact, now_ms())
            .await?;
        if !matches!(result, FireResult::Gone) {
            self.publish_changed(schedule);
        }
        if let FireResult::Fired(messages) = result {
            for message in messages {
                publish.appended(session.id, SCHEDULE_MODULE, message);
            }
            drop(publish);
            tracing::info!(
                schedule = schedule.id.0,
                session = session.id.0,
                "schedule fired"
            );
            // 调度循环已退出（停止中）时发送失败，消息下次启动处置为 held。
            let _ = self.wake.send(session.id).await;
        }
        Ok(())
    }
}

/// 常驻调度循环；只在出错时返回。`boot` 为本次启动收尾开始的时刻。
pub(crate) async fn run(engine: Arc<Engine>, boot: i64) -> RunError {
    match drive(engine, boot).await {
        Ok(never) => match never {},
        Err(e) => e.into(),
    }
}

async fn drive(engine: Arc<Engine>, boot: i64) -> Result<Infallible, StoreError> {
    let schedules = engine.schedules.clone().expect("常驻装配有定时任务");
    let store = &schedules.store;
    for fire in store.fires_interrupted_since(boot).await? {
        schedules.report(fire).await?;
    }
    loop {
        let now = now_ms();
        let mut wake: Option<i64> = None;
        for schedule in store.enabled_schedules().await? {
            let due = schedule.next_run_at.expect("启用任务有下次时刻");
            if due > now {
                wake = Some(wake.map_or(due, |w| w.min(due)));
                continue;
            }
            let rule = Rule::parse(&schedule.rule).expect("写入时已校验");
            let next = rule.next_after(now);
            if now - due <= SCHEDULE_LATE_GRACE.as_millis() as i64 {
                schedules.fire(&engine, &schedule, due, next).await?;
            } else {
                let (last, count) = missed(&rule, due, now);
                if let Some(fire) = store
                    .record_missed(schedule.id, due, last, count, next)
                    .await?
                {
                    schedules.report(fire).await?;
                    schedules.publish_changed(&schedule);
                }
            }
            if let Some(n) = next {
                wake = Some(wake.map_or(n, |w| w.min(n)));
            }
        }
        let wait = wake.map_or(SCHEDULE_TICK_MAX, |w| {
            Duration::from_millis((w - now_ms()).max(0) as u64).min(SCHEDULE_TICK_MAX)
        });
        tokio::select! {
            () = tokio::time::sleep(wait) => {}
            () = schedules.changed.notified() => {}
        }
    }
}

/// `[due, now]` 内按规则应触发的最后一次与次数（含 `due`）。
fn missed(rule: &Rule, due: i64, now: i64) -> (i64, u32) {
    let (mut last, mut count) = (due, 1);
    let mut t = due;
    while let Some(n) = rule.next_after(t).filter(|n| *n <= now) {
        (last, count, t) = (n, count + 1, n);
        if count >= SCHEDULE_MISSED_SCAN {
            // 触到上限的规则必然很密，最后一次落在醒来前一天内。
            t = t.max(now - MISSED_TAIL_MS);
        }
    }
    (last, count)
}

/// run 最后一条带正文的回复的首段。
async fn final_paragraph(store: &Store, run: RunId) -> Result<Option<String>, StoreError> {
    Ok(store
        .run_messages(run)
        .await?
        .into_iter()
        .rev()
        .find_map(|m| match m.body {
            MessageBody::Reply { blocks, .. } => {
                let text: String = blocks
                    .iter()
                    .filter_map(|b| match b {
                        ReplyBlock::Text { text, .. } => Some(text.as_str()),
                        _ => None,
                    })
                    .collect();
                let first = text.trim().split("\n\n").next().unwrap_or("").trim();
                (!first.is_empty()).then(|| first.to_owned())
            }
            _ => None,
        }))
}
