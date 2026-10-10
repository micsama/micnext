use std::future::Future;
use std::sync::Arc;
use std::time::Instant;

use mic_core::{BoxError, EventReceiver, Kernel, KernelEventKind};
use mic_message::{MessageBody, MessageId};
use mic_store::RunId;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::account::Account;
use crate::client::{Client, ClientError, TypingStatus};
use crate::limits::{TYPING_KEEPALIVE, TYPING_TICKET_TTL};
use crate::service::ConnectionExit;

/// 当前指示中的 run、其 ticket，以及 run 结束后待送达的最后一条出站。
struct Active {
    run: RunId,
    ticket: String,
    until: Option<MessageId>,
}

/// getconfig 结果，空 ticket 也缓存，与 SDK 一致。
struct Ticket {
    value: Option<String>,
    fetched: Instant,
}

/// 尽力而为的结果：继续，或连接失效需停止本对象收发。
enum Flow {
    Continue,
    Expired,
}

/// 本对象会话 run 期间发「输入中」，本 run 回复送达后取消；独立于投递，只读其进度游标。
/// 失败只记日志，-14 与 Protocol 不吞。
pub(crate) async fn run(
    kernel: Kernel,
    account: Arc<Account>,
    mut events: EventReceiver,
    context: watch::Receiver<Option<String>>,
    mut delivered: watch::Receiver<Option<MessageId>>,
    stop: CancellationToken,
) -> Result<ConnectionExit, BoxError> {
    let client = Client::new()?;
    let mut typing = Typing {
        client: &client,
        account: &account,
        stop: &stop,
        ticket: None,
        active: None,
    };
    let session = account.view.session_id;
    let mut outbound = None;
    if let Some(run) = kernel.executing_run(session).await? {
        let token = context.borrow().clone();
        if let Flow::Expired = typing.begin(run, token).await? {
            return Ok(ConnectionExit::NeedsLogin);
        }
    }
    let mut keepalive = tokio::time::interval(TYPING_KEEPALIVE);
    keepalive.set_missed_tick_behavior(MissedTickBehavior::Delay);
    loop {
        let flow = tokio::select! {
            biased;
            () = stop.cancelled() => return Ok(ConnectionExit::Stopped),
            event = events.recv() => match event {
                Ok(event) if event.session_id == session => match event.kind {
                    KernelEventKind::MessageAppended(message)
                        if matches!(
                            message.body,
                            MessageBody::Reply { .. } | MessageBody::Notification { .. }
                        ) =>
                    {
                        outbound = Some(message.id);
                        Flow::Continue
                    }
                    KernelEventKind::RunStarted { run_id } => {
                        let token = context.borrow().clone();
                        let flow = typing.begin(run_id, token).await?;
                        keepalive.reset();
                        flow
                    }
                    KernelEventKind::RunFinished { run_id, .. } => {
                        let progress = *delivered.borrow_and_update();
                        typing.finish(run_id, outbound, progress).await?
                    }
                    _ => Flow::Continue,
                },
                Ok(_) => Flow::Continue,
                Err(_) => {
                    let flow = typing.end().await?;
                    match (flow, kernel.executing_run(session).await?) {
                        (Flow::Continue, Some(run)) => {
                            let token = context.borrow().clone();
                            let flow = typing.begin(run, token).await?;
                            keepalive.reset();
                            flow
                        }
                        (flow, _) => flow,
                    }
                }
            },
            changed = delivered.changed(), if typing.is_settling() => match changed {
                Ok(()) => {
                    let progress = *delivered.borrow_and_update();
                    typing.settle(progress).await?
                }
                Err(_) => return Ok(ConnectionExit::Stopped),
            },
            _ = keepalive.tick(), if typing.is_active() => typing.keepalive().await?,
        };
        if let Flow::Expired = flow {
            return Ok(ConnectionExit::NeedsLogin);
        }
    }
}

struct Typing<'a> {
    client: &'a Client,
    account: &'a Account,
    stop: &'a CancellationToken,
    ticket: Option<Ticket>,
    active: Option<Active>,
}

