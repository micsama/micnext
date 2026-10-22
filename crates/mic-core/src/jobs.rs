//! 后台任务运行时：登记、执行、计时、终止、输出落盘与回注；子 agent 的派出、追问与停止。
//! 契约：docs/blueprints/archive/async-jobs.md。

mod agent;
mod output;
mod sink;
mod tools;

use std::collections::{BTreeSet, HashMap};
use std::future::Future;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use mic_message::{ContentPart, ExecFailureKind, ExecOutcome, MessageBody, SessionId};
use mic_store::{
    Job, JobId, JobKind, JobState, NewToolJob, RunId, RunState, Session, SessionKind, Store,
    StoreError,
};
use mic_tool::{Background, BackgroundTask, BoxFuture, ToolError, ToolHandle};
use serde::Serialize;
use tokio::sync::{mpsc, oneshot, watch, Notify};
use tokio::task::AbortHandle;

use crate::control::{RunControl, StopBy};
use crate::event::{Events, KernelEventKind};
use crate::limits::{JOB_HIDE_AFTER, JOB_READ_MAX, JOB_REPORT_TAIL, JOB_VIEW_TAIL};
use crate::run::now_ms;

pub(crate) use agent::{delivery, AGENT_INTERRUPTED, AGENT_MODULES, CORE_TOOLS};
pub use output::JobOutput;
pub(crate) use output::Tree;
use output::Writer;
use sink::Sink;

const INTERRUPTED: &str =
    "micnext stopped while this job was running; the external process state is unknown.";

pub(crate) struct Jobs {
    store: Store,
    events: Events,
    /// `-p` 没有调度器，回注只落库。
    wake: Option<mpsc::Sender<SessionId>>,
    dir: PathBuf,
    running: Mutex<HashMap<JobId, Running>>,
    /// 终态未能落盘的第一个错误；装配层据此停服务，下次启动按中断收尾。
    fault: Mutex<Option<StoreError>>,
    fault_set: Notify,
    control: Arc<RunControl>,
    /// 子 agent 可用的工具名上限（白名单模块的登记工具与管理工具）。
    agent_tools: BTreeSet<String>,
}

struct Running {
    /// 第一个终止请求取走。
    stop: Option<oneshot::Sender<StopRequest>>,
    /// 收尾完成即为 true；任务被中止时发送端随之丢弃。
    done: watch::Receiver<bool>,
    abort: AbortHandle,
}

struct StopRequest {
    manager: SessionId,
    by_user: bool,
    /// 本次终止是否赢得收尾。
    won: oneshot::Sender<bool>,
}

