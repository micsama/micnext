use mic_message::{Message, SessionId};
use mic_store::{RunId, RunState};
use std::sync::Arc;

use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};
use tokio::sync::{Mutex, MutexGuard};

use crate::limits::EVENT_CAPACITY;

/// 内核实时事件。可丢：订阅方落后即收到 `Lagged`，应断开并按稳定游标（消息 id）重放。
#[derive(Debug, Clone)]
pub struct KernelEvent {
    pub session_id: SessionId,
    /// 会话所属 Channel（Root 的 `channel`），订阅方据此过滤，无需自己查会话。
    pub channel: String,
    pub kind: KernelEventKind,
}

#[derive(Debug, Clone)]
pub enum KernelEventKind {
    /// 一轮开始（已认领）。
    RunStarted { run_id: RunId },
    /// 当前草稿的正文增量。
    TextDelta(String),
    /// 当前草稿的可见推理增量。
    ReasoningDelta(String),
    /// 本次调用尝试没有产生 `Reply`（失败、重试前、或成功但无内容），草稿作废。
    DraftDiscarded,
    /// 任一消息落盘，带 id；同一订阅内按 id 升序到达。成功调用的 `Reply` 即当前草稿的终点。
    MessageAppended(Message),
    /// 一轮结束，`state` 为落盘的终态。
    RunFinished { run_id: RunId, state: RunState },
}

pub struct EventReceiver(broadcast::Receiver<KernelEvent>);

impl EventReceiver {
    /// 内核停止后不再返回。
    pub async fn recv(&mut self) -> Result<KernelEvent, Lagged> {
        match self.0.recv().await {
            Ok(event) => Ok(event),
            Err(RecvError::Lagged(n)) => Err(Lagged(n)),
            Err(RecvError::Closed) => std::future::pending().await,
        }
    }

    /// 取走已到达的事件；没有则 `None`。
    pub(crate) fn try_recv(&mut self) -> Option<Result<KernelEvent, Lagged>> {
        match self.0.try_recv() {
            Ok(event) => Some(Ok(event)),
            Err(TryRecvError::Lagged(n)) => Some(Err(Lagged(n))),
            Err(TryRecvError::Empty | TryRecvError::Closed) => None,
        }
    }
}

#[derive(Debug, thiserror::Error)]
#[error("事件订阅落后，丢失了 {0} 条事件")]
pub struct Lagged(pub u64);

/// 事件总线的发送端。
#[derive(Clone)]
pub(crate) struct Events {
    tx: broadcast::Sender<KernelEvent>,
    order: Arc<Mutex<()>>,
}

impl Events {
    pub(crate) fn new() -> Self {
        Self {
            tx: broadcast::channel(EVENT_CAPACITY).0,
            order: Arc::default(),
        }
    }

    /// 没有订阅者时即丢弃。`MessageAppended` 只经 [`Publisher`] 发。
    pub(crate) fn emit(&self, session_id: SessionId, channel: &str, kind: KernelEventKind) {
        let _ = self.tx.send(KernelEvent {
            session_id,
            channel: channel.to_owned(),
            kind,
        });
    }

    /// 取得稳定消息的发布权，持有期间落盘再发布。
    pub(crate) async fn publisher(&self) -> Publisher<'_> {
        Publisher {
            events: self,
            _order: self.order.lock().await,
        }
    }

    pub(crate) fn subscribe(&self) -> EventReceiver {
        EventReceiver(self.tx.subscribe())
    }
}

/// 所有发布消息的写入互斥，故发布顺序即提交顺序（id 升序），订阅方可把消息 id 当完整前缀游标。
pub(crate) struct Publisher<'a> {
    events: &'a Events,
    _order: MutexGuard<'a, ()>,
}

impl Publisher<'_> {
    pub(crate) fn appended(&self, session_id: SessionId, channel: &str, message: Message) {
        self.events.emit(
            session_id,
            channel,
            KernelEventKind::MessageAppended(message),
        );
    }
}
