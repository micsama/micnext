//! 工具与 HTTP 共用的任务操作。

use mic_message::SessionId;
use mic_store::{
    CallStat, Fire, FireId, NewSchedule, PushDelivery, PushId, Schedule, ScheduleEdit, ScheduleId,
    StoreError,
};
use serde::{Deserialize, Serialize};

use super::{final_paragraph, Rule, Schedules};
use crate::event::KernelEventKind;
use crate::run::now_ms;

#[derive(Debug, thiserror::Error)]
pub enum ScheduleError {
    #[error("定时任务只在常驻服务可用")]
    Unavailable,
    #[error("定时任务不存在")]
    NotFound,
    #[error("{0}")]
    Invalid(String),
    #[error(transparent)]
    Store(#[from] StoreError),
}

#[derive(Default, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScheduleUpdate {
    pub name: Option<String>,
    pub rule: Option<String>,
    pub prompt: Option<String>,
    pub enabled: Option<bool>,
}

#[derive(Serialize)]
pub struct ScheduleView {
    #[serde(flatten)]
    pub schedule: Schedule,
    pub enabled: bool,
    pub recent: Vec<Fire>,
    pub usage: CallStat,
}

#[derive(Serialize)]
pub struct ScheduleFire {
    #[serde(flatten)]
    pub fire: Fire,
    pub pushes: Vec<FirePush>,
    pub reply: Option<String>,
}

#[derive(Serialize)]
pub struct FirePush {
    pub id: PushId,
    pub deliveries: Vec<PushDelivery>,
}

#[derive(Serialize)]
pub struct ScheduleFirePage {
    pub fires: Vec<ScheduleFire>,
    pub has_more: bool,
}

fn non_empty(field: &str, value: Option<String>) -> Result<Option<String>, ScheduleError> {
    match value.map(|v| v.trim().to_owned()) {
        Some(v) if v.is_empty() => Err(ScheduleError::Invalid(format!("{field}不能为空"))),
        v => Ok(v),
    }
}

fn upcoming(rule: &Rule) -> Result<i64, ScheduleError> {
    rule.next_after(now_ms())
        .ok_or_else(|| ScheduleError::Invalid("一次性时刻已过去，请重新设置时间规则".into()))
}

fn checked_rule(value: Option<String>) -> Result<Option<(String, i64)>, ScheduleError> {
    non_empty("时间规则", value)?
        .map(|value| {
            let rule = Rule::parse(&value)
                .map_err(|e| ScheduleError::Invalid(format!("时间规则不合法：{e}")))?;
            Ok((value, upcoming(&rule)?))
        })
        .transpose()
}

impl Schedules {
    pub(crate) fn publish_changed(&self, schedule: &Schedule) {
        self.events.emit(
            schedule.session_id,
            mic_store::SCHEDULE_MODULE,
            KernelEventKind::ScheduleChanged {
                schedule: schedule.id,
            },
        );
    }

    pub(crate) async fn create(
        &self,
        session: SessionId,
        name: String,
        rule: String,
        prompt: String,
    ) -> Result<Schedule, ScheduleError> {
        let name = non_empty("名字", Some(name))?.expect("已给出名字");
        let prompt = non_empty("指令", Some(prompt))?.expect("已给出指令");
        let (rule, next) = checked_rule(Some(rule))?.expect("已给出规则");
        let session = self.store.session(session).await?.expect("调用会话存在");
        let schedule = self
            .store
            .create_schedule(NewSchedule {
                person: self.owner,
                name,
                rule,
                prompt,
                pwd: session.pwd,
                persona: session.persona_id,
                model: session.model_id,
                reasoning_effort: session.reasoning_effort,
                next_run_at: next,
                at: now_ms(),
            })
            .await?;
        self.changed();
        self.publish_changed(&schedule);
        Ok(schedule)
    }

