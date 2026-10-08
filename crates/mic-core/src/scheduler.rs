//! 会话级调度：同一会话至多一个 worker，跨会话并行。契约：docs/blueprints/run-execution.md §4.2。

use std::collections::HashMap;
use std::sync::Arc;

use mic_message::SessionId;
use mic_store::StoreError;
use tokio::sync::mpsc;
use tokio::task::{Id, JoinSet};

use crate::kernel::session_channel;
use crate::run::{now_ms, Engine};
use crate::RunError;

enum Slot {
    Running,
    /// worker 在跑时又被唤醒：结束后重起一次，接住它没来得及认领的输入。
    Dirty,
}

/// 只在出错时返回；丢弃即停止（worker 随之取消）。
pub(crate) async fn run(
    engine: Arc<Engine>,
    mut wake: mpsc::Receiver<SessionId>,
    initial: Vec<SessionId>,
) -> RunError {
    let mut slots: HashMap<SessionId, Slot> = HashMap::new();
    let mut workers = Workers {
        engine,
        set: JoinSet::new(),
        owners: HashMap::new(),
    };
    for session_id in initial {
        slots.insert(session_id, Slot::Running);
        workers.start(session_id);
    }
    loop {
        tokio::select! {
            Some(session_id) = wake.recv() => match slots.get_mut(&session_id) {
                Some(slot) => *slot = Slot::Dirty,
                None => {
                    slots.insert(session_id, Slot::Running);
                    workers.start(session_id);
                }
            },
            Some(joined) = workers.set.join_next_with_id() => {
                let (id, result) = match joined {
                    Ok(done) => done,
                    Err(e) => {
                        let session_id = workers.owners[&e.id()];
                        return RunError::RunPanicked { session_id, message: panic_message(e) };
                    }
                };
                if let Err(e) = result {
                    return e.into();
                }
                let session_id = workers.owners.remove(&id).expect("worker 登记过");
                if let Some(Slot::Dirty) = slots.remove(&session_id) {
                    slots.insert(session_id, Slot::Running);
                    workers.start(session_id);
                }
            }
        }
    }
}

struct Workers {
    engine: Arc<Engine>,
    set: JoinSet<Result<(), StoreError>>,
    owners: HashMap<Id, SessionId>,
}

impl Workers {
    fn start(&mut self, session_id: SessionId) {
        let handle = self.set.spawn(worker(self.engine.clone(), session_id));
        self.owners.insert(handle.id(), session_id);
    }
}

/// 逐个执行该会话可认领的 run，直到没有。
async fn worker(engine: Arc<Engine>, session_id: SessionId) -> Result<(), StoreError> {
    let session = engine
        .store
        .session(session_id)
        .await?
        .expect("被唤醒的会话必然存在");
    let channel = session_channel(&engine.store, &session).await?;
    while let Some((run, settings)) = engine.store.claim_next(session_id, now_ms()).await? {
        engine.run(&session, &channel, run, settings).await?;
    }
    Ok(())
}

pub(crate) fn panic_message(e: tokio::task::JoinError) -> String {
    if !e.is_panic() {
        return e.to_string();
    }
    let payload = e.into_panic();
    payload
        .downcast_ref::<&str>()
        .map(|s| (*s).to_owned())
        .or_else(|| payload.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic 负载不是字符串".into())
}
