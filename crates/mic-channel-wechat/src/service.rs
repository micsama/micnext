use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mic_core::{
    Activation, BoxError, BoxFuture, ChannelConnection, ChannelSetup, ChannelSetupError,
    ChannelSetupView, EventReceiver, IncomingPart, Kernel, Module, ModuleConfig, Registry, Service,
    SetupAttempt, SetupAttemptId, SetupFailure, SetupProgress,
};
use serde::Deserialize;
use tokio::sync::{mpsc, oneshot, watch};
use tokio::task::{JoinHandle, JoinSet};
use tokio_util::sync::CancellationToken;

use crate::account::{self, Account, AccountError};
use crate::client::{Client, ClientError, IncomingContent, Unsupported};
use crate::delivery;
use crate::limits::{
    ATTEMPT_ID_BYTES, COMMAND_CAPACITY, NETWORK_RETRY, NETWORK_SLOW_RETRY, NETWORK_SLOW_THRESHOLD,
    VERIFY_CODE_MAX_CHARS,
};
use crate::login::{self, Outcome, Progress};

pub struct WechatModule;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {}

impl Module for WechatModule {
    fn name(&self) -> &'static str {
        "wechat"
    }
    fn activation(&self) -> Activation {
        Activation::Always
    }
    fn install(&self, reg: &mut Registry, config: ModuleConfig) -> Result<(), BoxError> {
        config.parse::<Config>()?;
        let (commands, rx) = mpsc::channel(COMMAND_CAPACITY);
        let (view, snapshot) = watch::channel(None);
        reg.migrations(account::MIGRATIONS);
        reg.channel_setup(Setup { commands, snapshot });
        reg.channel_prompt(
            "wechat",
            "WeChat: Markdown mostly works (no math, footnotes, collapsibles; code blocks unhighlighted). Tool activity is invisible, but text sent with tool calls reaches the user as messages: keep such updates short and occasional.",
        );
        reg.service(Coordinator { commands: rx, view });
        Ok(())
    }
}

type Ack<T> = oneshot::Sender<Result<T, ChannelSetupError>>;

enum Command {
    Begin(Ack<SetupAttempt>),
    SubmitCode {
        id: SetupAttemptId,
        code: String,
        ack: Ack<()>,
    },
    Cancel {
        id: SetupAttemptId,
        ack: Ack<()>,
    },
}

struct Setup {
    commands: mpsc::Sender<Command>,
    snapshot: watch::Receiver<Option<ChannelSetupView>>,
}

impl ChannelSetup for Setup {
    fn status(&self) -> BoxFuture<Result<ChannelSetupView, ChannelSetupError>> {
        let view = self
            .snapshot
            .borrow()
            .clone()
            .ok_or(ChannelSetupError::Unavailable);
        Box::pin(async move { view })
    }
    fn begin(&self) -> BoxFuture<Result<SetupAttempt, ChannelSetupError>> {
        let tx = self.commands.clone();
        Box::pin(async move {
            let (ack, response) = oneshot::channel();
            tx.send(Command::Begin(ack))
                .await
                .map_err(|_| ChannelSetupError::Unavailable)?;
            response.await.map_err(|_| ChannelSetupError::Unavailable)?
        })
    }
    fn submit_code(
        &self,
        id: SetupAttemptId,
        code: String,
    ) -> BoxFuture<Result<(), ChannelSetupError>> {
        let tx = self.commands.clone();
        Box::pin(async move {
            let code = code.trim().to_owned();
            if code.is_empty() || code.chars().count() > VERIFY_CODE_MAX_CHARS {
                return Err(ChannelSetupError::InvalidCode);
            }
            let (ack, response) = oneshot::channel();
            tx.send(Command::SubmitCode { id, code, ack })
                .await
                .map_err(|_| ChannelSetupError::Unavailable)?;
            response.await.map_err(|_| ChannelSetupError::Unavailable)?
        })
    }
    fn cancel(&self, id: SetupAttemptId) -> BoxFuture<Result<(), ChannelSetupError>> {
        let tx = self.commands.clone();
        Box::pin(async move {
            let (ack, response) = oneshot::channel();
            tx.send(Command::Cancel { id, ack })
                .await
                .map_err(|_| ChannelSetupError::Unavailable)?;
            response.await.map_err(|_| ChannelSetupError::Unavailable)?
        })
    }
}

