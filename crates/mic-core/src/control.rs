//! 执行入口登记：每个会话至多一个认领中/执行中的槽位，停止令牌与停 agent 的闸门在此。

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use mic_message::SessionId;
use mic_store::RunId;
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

#[derive(Default)]
pub(crate) struct RunControl {
    inner: Mutex<Inner>,
}

#[derive(Default)]
struct Inner {
    active: HashMap<SessionId, Active>,
    /// 停止中的会话：不认领、不唤醒、不追问。
    closing: HashMap<SessionId, StopBy>,
    /// 用户请求接续的 run；worker 下次认领时先取。
    resume: HashMap<SessionId, RunId>,
}

struct Active {
    /// 认领到 run 之前为 `None`。
    run: Option<RunId>,
    token: CancellationToken,
    done: watch::Receiver<bool>,
}

/// 谁停的 agent；决定子 run 交付怎么写。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct StopBy {
    pub(crate) manager: SessionId,
    pub(crate) by_user: bool,
}

/// 会话的执行槽位；丢弃即释放，等待它的停止方随之返回。
pub(crate) struct Slot {
    control: Arc<RunControl>,
    session: SessionId,
    token: CancellationToken,
    done: watch::Sender<bool>,
}

/// 停止期闸门；丢弃即重新开放。
pub(crate) struct Closing {
    control: Arc<RunControl>,
    session: SessionId,
}

impl RunControl {
    fn lock(&self) -> std::sync::MutexGuard<'_, Inner> {
        self.inner.lock().expect("执行登记表未中毒")
    }

    /// 占用会话的执行槽位；停止中返回 `None`。调用方保证同一会话不并发占用。
    pub(crate) fn enter(self: &Arc<Self>, session: SessionId) -> Option<Slot> {
        let mut inner = self.lock();
        if inner.closing.contains_key(&session) {
            return None;
        }
        let token = CancellationToken::new();
        let (done, rx) = watch::channel(false);
        let previous = inner.active.insert(
            session,
            Active {
                run: None,
                token: token.clone(),
                done: rx,
            },
        );
        assert!(previous.is_none(), "同一会话至多一个执行槽位");
        Some(Slot {
            control: self.clone(),
            session,
            token,
            done,
        })
    }

    /// 停该会话执行中的 run（给定 `run` 则只停它）；没有匹配返回 false。
    pub(crate) fn stop(&self, session: SessionId, run: Option<RunId>) -> bool {
        let inner = self.lock();
        match inner.active.get(&session) {
            Some(a) if a.run.is_some() && (run.is_none() || a.run == run) => {
                a.token.cancel();
                true
            }
            _ => false,
        }
    }

    /// 关闭会话的认领入口并取消其当前执行；返回闸门与是否有执行被取消。
    /// 已在停止中返回 `None`。
    pub(crate) async fn close(
        self: &Arc<Self>,
        session: SessionId,
        by: StopBy,
    ) -> Option<(Closing, bool)> {
        let done = {
            let mut inner = self.lock();
            if inner.closing.contains_key(&session) {
                return None;
            }
            inner.closing.insert(session, by);
            inner.active.get(&session).map(|a| {
                a.token.cancel();
                (a.done.clone(), a.run.is_some())
            })
        };
        let closing = Closing {
            control: self.clone(),
            session,
        };
        let cancelled = match done {
            Some((mut done, cancelled)) => {
                let _ = done.wait_for(|d| *d).await;
                cancelled
            }
            None => false,
        };
        Some((closing, cancelled))
    }

    pub(crate) fn closing(&self, session: SessionId) -> Option<StopBy> {
        self.lock().closing.get(&session).copied()
    }

    pub(crate) fn request_resume(&self, session: SessionId, run: RunId) {
        self.lock().resume.insert(session, run);
    }

    pub(crate) fn take_resume(&self, session: SessionId) -> Option<RunId> {
        self.lock().resume.remove(&session)
    }
}

impl Slot {
    pub(crate) fn token(&self) -> CancellationToken {
        self.token.clone()
    }

    pub(crate) fn set_run(&self, run: RunId) {
        let mut inner = self.control.lock();
        let active = inner.active.get_mut(&self.session).expect("槽位登记过");
        active.run = Some(run);
    }
}

impl Drop for Slot {
    fn drop(&mut self) {
        self.control.lock().active.remove(&self.session);
        let _ = self.done.send(true);
    }
}

impl Drop for Closing {
    fn drop(&mut self) {
        self.control.lock().closing.remove(&self.session);
    }
}
