use std::future::Future;
use std::sync::Arc;

use mic_core::{BoxError, EventReceiver, Kernel, KernelEventKind};
use mic_store::RunId;
use tokio::sync::watch;
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;

use crate::account::Account;
use crate::client::{Client, ClientError, TypingStatus};
use crate::limits::TYPING_KEEPALIVE;
use crate::service::ConnectionExit;

/// 当前指示中的 run 与其 ticket。
struct Active {
    run: RunId,
    ticket: String,
}

/// 尽力而为的结果：继续，或连接失效需停止本对象收发。
enum Flow {
    Continue,
    Expired,
}

/// 本对象会话 run 期间发「输入中」，结束取消；失败只记日志，-14 与 Protocol 不吞。
pub(crate) async fn run(
    kernel: Kernel,
    account: Arc<Account>,
    mut events: EventReceiver,
    context: watch::Receiver<Option<String>>,
    stop: CancellationToken,
) -> Result<ConnectionExit, BoxError> {
    let client = Client::new()?;
    let typing = Typing {
        client: &client,
        account: &account,
        context: &context,
        stop: &stop,
    };
    let session = account.view.session_id;
    let mut active = None;
    if let Some(run) = kernel.executing_run(session).await? {
        if let Flow::Expired = typing.begin(run, &mut active).await? {
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
                    KernelEventKind::RunStarted { run_id } => typing.begin(run_id, &mut active).await?,
                    KernelEventKind::RunFinished { run_id, .. }
                        if active.as_ref().is_some_and(|active| active.run == run_id) =>
                    {
                        typing.end(&mut active).await?
                    }
                    _ => Flow::Continue,
                },
                Ok(_) => Flow::Continue,
                Err(_) => {
                    let flow = typing.end(&mut active).await?;
                    match (flow, kernel.executing_run(session).await?) {
                        (Flow::Continue, Some(run)) => typing.begin(run, &mut active).await?,
                        (flow, _) => flow,
                    }
                }
            },
            _ = keepalive.tick(), if active.is_some() => {
                let ticket = active.as_ref().map(|active| active.ticket.clone()).expect("守卫已确认");
                typing.send(&ticket, TypingStatus::Typing).await?
            }
        };
        if let Flow::Expired = flow {
            return Ok(ConnectionExit::NeedsLogin);
        }
    }
}

struct Typing<'a> {
    client: &'a Client,
    account: &'a Account,
    context: &'a watch::Receiver<Option<String>>,
    stop: &'a CancellationToken,
}

impl Typing<'_> {
    /// 同一 run 重复开始（启动校准与 RunStarted 重叠）不重复取 ticket。
    async fn begin(&self, run: RunId, active: &mut Option<Active>) -> Result<Flow, ClientError> {
        if active.as_ref().is_some_and(|active| active.run == run) {
            return Ok(Flow::Continue);
        }
        if let Flow::Expired = self.end(active).await? {
            return Ok(Flow::Expired);
        }
        let Some(token) = self.context.borrow().clone() else {
            return Ok(Flow::Continue);
        };
        let ticket = match self
            .guard(self.client.typing_ticket(&self.account.credentials, &token))
            .await
        {
            Ok(Some(ticket)) => ticket,
            Ok(None) => {
                tracing::debug!("wechat typing ticket absent");
                return Ok(Flow::Continue);
            }
            Err(error) => return best_effort(error),
        };
        let flow = self.send(&ticket, TypingStatus::Typing).await?;
        *active = Some(Active { run, ticket });
        Ok(flow)
    }

    async fn end(&self, active: &mut Option<Active>) -> Result<Flow, ClientError> {
        match active.take() {
            Some(active) => self.send(&active.ticket, TypingStatus::Cancel).await,
            None => Ok(Flow::Continue),
        }
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
