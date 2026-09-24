use mic_message::{SessionEntry, SessionId};
use mic_store::{QueryId, QueryState};
use tokio::sync::broadcast::{self, error::RecvError, error::TryRecvError};

use crate::limits::EVENT_CAPACITY;

/// 内核实时事件。可丢：订阅方落后即收到 `Lagged`，应断开并按稳定游标（entry id）重放。
#[derive(Debug, Clone)]
pub struct KernelEvent {
    pub session_id: SessionId,
    /// 会话所属 Channel（Root 的 `channel`），订阅方据此过滤，无需自己查会话。
    pub channel: String,
    pub kind: KernelEventKind,
}

#[derive(Debug, Clone)]
pub enum KernelEventKind {
    /// 一轮开始（已 claim）。
    QueryStarted { query_id: QueryId },
    /// 当前草稿的正文增量。
    TextDelta(String),
    /// 当前草稿的可见推理增量。
    ReasoningDelta(String),
    /// 当前草稿结束：成功时其落盘 entry 已先以 `EntryAppended` 发出；失败（含重试前）时草稿作废。
    DraftEnded,
    /// 任一 entry 落盘，带 id。
    EntryAppended(SessionEntry),
    /// 一轮结束，`state` 为落盘的终态。
    QueryFinished {
        query_id: QueryId,
        state: QueryState,
    },
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
pub(crate) struct Events(broadcast::Sender<KernelEvent>);

impl Events {
    pub(crate) fn new() -> Self {
        Self(broadcast::channel(EVENT_CAPACITY).0)
    }

    /// 没有订阅者时即丢弃。
    pub(crate) fn emit(&self, session_id: SessionId, channel: &str, kind: KernelEventKind) {
        let _ = self.0.send(KernelEvent {
            session_id,
            channel: channel.to_owned(),
            kind,
        });
    }

    pub(crate) fn subscribe(&self) -> EventReceiver {
        EventReceiver(self.0.subscribe())
    }
}