enum End {
    Exited(ExecOutcome),
    TimedOut(Duration),
    Stopped(StopRequest),
    /// 输出目录无法建立，任务未启动。
    Unstarted(std::io::Error),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopJobOutcome {
    Stopped,
    /// agent 休眠中，没有可停的执行。
    AlreadyIdle,
    AlreadyFinished,
}

/// 任务列表一项；模型 `jobs` 与 Web 列表同口径。
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct JobView {
    pub id: JobId,
    pub tool_name: String,
    pub owner_session_id: SessionId,
    pub description: String,
    pub created_at: i64,
    #[serde(flatten)]
    pub detail: JobDetail,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum JobDetail {
    Tool {
        state: JobState,
        finished_at: Option<i64>,
        timeout_ms: Option<u64>,
        output_bytes: u64,
        /// 输出末尾一小段。
        tail: String,
    },
    Agent {
        child_session_id: SessionId,
        /// 有执行中的 run；否则休眠。
        running: bool,
        /// 最近一个已结束 run 的状态。
        #[serde(serialize_with = "run_state")]
        last_run: Option<RunState>,
        last_active_at: i64,
    },
}

fn run_state<S: serde::Serializer>(state: &Option<RunState>, s: S) -> Result<S::Ok, S::Error> {
    match state {
        Some(state) => s.serialize_some(state.as_str()),
        None => s.serialize_none(),
    }
}

#[derive(Debug, thiserror::Error)]
pub enum JobAccessError {
    #[error("任务不存在")]
    NotFound,
    #[error("任务不在本会话及其子会话名下")]
    Forbidden,
    #[error("单次最多读取 {JOB_READ_MAX} 字节")]
    ReadLimit,
    #[error("子 agent 的交付不按偏移读取")]
    AgentOffset,
    #[error("读取任务输出失败：{0}")]
    Io(#[from] std::io::Error),
    #[error(transparent)]
    Store(#[from] StoreError),
}

/// 一次工具调用的派出身份。
#[derive(Clone)]
pub(crate) struct Origin {
    pub(crate) session: SessionId,
    pub(crate) run: RunId,
    pub(crate) tool_call_id: String,
    pub(crate) tool_name: String,
}

impl Jobs {
    pub(crate) fn new(
        store: Store,
        events: Events,
        wake: Option<mpsc::Sender<SessionId>>,
        data_dir: PathBuf,
        control: Arc<RunControl>,
        agent_tools: BTreeSet<String>,
    ) -> Self {
        Self {
            store,
            events,
            wake,
            dir: jobs_dir(data_dir),
            running: Mutex::default(),
            fault: Mutex::default(),
            fault_set: Notify::new(),
            control,
            agent_tools,
        }
    }

    /// 测试用：输出写到进程独有的临时目录。
    #[cfg(test)]
    pub(crate) fn scratch(store: &Store, events: &Events) -> Arc<Self> {
        static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
        let n = NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let dir = std::env::temp_dir().join(format!("micnext-test-{}-{n}", std::process::id()));
        Arc::new(Self::new(
            store.clone(),
            events.clone(),
            None,
            dir,
            Arc::default(),
            BTreeSet::new(),
        ))
    }

    /// core 生成的工具，绑定调用会话：管理工具；能收到交付的 Root 会话另有 `agent`。
    pub(crate) fn tools(self: &Arc<Self>, session: &Session) -> Vec<ToolHandle> {
        let mut tools = tools::handles(self, session.id).to_vec();
        if self.wake.is_some() && matches!(session.kind, SessionKind::Root { .. }) {
            tools.push(agent::handle(self, session));
        }
        tools
    }

    /// 登记即提交，随后立即开始执行。
    async fn dispatch(
        self: Arc<Self>,
        origin: Origin,
        task: BackgroundTask,
    ) -> Result<JobId, StoreError> {
        let tree = tree(&self.store.session_root(origin.session).await?);
        let job = self
            .store
            .create_tool_job(NewToolJob {
                owner_session_id: origin.session,
                origin_run_id: origin.run,
                origin_tool_call_id: origin.tool_call_id,
                tool_name: origin.tool_name,
                description: task.description.clone(),
                timeout_ms: task.timeout.map(|t| t.as_millis() as u64),
                created_at: now_ms(),
            })
            .await?;
        let id = job.id;
        let (stop_tx, stop_rx) = oneshot::channel();
        let (done_tx, done_rx) = watch::channel(false);
        {
            // 持锁 spawn：任务收尾时的移除必在登记之后。
            let mut running = self.running.lock().expect("任务表未中毒");
            let handle =
                tokio::spawn(
                    self.clone()
                        .drive(job, tree.clone(), task, stop_rx, done_tx),
                );
            running.insert(
                id,
                Running {
                    stop: Some(stop_tx),
                    done: done_rx,
                    abort: handle.abort_handle(),
                },
            );
        }
        self.changed(&tree);
        Ok(id)
    }

    async fn drive(
        self: Arc<Self>,
        job: Job,
        tree: Tree,
        task: BackgroundTask,
        stop: oneshot::Receiver<StopRequest>,
        done: watch::Sender<bool>,
    ) {
        let dir = self.dir.join(job.id.0.to_string());
        let end = match Writer::create(dir, self.events.clone(), tree.clone()) {
            Err(e) => End::Unstarted(e),
            Ok((writer, seal)) => {
                let (sink, seal) = Sink::new(writer, seal, self.clone(), job.clone(), tree.clone());
                let mut run = (task.run)(Box::new(sink));
                let stopped = async {
                    match stop.await {
                        Ok(request) => request,
                        Err(_) => std::future::pending().await,
                    }
                };
                let timeout = async {
                    match task.timeout {
                        Some(t) => tokio::time::sleep(t).await,
                        None => std::future::pending().await,
                    }
                };
                let end = tokio::select! {
                    biased;
                    request = stopped => End::Stopped(request),
                    () = timeout => End::TimedOut(task.timeout.expect("只有限时任务会超时")),
                    outcome = &mut run => End::Exited(outcome),
                };
                // 丢弃 `run` 即释放工具资源（如杀进程组）；封口后输出与通知不再变化，再读尾部收尾。
                drop(run);
                seal.seal().await;
                end
            }
        };
        let id = job.id;
        if let Err(e) = self.finish(job, &tree, end).await {
            // 库里仍是 Running：留在运行表，不当作已结束。
            tracing::error!(job = id.0, "background job finish failed");
            self.fault.lock().expect("故障槽未中毒").get_or_insert(e);
            self.fault_set.notify_one();
            return;
        }
        self.running.lock().expect("任务表未中毒").remove(&id);
        let _ = done.send(true);
        self.changed(&tree);
    }

    async fn finish(&self, job: Job, tree: &Tree, end: End) -> Result<(), StoreError> {
        match &end {
            End::Exited(ExecOutcome::Failed {
                kind: ExecFailureKind::Dependency,
                message,
            }) => {
                tracing::warn!(job = job.id.0, error = %message, "background job dependency failed");
            }
            End::Unstarted(error) => {
                tracing::warn!(job = job.id.0, error = %crate::error::chain(error), "background job output create failed");
            }
            _ => {}
        }
        let tail = self.tail(job.id, JOB_REPORT_TAIL).await;
        let (state, outcome, won) = match end {
            End::Exited(ExecOutcome::Completed { mut output }) => {
                output.push(text(tail));
                (
                    JobState::Completed,
                    Some(ExecOutcome::Completed { output }),
                    None,
                )
            }
            End::Exited(ExecOutcome::Failed { kind, message }) => (
                JobState::Failed,
                Some(ExecOutcome::Failed {
                    kind,
                    message: format!("{message}\n{tail}"),
                }),
                None,
            ),
            End::Exited(ExecOutcome::Cancelled { message }) => (
                JobState::Cancelled,
                Some(ExecOutcome::Cancelled {
                    message: format!("{message}\n{tail}"),
                }),
                None,
            ),
            End::TimedOut(after) => (
                JobState::TimedOut,
                Some(ExecOutcome::Completed {
                    output: vec![
                        text(format!("[timed out after {}ms]", after.as_millis())),
                        text(tail),
                    ],
                }),
                None,
            ),
            End::Stopped(request) => {
                let message = if request.by_user {
                    Some("Stopped by the user.".to_owned())
                } else if request.manager != job.owner_session_id {
                    Some(format!("Stopped by session {}.", request.manager.0))
                } else {
                    // 发起方自己终止，已从工具返回获知。
                    None
                };
                (
                    JobState::Cancelled,
                    message.map(|m| ExecOutcome::Cancelled {
                        message: format!("{m}\n{tail}"),
                    }),
                    Some(request.won),
                )
            }
            End::Unstarted(e) => (
                JobState::Failed,
                Some(ExecOutcome::Failed {
                    kind: ExecFailureKind::Dependency,
                    message: format!(
                        "failed to create the output log: {}",
                        crate::error::chain(&e)
                    ),
                }),
                None,
            ),
        };
        let owner = job.owner_session_id;
        let report = outcome.map(|outcome| completion(&job, outcome));
        let finished = {
            let publish = self.events.publisher().await;
            let finished = self.store.finish_job(job.id, state, report, now_ms()).await;
            if let Ok(Some(Some(message))) = &finished {
                publish.appended(owner, &tree.channel, message.clone());
            }
            finished
        };
        if let Some(won) = won {
            let _ = won.send(matches!(finished, Ok(Some(_))));
        }
        if let Some(Some(_)) = finished? {
            self.wake(owner).await?;
        }
        Ok(())
    }

    /// 停止中的会话只记录不唤醒。
    pub(crate) async fn wake(&self, session: SessionId) -> Result<(), StoreError> {
        if self.control.closing(session).is_some()
            || self
                .store
                .session_root(session)
                .await?
                .archived_at
                .is_some()
        {
            return Ok(());
        }
        if let Some(wake) = &self.wake {
            let _ = wake.send(session).await;
        }
        Ok(())
    }

    pub(crate) fn changed(&self, tree: &Tree) {
        self.events
            .emit(tree.root, &tree.channel, KernelEventKind::JobsChanged);
    }

    /// 回注与列表用的输出尾部说明；读失败也照实写进去，不当作无输出。
    async fn tail(&self, id: JobId, limit: u64) -> String {
        let dir = self.dir.join(id.0.to_string());
        match tokio::task::spawn_blocking(move || output::read(&dir, None, limit))
            .await
            .expect("读输出不 panic")
        {
            Ok(out) => render_tail(id, &out),
            Err(e) => {
                let error = crate::error::chain(&e);
                tracing::warn!(job = id.0, %error, "background job output read failed");
                format!("[output unavailable: {error}]")
            }
        }
    }

    pub(crate) async fn views(
        &self,
        manager: SessionId,
        include_old: bool,
    ) -> Result<Vec<JobView>, JobAccessError> {
        let since = (!include_old).then(|| now_ms() - JOB_HIDE_AFTER.as_millis() as i64);
        let jobs = self.store.subtree_jobs(manager, since).await?;
        let dir = self.dir.clone();
        Ok(tokio::task::spawn_blocking(move || {
            jobs.into_iter()
                .map(|(job, activity)| {
                    let detail = match (job.kind, activity) {
                        (
                            JobKind::Tool {
                                timeout_ms,
                                state,
                                finished_at,
                            },
                            _,
                        ) => {
                            let out =
                                output::read(&dir.join(job.id.0.to_string()), None, JOB_VIEW_TAIL)?;
                            JobDetail::Tool {
                                state,
                                finished_at,
                                timeout_ms,
                                output_bytes: out.total_bytes,
                                tail: out.text,
                            }
                        }
                        (JobKind::Agent { child_session_id }, Some(a)) => JobDetail::Agent {
                            child_session_id,
                            running: a.running,
                            last_run: a.last_run,
                            last_active_at: a.last_active_at,
                        },
                        (JobKind::Agent { .. }, None) => unreachable!("agent 必有执行概况"),
                    };
                    Ok(JobView {
                        id: job.id,
                        tool_name: job.tool_name,
                        owner_session_id: job.owner_session_id,
                        description: job.description,
                        created_at: job.created_at,
                        detail,
                    })
                })
                .collect::<std::io::Result<Vec<_>>>()
        })
        .await
        .expect("读输出不 panic")?)
    }

    /// tool job 按偏移读输出；agent job 返回最近一次交付（整段，不接受 `offset`）。
    pub(crate) async fn output(
        &self,
        manager: SessionId,
        id: JobId,
        offset: Option<u64>,
        limit: u64,
    ) -> Result<(Job, JobOutput), JobAccessError> {
        if limit == 0 || limit > JOB_READ_MAX {
            return Err(JobAccessError::ReadLimit);
        }
        let job = self.managed(manager, id).await?;
        if let JobKind::Agent { child_session_id } = job.kind {
            if offset.is_some() {
                return Err(JobAccessError::AgentOffset);
            }
            let text = match self.store.latest_agent_report(child_session_id).await? {
                Some(m) => agent::report_text(&m.body),
                None => String::new(),
            };
            let len = text.len() as u64;
            let out = JobOutput {
                text,
                offset: 0,
                next_offset: len,
                eof: true,
                available_from: 0,
                total_bytes: len,
            };
            return Ok((job, out));
        }
        let dir = self.dir.join(id.0.to_string());
        let out = tokio::task::spawn_blocking(move || output::read(&dir, offset, limit))
            .await
            .expect("读输出不 panic")?;
        Ok((job, out))
    }

    /// 收尾完成后返回；与完成、超时或其他终止竞争时只有一方收尾。
    pub(crate) async fn stop(
        &self,
        manager: SessionId,
        id: JobId,
        by_user: bool,
    ) -> Result<StopJobOutcome, JobAccessError> {
        let job = self.managed(manager, id).await?;
        if let JobKind::Agent { child_session_id } = job.kind {
            return Ok(self
                .stop_agent(&job, child_session_id, StopBy { manager, by_user })
                .await?);
        }
        Ok(self.stop_tool(manager, &job, by_user).await)
    }

    async fn stop_tool(&self, manager: SessionId, job: &Job, by_user: bool) -> StopJobOutcome {
        let id = job.id;
        if job.tool_state() != Some(JobState::Running) {
            return StopJobOutcome::AlreadyFinished;
        }
        let entry = self
            .running
            .lock()
            .expect("任务表未中毒")
            .get_mut(&id)
            .map(|r| (r.stop.take(), r.done.clone()));
        let Some((stop, mut done)) = entry else {
            return StopJobOutcome::AlreadyFinished;
        };
        if let Some(stop) = stop {
            let (won, result) = oneshot::channel();
            let request = StopRequest {
                manager,
                by_user,
                won,
            };
            if stop.send(request).is_ok() {
                if let Ok(true) = result.await {
                    return StopJobOutcome::Stopped;
                }
            }
        }
        let _ = done.wait_for(|d| *d).await;
        StopJobOutcome::AlreadyFinished
    }

    async fn managed(&self, manager: SessionId, id: JobId) -> Result<Job, JobAccessError> {
        let job = self.store.job(id).await?.ok_or(JobAccessError::NotFound)?;
        if !self.store.in_subtree(manager, job.owner_session_id).await? {
            return Err(JobAccessError::Forbidden);
        }
        Ok(job)
    }

    /// 根标记已经落盘：取消执行、清理后台任务，保留停止事实，不再唤醒。
    pub(crate) async fn archive(&self, root: SessionId) -> Result<(), StoreError> {
        let sessions = self.store.subtree_sessions(root).await?;
        let mut closing = Vec::new();
        for session in &sessions {
            if let Some((guard, _)) = self
                .control
                .close(
                    session.id,
                    StopBy {
                        manager: root,
                        by_user: true,
                    },
                )
                .await
            {
                closing.push(guard);
            }
        }
        for session in &sessions {
            for job in self.store.running_jobs(Some(session.id)).await? {
                self.stop_tool(root, &job, true).await;
                if self
                    .store
                    .job(job.id)
                    .await?
                    .is_some_and(|j| j.tool_state() == Some(JobState::Running))
                {
                    return Err(StoreError::JobStopFailed { job: job.id.0 });
                }
            }
            self.store.hold_session_inputs(session.id, now_ms()).await?;
        }
        drop(closing);
        self.changed(&tree(&self.store.session_root(root).await?));
        Ok(())
    }

    /// 退出前丢弃全部后台任务（工具随之释放资源）；状态留在 `running`，下次启动收尾。
    /// 等到第一次终态落盘失败。
    pub(crate) async fn fault(&self) -> StoreError {
        loop {
            if let Some(e) = self.fault.lock().expect("故障槽未中毒").take() {
                return e;
            }
            self.fault_set.notified().await;
        }
    }

    pub(crate) async fn shutdown(&self) {
        let running: Vec<_> = self
            .running
            .lock()
            .expect("任务表未中毒")
            .drain()
            .map(|(_, r)| (r.abort, r.done))
            .collect();
        for (abort, mut done) in running {
            abort.abort();
            let _ = done.wait_for(|d| *d).await;
        }
    }
}

fn jobs_dir(data_dir: PathBuf) -> PathBuf {
    data_dir.join("jobs")
}

pub(crate) fn tree(root: &Session) -> Tree {
    let channel = match &root.kind {
        SessionKind::Root { channel, .. } => channel.clone(),
        SessionKind::Triggered { module, .. } => module.clone(),
        SessionKind::Task { .. } => unreachable!("session_root 返回的必是树根"),
    };
    Tree {
        root: root.id,
        channel,
    }
}

fn text(text: String) -> ContentPart {
    ContentPart::Text { text }
}

fn completion(job: &Job, outcome: ExecOutcome) -> MessageBody {
    MessageBody::Completion {
        person: job.person,
        tool_name: job.tool_name.clone(),
        exec_id: job.id.0.to_string(),
        outcome,
    }
}

fn render_tail(id: JobId, out: &JobOutput) -> String {
    if out.total_bytes == 0 {
        return "[no output]".to_owned();
    }
    let shown = out.next_offset - out.offset;
    if shown == out.total_bytes {
        return format!("[output: {} bytes]\n{}", out.total_bytes, out.text);
    }
    format!(
        "[output: {} bytes, last {shown} shown; read more with job_output job_id={}]\n{}",
        out.total_bytes, id.0, out.text
    )
}

/// 启动收尾：遗留 `running` 的任务记为中断并回注（随后与其他未处理输入一起待命）。
/// 在任何 worker 出现之前运行，不发事件。
pub(crate) async fn recover(store: &Store, data_dir: PathBuf) -> Result<usize, StoreError> {
    let dir = jobs_dir(data_dir);
    let stale = store.running_jobs(None).await?;
    for job in &stale {
        let tail = match output::read(&dir.join(job.id.0.to_string()), None, JOB_REPORT_TAIL) {
            Ok(out) => render_tail(job.id, &out),
            Err(e) => {
                let error = crate::error::chain(&e);
                tracing::warn!(job = job.id.0, %error, "background job output read failed");
                format!("[output unavailable: {error}]")
            }
        };
        let outcome = ExecOutcome::Cancelled {
            message: format!("{INTERRUPTED}\n{tail}"),
        };
        store
            .finish_job(
                job.id,
                JobState::Interrupted,
                Some(completion(job, outcome)),
                now_ms(),
            )
            .await?;
    }
    Ok(stale.len())
}

/// 登记结果；`None` 为登记中，失败带原因。
type Registration = watch::Receiver<Option<Result<JobId, String>>>;

/// 绑定一次工具调用的派出 port。登记在独立任务里完成，工具 future 被丢弃也不会半途而废。
pub(crate) struct JobPort {
    jobs: Arc<Jobs>,
    /// 任务会话为 false：定时任务不派后台任务。
    background: bool,
    origin: Origin,
    registration: Mutex<Option<Registration>>,
}

impl JobPort {
    pub(crate) fn new(jobs: Arc<Jobs>, background: bool, origin: Origin) -> Self {
        Self {
            jobs,
            background,
            origin,
            registration: Mutex::default(),
        }
    }

    pub(crate) fn origin(&self) -> &Origin {
        &self.origin
    }

    /// 本调用已提交的任务；未派出或登记失败为 `None`。
    pub(crate) async fn registered(&self) -> Option<JobId> {
        let mut rx = self.registration.lock().expect("登记槽未中毒").clone()?;
        let value = rx.wait_for(Option::is_some).await.ok()?;
        value.clone().and_then(Result::ok)
    }

    /// 登记在独立任务里完成，工具 future 被丢弃也不会半途而废；一个调用只能登记一次。
    async fn register(
        &self,
        register: impl Future<Output = Result<JobId, String>> + Send + 'static,
    ) -> Result<JobId, ToolError> {
        let mut rx = {
            let mut slot = self.registration.lock().expect("登记槽未中毒");
            if slot.is_some() {
                return Err(ToolError::business(
                    "a tool call can dispatch only one background job",
                ));
            }
            let (tx, rx) = watch::channel(None);
            tokio::spawn(async move {
                let _ = tx.send(Some(register.await));
            });
            *slot = Some(rx.clone());
            rx
        };
        let value = rx
            .wait_for(Option::is_some)
            .await
            .expect("登记任务结束前必发送结果")
            .clone();
        value.expect("已等到结果").map_err(ToolError::dependency)
    }
}

impl Background for JobPort {
    fn dispatch(&self, task: BackgroundTask) -> BoxFuture<'_, Result<String, ToolError>> {
        Box::pin(async move {
            if !self.background {
                return Err(ToolError::business(
                    "background jobs are unavailable in scheduled tasks; run in the foreground",
                ));
            }
            let (jobs, origin) = (self.jobs.clone(), self.origin.clone());
            let id = self
                .register(async move {
                    jobs.dispatch(origin, task).await.map_err(|e| {
                        tracing::error!(error = %crate::error::chain(&e), "background job registration failed");
                        format!("failed to register the background job: {e}")
                    })
                })
                .await?;
            Ok(id.0.to_string())
        })
    }
}

/// 不允许后台派出的调用场景（如压缩时读文件快照）。
pub(crate) struct NoBackground;

impl Background for NoBackground {
    fn dispatch(&self, _: BackgroundTask) -> BoxFuture<'_, Result<String, ToolError>> {
        Box::pin(async {
            Err(ToolError::business(
                "background execution is not available here",
            ))
        })
    }
}

#[cfg(test)]
mod tests;