impl Typing<'_> {
    fn is_active(&self) -> bool {
        self.active.is_some()
    }

    fn is_settling(&self) -> bool {
        self.active
            .as_ref()
            .is_some_and(|active| active.until.is_some())
    }

    /// 同一 run 重复开始（启动校准与 RunStarted 重叠）不重复发送。
    async fn begin(&mut self, run: RunId, context: Option<String>) -> Result<Flow, ClientError> {
        if self.active.as_ref().is_some_and(|active| active.run == run) {
            return Ok(Flow::Continue);
        }
        if let Flow::Expired = self.end().await? {
            return Ok(Flow::Expired);
        }
        let Some(context) = context else {
            tracing::info!(run_id = run.0, "wechat typing skipped: no context token");
            return Ok(Flow::Continue);
        };
        let ticket = match self.ticket(&context).await {
            Ok(Some(ticket)) => ticket,
            Ok(None) => {
                tracing::info!(run_id = run.0, "wechat typing skipped: ticket absent");
                return Ok(Flow::Continue);
            }
            Err(error) => return best_effort(error),
        };
        let flow = self.send(&ticket, TypingStatus::Typing).await?;
        tracing::debug!(run_id = run.0, "wechat typing started");
        self.active = Some(Active {
            run,
            ticket,
            until: None,
        });
        Ok(flow)
    }

    async fn keepalive(&self) -> Result<Flow, ClientError> {
        match &self.active {
            Some(active) => self.send(&active.ticket, TypingStatus::Typing).await,
            None => Ok(Flow::Continue),
        }
    }

    /// 本 run 的出站先于 RunFinished 发布；已送达即取消，否则等投递游标追上。
    async fn finish(
        &mut self,
        run: RunId,
        outbound: Option<MessageId>,
        delivered: Option<MessageId>,
    ) -> Result<Flow, ClientError> {
        match &mut self.active {
            Some(active) if active.run == run => {
                active.until = outbound;
                self.settle(delivered).await
            }
            _ => Ok(Flow::Continue),
        }
    }

    async fn settle(&mut self, delivered: Option<MessageId>) -> Result<Flow, ClientError> {
        match self.active.as_ref().and_then(|active| active.until) {
            Some(until) if delivered.is_none_or(|delivered| delivered < until) => {
                Ok(Flow::Continue)
            }
            _ => self.end().await,
        }
    }

    async fn end(&mut self) -> Result<Flow, ClientError> {
        match self.active.take() {
            Some(active) => {
                tracing::debug!(run_id = active.run.0, "wechat typing cancelled");
                self.send(&active.ticket, TypingStatus::Cancel).await
            }
            None => Ok(Flow::Continue),
        }
    }

    async fn ticket(&mut self, context: &str) -> Result<Option<String>, ClientError> {
        if let Some(ticket) = self
            .ticket
            .as_ref()
            .filter(|ticket| ticket.fetched.elapsed() < TYPING_TICKET_TTL)
        {
            return Ok(ticket.value.clone());
        }
        let value = self
            .guard(
                self.client
                    .typing_ticket(&self.account.credentials, context),
            )
            .await?;
        self.ticket = Some(Ticket {
            value: value.clone(),
            fetched: Instant::now(),
        });
        Ok(value)
    }

    async fn send(&self, ticket: &str, status: TypingStatus) -> Result<Flow, ClientError> {
        match self
            .guard(
                self.client
                    .typing(&self.account.credentials, ticket, status),
            )
            .await
        {
            Ok(()) => Ok(Flow::Continue),
            Err(error) => best_effort(error),
        }
    }

    /// stop 时放弃请求，按网络失败处理；外层循环随即退出。
    async fn guard<T>(
        &self,
        request: impl Future<Output = Result<T, ClientError>>,
    ) -> Result<T, ClientError> {
        tokio::select! {
            biased;
            () = self.stop.cancelled() => Err(ClientError::Network),
            result = request => result,
        }
    }
}

fn best_effort(error: ClientError) -> Result<Flow, ClientError> {
    match error {
        ClientError::SessionExpired => Ok(Flow::Expired),
        ClientError::Protocol(_) => Err(error),
        error => {
            tracing::warn!(error = %error, "wechat typing failed");
            Ok(Flow::Continue)
        }
    }
}
