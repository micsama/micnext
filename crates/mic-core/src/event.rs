use mic_message::{Message, MessageBody, MessageId, SessionId};
use mic_store::{PushId, Resumable, RunId, RunState, ScheduleId};
use std::collections::HashMap;
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
    ScheduleChanged {
        schedule: ScheduleId,
    },
    PushDeliveryChanged {
        push_id: PushId,
    },
    /// 会话行的列表投影变了（标题、置顶、归档、已读）；订阅方重新拉取。
    SessionChanged,
    /// run 认领了这些输入；某个 run_id 第一次出现即该 run 开始。
    InputsClaimed {
        run_id: RunId,
        inputs: Vec<MessageId>,
    },
    /// 即将进行第 `attempt` 次重试，等待 `delay_ms` 毫秒。
    ModelRetrying {
        attempt: u32,
        delay_ms: u64,
    },
    /// 一次调用尝试开始，新草稿取代旧的；`context_at` 即成功时 `Reply` 的 `created_at`。
    DraftStarted {
        context_at: i64,
    },
    /// 当前草稿的正文增量。
    TextDelta(String),
    /// 当前草稿的可见推理增量。
    ReasoningDelta(String),
    /// 本次调用尝试没有产生 `Reply`（失败、重试前、或成功但无内容），草稿作废。
    DraftDiscarded,
    /// 任一消息落盘，带 id；同一订阅内按 id 升序到达。成功调用的 `Reply` 即当前草稿的终点。
    MessageAppended(Message),
    /// 一次执行结束，`state` 为落盘的终态；`resume` 为收尾后会话可接续的 run。
    RunFinished {
        run_id: RunId,
        state: RunState,
        resume: Option<Resumable>,
    },
    /// 来源 run 的输入预测已落盘；会话开始新 run 后即作废。
    PredictionReady {
        run_id: RunId,
        text: String,
    },
    /// 会话树的后台任务有变化（登记、输出增长、结束），`session_id` 为树根；订阅方重新拉取。
    JobsChanged,
    /// 新推送已记录，`session_id` 为来源会话；订阅方重新拉取通知中心。
    PushCreated {
        push_id: PushId,
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

/// 正在生成、尚未落盘的回复，由草稿事件折叠而来。
#[derive(Debug, Clone)]
pub struct ReplyDraft {
    pub context_at: i64,
    pub reasoning: String,
    pub text: String,
}

/// 事件总线的发送端。
#[derive(Clone)]
pub(crate) struct Events {
    tx: broadcast::Sender<KernelEvent>,
    order: Arc<Mutex<()>>,
    /// 各会话当前草稿；与发送同锁更新，订阅时取快照不重不漏。
    drafts: Arc<std::sync::Mutex<HashMap<SessionId, ReplyDraft>>>,
}

impl Events {
    pub(crate) fn new() -> Self {
        Self {
            tx: broadcast::channel(EVENT_CAPACITY).0,
            order: Arc::default(),
            drafts: Arc::default(),
        }
    }

    /// 没有订阅者时即丢弃。`MessageAppended` 只经 [`Publisher`] 发。
    pub(crate) fn emit(&self, session_id: SessionId, channel: &str, kind: KernelEventKind) {
        let mut drafts = self.drafts.lock().expect("草稿表锁未中毒");
        match &kind {
            KernelEventKind::DraftStarted { context_at } => {
                drafts.insert(
                    session_id,
                    ReplyDraft {
                        context_at: *context_at,
                        reasoning: String::new(),
                        text: String::new(),
                    },
                );
            }
            KernelEventKind::TextDelta(text) => {
                if let Some(d) = drafts.get_mut(&session_id) {
                    d.text.push_str(text);
                }
            }
            KernelEventKind::ReasoningDelta(text) => {
                if let Some(d) = drafts.get_mut(&session_id) {
                    d.reasoning.push_str(text);
                }
            }
            KernelEventKind::DraftDiscarded => {
                drafts.remove(&session_id);
            }
            KernelEventKind::MessageAppended(Message {
                body: MessageBody::Reply { .. },
                ..
            }) => {
                drafts.remove(&session_id);
            }
            _ => {}
        }
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

    /// 订阅并取该会话当前草稿；之后的草稿事件全部留在接收器中。
    pub(crate) fn subscribe_with_draft(
        &self,
        session_id: SessionId,
    ) -> (EventReceiver, Option<ReplyDraft>) {
        let drafts = self.drafts.lock().expect("草稿表锁未中毒");
        (self.subscribe(), drafts.get(&session_id).cloned())
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