struct Coordinator {
    commands: mpsc::Receiver<Command>,
    view: watch::Sender<Option<ChannelSetupView>>,
}

struct ActiveLogin {
    stop: CancellationToken,
    code: watch::Sender<Option<String>>,
    task: JoinHandle<Result<Outcome, ClientError>>,
}

pub(crate) enum ConnectionExit {
    Stopped,
    NeedsLogin,
}

struct ActiveConnection {
    account: Arc<Account>,
    stop: CancellationToken,
    task: JoinHandle<Result<ConnectionExit, BoxError>>,
}

impl Service for Coordinator {
    fn run(
        self: Box<Self>,
        kernel: Kernel,
        stop: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>> {
        Box::pin(self.serve(kernel, stop))
    }
}

impl Coordinator {
    async fn serve(mut self, kernel: Kernel, stop: CancellationToken) -> Result<(), BoxError> {
        account::recover_batches(&kernel).await?;
        delivery::recover(&kernel).await?;
        let account = account::active(&kernel).await?;
        let mut view = ChannelSetupView {
            account: account.as_ref().map(|a| a.view.clone()),
            login: None,
        };
        self.view.send_replace(Some(view.clone()));
        let (progress_tx, mut progress_rx) = mpsc::unbounded_channel();
        let mut active: Option<ActiveLogin> = None;
        let mut connection = start_connection(kernel.clone(), account, &stop).await?;
        let result = async {
          loop {
            tokio::select! {
                biased;
                () = stop.cancelled() => break,
                command = self.commands.recv() => {
                    let Some(command) = command else { break };
                    match command {
                        Command::Begin(ack) => {
                            cancel_active(&mut active).await?;
                            let id = new_attempt_id()?;
                            let tokens = account::local_tokens(&kernel).await?;
                            let login_stop = stop.child_token();
                            let (code, input) = watch::channel(None);
                            let task = tokio::spawn(login::run(id.clone(), tokens, input, progress_tx.clone(), login_stop.clone()));
                            let attempt = SetupAttempt { id, progress: SetupProgress::Preparing };
                            view.login = Some(attempt.clone());
                            active = Some(ActiveLogin { stop: login_stop, code, task });
                            self.view.send_replace(Some(view.clone()));
                            let _ = ack.send(Ok(attempt));
                        }
                        Command::SubmitCode { id, code, ack } => {
                            let result = current(&view, &id).and_then(|attempt| {
                                let SetupProgress::NeedsCode { qr_content } = &attempt.progress else {
                                    return Err(ChannelSetupError::WrongPhase);
                                };
                                active.as_ref().ok_or(ChannelSetupError::WrongPhase)?.code.send(Some(code))
                                    .map(|_| qr_content.clone()).map_err(|_| ChannelSetupError::WrongPhase)
                            });
                            let result = result.map(|qr_content| {
                                view.login.as_mut().expect("current 已确认 attempt").progress = SetupProgress::Scanned { qr_content };
                                self.view.send_replace(Some(view.clone()));
                            });
                            let _ = ack.send(result);
                        }
                        Command::Cancel { id, ack } => {
                            let result = current(&view, &id).and_then(|attempt| {
                                if terminal(&attempt.progress) { Err(ChannelSetupError::WrongPhase) } else { Ok(()) }
                            });
                            if result.is_ok() {
                                cancel_active(&mut active).await?;
                                view.login.as_mut().expect("current 已确认 attempt").progress = SetupProgress::Cancelled;
                                self.view.send_replace(Some(view.clone()));
                            }
                            let _ = ack.send(result);
                        }
                    }
                }
                Some(Progress { id, progress }) = progress_rx.recv() => {
                    if active.is_some() {
                        if let Some(attempt) = &mut view.login {
                            if attempt.id == id {
                                attempt.progress = progress;
                                self.view.send_replace(Some(view.clone()));
                            }
                        }
                    }
                }
                result = joined(&mut active) => {
                    active.take();
                    let result = result.map_err(|_| ClientError::Protocol("登录任务崩溃"))?;
                    let progress = match result {
                        Ok(Outcome::Connected(credentials)) => {
                            stop_connection(&kernel, &mut connection).await?;
                            let account = account::save(&kernel, credentials).await?;
                            view.account = Some(account.clone());
                            connection = start_connection(kernel.clone(), account::active(&kernel).await?, &stop).await?;
                            tracing::info!("wechat account linked");
                            SetupProgress::Connected { account }
                        }
                        Ok(Outcome::Expired) => SetupProgress::Expired,
                        Ok(Outcome::Cancelled) => SetupProgress::Cancelled,
                        Ok(Outcome::Failed(reason)) => SetupProgress::Failed { reason },
                        Err(error) => {
                            view.login.as_mut().expect("task 对应 attempt").progress = SetupProgress::Failed { reason: SetupFailure::Protocol };
                            self.view.send_replace(Some(view.clone()));
                            return Err(error.into());
                        }
                    };
                    view.login.as_mut().expect("task 对应 attempt").progress = progress;
                    self.view.send_replace(Some(view.clone()));
                }
                result = connection_joined(&mut connection) => {
                    let connection = connection.take().expect("join 对应连接");
                    let exit = result.map_err(|_| ClientError::Protocol("收发任务崩溃"))??;
                    match exit {
                        ConnectionExit::NeedsLogin => {
                            account::set_connection(&kernel, &connection.account.credentials.user_id, ChannelConnection::NeedsLogin).await?;
                            view.account.as_mut().expect("连接对应账号").connection = ChannelConnection::NeedsLogin;
                            self.view.send_replace(Some(view.clone()));
                            tracing::warn!("wechat account needs login");
                        }
                        ConnectionExit::Stopped => return Err(AccountError::Invalid("收发任务未收到停止信号就退出").into()),
                    }
                }
            }
          }
          Ok::<_, BoxError>(())
        }.await;
        if let Some(active) = &active {
            active.stop.cancel();
        }
        if let Some(connection) = &connection {
            connection.stop.cancel();
        }
        let login_closed = cancel_active(&mut active).await;
        let connection_closed = stop_connection(&kernel, &mut connection).await;
        result.and(login_closed).and(connection_closed)
    }
}

async fn start_connection(
    kernel: Kernel,
    account: Option<Account>,
    stop: &CancellationToken,
) -> Result<Option<ActiveConnection>, BoxError> {
    let Some(account) = account else {
        return Ok(None);
    };
    match account.view.connection {
        ChannelConnection::NeedsLogin | ChannelConnection::Faulted => return Ok(None),
        ChannelConnection::Connected => {}
    }
    let state = account::state(&kernel, &account.credentials.user_id).await?;
    let (events, cut) = kernel.subscribe_from_now(account.view.session_id).await?;
    let account = Arc::new(account);
    let connection_stop = stop.child_token();
    let connection = Connection {
        kernel,
        account: account.clone(),
        state,
        events,
        cut,
    };
    let task = tokio::spawn(connection.run(connection_stop.clone()));
    Ok(Some(ActiveConnection {
        account,
        stop: connection_stop,
        task,
    }))
}

struct Connection {
    kernel: Kernel,
    account: Arc<Account>,
    state: account::State,
    events: EventReceiver,
    cut: Option<mic_message::MessageId>,
}

impl Connection {
    async fn run(self, stop: CancellationToken) -> Result<ConnectionExit, BoxError> {
        let (context, token) = watch::channel(self.state.context_token.clone());
        let mut tasks = JoinSet::new();
        tasks.spawn(delivery::run(
            self.kernel.clone(),
            self.account.clone(),
            self.events,
            self.cut,
            token,
            stop.clone(),
        ));
        tasks.spawn(inbound(
            self.kernel,
            self.account,
            self.state,
            context,
            stop.clone(),
        ));
        let first = tokio::select! {
            biased;
            () = stop.cancelled() => None,
            result = tasks.join_next() => result,
        };
        stop.cancel();
        let mut result = first
            .map(worker_result)
            .unwrap_or(Ok(ConnectionExit::Stopped));
        while let Some(joined) = tasks.join_next().await {
            match worker_result(joined) {
                Ok(ConnectionExit::NeedsLogin) if result.is_ok() => {
                    result = Ok(ConnectionExit::NeedsLogin)
                }
                Err(error) => result = Err(error),
                _ => {}
            }
        }
        result
    }
}

fn worker_result(
    result: Result<Result<ConnectionExit, BoxError>, tokio::task::JoinError>,
) -> Result<ConnectionExit, BoxError> {
    result.map_err(|_| ClientError::Protocol("收发子任务崩溃"))?
}

async fn connection_joined(
    connection: &mut Option<ActiveConnection>,
) -> Result<Result<ConnectionExit, BoxError>, tokio::task::JoinError> {
    match connection {
        Some(connection) => (&mut connection.task).await,
        None => std::future::pending().await,
    }
}

async fn stop_connection(
    kernel: &Kernel,
    connection: &mut Option<ActiveConnection>,
) -> Result<(), BoxError> {
    if let Some(connection) = connection.take() {
        connection.stop.cancel();
        if matches!(
            connection
                .task
                .await
                .map_err(|_| ClientError::Protocol("收发任务崩溃"))??,
            ConnectionExit::NeedsLogin
        ) {
            account::set_connection(
                kernel,
                &connection.account.credentials.user_id,
                ChannelConnection::NeedsLogin,
            )
            .await?;
        }
    }
    Ok(())
}

async fn inbound(
    kernel: Kernel,
    account: Arc<Account>,
    mut state: account::State,
    context: watch::Sender<Option<String>>,
    stop: CancellationToken,
) -> Result<ConnectionExit, BoxError> {
    let client = Client::new()?;
    let mut failures = 0u32;
    loop {
        let updates = tokio::select! {
            biased;
            () = stop.cancelled() => return Ok(ConnectionExit::Stopped),
            result = client.updates(&account.credentials, &state.cursor, state.timeout_ms) => result,
        };
        let updates = match updates {
            Ok(updates) => {
                failures = 0;
                updates
            }
            Err(ClientError::Timeout) => continue,
            Err(ClientError::SessionExpired) => return Ok(ConnectionExit::NeedsLogin),
            Err(error @ ClientError::Protocol(_)) => return Err(error.into()),
            Err(error) => {
                failures = failures.saturating_add(1);
                tracing::warn!(failures, error = %error, "wechat inbound network or business failure");
                let delay = if failures >= NETWORK_SLOW_THRESHOLD {
                    NETWORK_SLOW_RETRY
                } else {
                    NETWORK_RETRY
                };
                tokio::select! {
                    biased;
                    () = stop.cancelled() => return Ok(ConnectionExit::Stopped),
                    () = tokio::time::sleep(delay) => {},
                }
                continue;
            }
        };
        if let Some(timeout) = updates.timeout_ms {
            state.timeout_ms = timeout;
        }
        let batch = account::persist_batch(
            &kernel,
            &account.credentials.user_id,
            &updates,
            state.timeout_ms,
        )
        .await?;
        state.cursor = updates.cursor;
        for incoming in updates.incoming {
            if stop.is_cancelled() {
                return Ok(ConnectionExit::Stopped);
            }
            account::remember_context(
                &kernel,
                &account.credentials.user_id,
                &incoming.context_token,
            )
            .await?;
            context.send_replace(Some(incoming.context_token));
            let Flattened { parts, kinds } = flatten(incoming.content);
            if parts.is_empty() {
                continue;
            }
            let message = if kinds.is_empty() {
                kernel
                    .append_user_input(account.view.session_id, account.person, parts)
                    .await?
            } else {
                kernel
                    .append_recorded_input(
                        account.view.session_id,
                        account.person,
                        parts,
                        "wechat",
                        format!("微信渠道暂不支持{}，这条消息仅作记录。", kinds.join("、")),
                    )
                    .await?
            };
            tracing::info!(
                session_id = account.view.session_id.0,
                message_id = message.0,
                unsupported = kinds.len(),
                "wechat input appended"
            );
        }
        if let Some(batch) = batch {
            account::finish_batch(&kernel, batch).await?;
        }
    }
}

struct Flattened {
    parts: Vec<IncomingPart>,
    /// 不支持的类型名，按首次出现去重。
    kinds: Vec<&'static str>,
}

/// 不支持的 item 按原位置打扁成短占位，供用户与模型看到同一份记录。
fn flatten(content: Vec<IncomingContent>) -> Flattened {
    let mut out = Flattened {
        parts: Vec::with_capacity(content.len()),
        kinds: Vec::new(),
    };
    for item in content {
        let (placeholder, kind) = match item {
            IncomingContent::Text(text) => {
                out.parts.push(IncomingPart::Text(text));
                continue;
            }
            IncomingContent::VoiceText(text) => {
                out.parts
                    .push(IncomingPart::Text(format!("[语音转写] {text}")));
                continue;
            }
            IncomingContent::Unsupported(Unsupported::Image) => ("[图片]".to_owned(), "图片"),
            IncomingContent::Unsupported(Unsupported::Voice) => ("[语音]".to_owned(), "语音"),
            IncomingContent::Unsupported(Unsupported::File { name }) => (
                name.map_or_else(|| "[文件]".to_owned(), |name| format!("[文件：{name}]")),
                "文件",
            ),
            IncomingContent::Unsupported(Unsupported::Video) => ("[视频]".to_owned(), "视频"),
            IncomingContent::Unsupported(Unsupported::Other) => {
                ("[不支持的消息]".to_owned(), "此类消息")
            }
        };
        out.parts.push(IncomingPart::Text(placeholder));
        if !out.kinds.contains(&kind) {
            out.kinds.push(kind);
        }
    }
    out
}

fn current<'a>(
    view: &'a ChannelSetupView,
    id: &SetupAttemptId,
) -> Result<&'a SetupAttempt, ChannelSetupError> {
    view.login
        .as_ref()
        .filter(|attempt| &attempt.id == id)
        .ok_or(ChannelSetupError::AttemptNotFound)
}

fn terminal(progress: &SetupProgress) -> bool {
    matches!(
        progress,
        SetupProgress::Connected { .. }
            | SetupProgress::Expired
            | SetupProgress::Cancelled
            | SetupProgress::Failed { .. }
    )
}

async fn joined(
    active: &mut Option<ActiveLogin>,
) -> Result<Result<Outcome, ClientError>, tokio::task::JoinError> {
    match active {
        Some(active) => (&mut active.task).await,
        None => std::future::pending().await,
    }
}

async fn cancel_active(active: &mut Option<ActiveLogin>) -> Result<(), BoxError> {
    if let Some(active) = active.take() {
        active.stop.cancel();
        active
            .task
            .await
            .map_err(|_| ClientError::Protocol("登录任务崩溃"))??;
    }
    Ok(())
}

fn new_attempt_id() -> Result<SetupAttemptId, AccountError> {
    let mut bytes = [0; ATTEMPT_ID_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| ClientError::Protocol("随机源不可用"))?;
    Ok(SetupAttemptId(
        bytes.iter().map(|byte| format!("{byte:02x}")).collect(),
    ))
}