    pub(crate) async fn update(
        &self,
        id: ScheduleId,
        edit: ScheduleUpdate,
    ) -> Result<Schedule, ScheduleError> {
        if edit.name.is_none()
            && edit.rule.is_none()
            && edit.prompt.is_none()
            && edit.enabled.is_none()
        {
            return Err(ScheduleError::Invalid("没有要修改的字段".into()));
        }
        let renamed = edit.name.is_some();
        let name = non_empty("名字", edit.name)?;
        let prompt = non_empty("指令", edit.prompt)?;
        let rule = checked_rule(edit.rule)?;
        let old = self.get(id).await?;
        let next_run_at = match (edit.enabled, &rule) {
            (Some(false), _) => old.enabled().then_some(None),
            (Some(true), _) if !old.enabled() => Some(Some(match &rule {
                Some((_, next)) => *next,
                None => upcoming(&Rule::parse(&old.rule).expect("写入时已校验"))?,
            })),
            (_, Some((when, next))) if old.enabled() && *when != old.rule => Some(Some(*next)),
            _ => None,
        };
        let schedule = self
            .store
            .update_schedule(
                id,
                ScheduleEdit {
                    name,
                    rule: rule.map(|(value, _)| value),
                    prompt,
                    next_run_at,
                },
                now_ms(),
            )
            .await?
            .ok_or(ScheduleError::NotFound)?;
        self.changed();
        self.publish_changed(&schedule);
        if renamed {
            self.events.emit(
                schedule.session_id,
                mic_store::SCHEDULE_MODULE,
                KernelEventKind::SessionTitleChanged,
            );
        }
        Ok(schedule)
    }

    pub(crate) async fn delete(&self, id: ScheduleId) -> Result<(), ScheduleError> {
        let schedule = self.get(id).await?;
        if !self.store.delete_schedule(id).await? {
            return Err(ScheduleError::NotFound);
        }
        self.changed();
        self.publish_changed(&schedule);
        Ok(())
    }

    pub(crate) async fn get(&self, id: ScheduleId) -> Result<Schedule, ScheduleError> {
        self.store
            .schedule(id)
            .await?
            .ok_or(ScheduleError::NotFound)
    }

    pub(crate) async fn list(&self) -> Result<Vec<ScheduleView>, ScheduleError> {
        let mut views = Vec::new();
        for schedule in self.store.schedules(self.owner).await? {
            views.push(self.view(schedule).await?);
        }
        Ok(views)
    }

    pub(crate) async fn view(&self, schedule: Schedule) -> Result<ScheduleView, ScheduleError> {
        Ok(ScheduleView {
            recent: self.store.schedule_fires(schedule.id, 10).await?,
            usage: self.store.schedule_usage(schedule.id).await?,
            enabled: schedule.enabled(),
            schedule,
        })
    }

    pub(crate) async fn fires(
        &self,
        id: ScheduleId,
        before: Option<FireId>,
        limit: u32,
    ) -> Result<ScheduleFirePage, ScheduleError> {
        self.get(id).await?;
        let mut fires = self
            .store
            .schedule_fires_before(id, before, limit + 1)
            .await?;
        let has_more = fires.len() > limit as usize;
        fires.truncate(limit as usize);
        let mut views = Vec::with_capacity(fires.len());
        for fire in fires {
            let (reply, pushes) = match fire.run_id {
                Some(run) => (
                    final_paragraph(&self.store, run).await?.map(|s| {
                        let clipped: String = s.chars().take(200).collect();
                        if clipped.len() < s.len() {
                            format!("{clipped}…")
                        } else {
                            clipped
                        }
                    }),
                    self.store
                        .run_pushes(run)
                        .await?
                        .into_iter()
                        .map(|(id, deliveries)| FirePush { id, deliveries })
                        .collect(),
                ),
                None => (None, Vec::new()),
            };
            views.push(ScheduleFire {
                fire,
                reply,
                pushes,
            });
        }
        Ok(ScheduleFirePage {
            fires: views,
            has_more,
        })
    }
}
