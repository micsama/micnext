use std::collections::{BTreeSet, HashMap};
use std::pin::Pin;
use std::sync::Mutex;
use std::task::{Context, Poll};
use std::time::Duration;

use mic_message::ToolResultOutcome;
use mic_store::{
    CredentialWrite, EndpointWrite, InputDisposition, ModelWrite, NewInputPart, NewSession,
    ReasoningEffort as E, SessionKind, StopReason, Store,
};
use tokio::sync::{mpsc, Notify};

use super::*;
use crate::event::Events;
use crate::provider::{Factories, ModelEvent, ModelResponse, Provider, ProviderError};
use crate::run::Engine;
use crate::{
    BoxFuture, BoxStream, ConfigError, ModelRequest, ProbeError, ProviderFactory, ReasoningOptions,
    SecretValue,
};

/// 一次模型调用的要点。
#[derive(Clone)]
struct Seen {
    system: String,
    tools: Vec<String>,
    last: String,
}

impl Seen {
    fn root(&self) -> bool {
        self.tools.iter().any(|t| t == "agent")
    }
}

/// 回复脚本：`None` 表示这次调用一直挂起，直到 run 被取消。
type Script = dyn Fn(&Seen) -> Option<Vec<ReplyBlock>> + Send + Sync;

struct Brain {
    script: Box<Script>,
    seen: Mutex<Vec<Seen>>,
    blocked: Notify,
}

struct Factory(Arc<Brain>);

impl ProviderFactory for Factory {
    fn kind(&self) -> &'static str {
        "test"
    }
    fn display_name(&self) -> &'static str {
        "test"
    }
    fn check_endpoint(&self, json: &str) -> Result<String, ConfigError> {
        Ok(json.into())
    }
    fn check_model(&self, _: &str, _: &str, json: &str) -> Result<String, ConfigError> {
        Ok(json.into())
    }
    fn key_env(&self, _: &str) -> Option<&'static str> {
        None
    }
    fn reasoning_options(&self, _: &str, _: &str) -> Result<Option<ReasoningOptions>, ConfigError> {
        Ok(None)
    }
    fn build(
        &self,
        _: &str,
        _: Option<SecretValue>,
        _: &str,
        _: &str,
        _: Option<E>,
    ) -> Result<Arc<dyn Provider>, ConfigError> {
        Ok(Arc::new(Mock(self.0.clone())))
    }
    fn list_models(
        &self,
        _: &str,
        _: Option<SecretValue>,
    ) -> BoxFuture<Result<Vec<String>, ProbeError>> {
        Box::pin(async { Ok(Vec::new()) })
    }
}

struct Mock(Arc<Brain>);

/// `None` 为永远挂起的流。
struct OneEvent(Option<Option<Result<ModelEvent, ProviderError>>>);

impl futures_core::Stream for OneEvent {
    type Item = Result<ModelEvent, ProviderError>;
    fn poll_next(mut self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match self.0.take() {
            Some(Some(event)) => Poll::Ready(Some(event)),
            Some(None) => {
                self.0 = Some(None);
                Poll::Pending
            }
            None => Poll::Ready(None),
        }
    }
}

impl Provider for Mock {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(&self, req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        let seen = Seen {
            system: req.system,
            tools: req.tools.into_iter().map(|t| t.name).collect(),
            last: req.messages.last().map(text_of).unwrap_or_default(),
        };
        let reply = (self.0.script)(&seen);
        self.0.seen.lock().unwrap().push(seen);
        let event = match reply {
            Some(blocks) => {
                let stop = if blocks
                    .iter()
                    .any(|b| matches!(b, ReplyBlock::ToolCall { .. }))
                {
                    StopReason::ToolUse
                } else {
                    StopReason::EndTurn
                };
                Some(Ok(ModelEvent::Finished(ModelResponse { blocks, stop })))
            }
            None => {
                self.0.blocked.notify_one();
                None
            }
        };
        Box::pin(OneEvent(Some(event)))
    }
}

fn text_of(m: &Message) -> String {
    match &m.body {
        MessageBody::UserInput { parts, .. } => parts
            .iter()
            .filter_map(|p| match p {
                ContentPart::Text { text } => Some(text.as_str()),
                _ => None,
            })
            .collect(),
        MessageBody::ToolResult { .. } => "<tool result>".into(),
        MessageBody::Completion { .. } => "<completion>".into(),
        _ => String::new(),
    }
}

fn say(text: &str) -> Option<Vec<ReplyBlock>> {
    Some(vec![ReplyBlock::Text {
        text: text.into(),
        phase: None,
    }])
}

fn spawn(calls: &[(&str, &str)]) -> Option<Vec<ReplyBlock>> {
    Some(
        calls
            .iter()
            .enumerate()
            .map(|(i, (description, task))| ReplyBlock::ToolCall {
                id: format!("call-{i}"),
                name: "agent".into(),
                args: serde_json::json!({ "task": task, "description": description }),
            })
            .collect(),
    )
}

/// 根会话说完 `go` 后按 `calls` 派子 agent；子 agent 收到 `block` 就挂起，否则回 `done: <task>`。
fn script(calls: &'static [(&'static str, &'static str)]) -> Box<Script> {
    Box::new(move |seen| {
        if seen.root() {
            return match seen.last.as_str() {
                "go" => spawn(calls),
                _ => say("ok"),
            };
        }
        match seen.last.as_str() {
            "block" => None,
            task => say(&format!("done: {task}")),
        }
    })
}

struct Fixture {
    store: Store,
    engine: Arc<Engine>,
    brain: Arc<Brain>,
    root: SessionId,
    scheduler: tokio::task::JoinHandle<crate::RunError>,
}

impl Fixture {
    async fn new(script: Box<Script>) -> Self {
        let store = Store::open_in_memory(&[]).await.unwrap();
        let owner = store.ensure_person("test", 0).await.unwrap();
        let endpoint = store
            .create_endpoint(
                EndpointWrite {
                    name: "test".into(),
                    kind: "test".into(),
                    config_json: "{}".into(),
                    credential: CredentialWrite::Clear,
                },
                0,
            )
            .await
            .unwrap();
        let model = store
            .create_model(
                ModelWrite {
                    endpoint_id: endpoint,
                    name: "test".into(),
                    config_json: "{}".into(),
                    context_window: None,
                    images: true,
                    price: Default::default(),
                },
                0,
            )
            .await
            .unwrap();
        let root = store
            .create_session(NewSession {
                kind: SessionKind::Root {
                    channel: "web".into(),
                    chat: "agent".into(),
                },
                parent_session_id: None,
                delivery_target: None,
                pwd: "/tmp".into(),
                tool_scope: ToolScope::All,
                created_at: 0,
                persona: None,
                model: Some(model),
                reasoning_effort: None,
            })
            .await
            .unwrap()
            .id;
        let brain = Arc::new(Brain {
            script,
            seen: Mutex::default(),
            blocked: Notify::new(),
        });
        let mut factories = Factories::default();
        factories.insert(Box::new(Factory(brain.clone()))).unwrap();
        let events = Events::new();
        let control: Arc<crate::control::RunControl> = Arc::default();
        let (wake, wakes) = mpsc::channel(64);
        let dir =
            std::env::temp_dir().join(format!("micnext-agent-{}-{}", std::process::id(), now_ms()));
        let jobs = Arc::new(Jobs::new(
            store.clone(),
            events.clone(),
            Some(wake.clone()),
            dir,
            control.clone(),
            BTreeSet::from(["jobs".to_owned(), "job_output".into(), "job_stop".into()]),
        ));
        let engine = Arc::new(Engine {
            jobs,
            store: store.clone(),
            events,
            factories: Arc::new(factories),
            tools: Vec::new(),
            channel_prompts: Arc::new(HashMap::new()),
            control,
        });
        let scheduler = tokio::spawn(crate::scheduler::run(engine.clone(), wakes));
        store
            .append_input(
                root,
                owner,
                vec![NewInputPart::Text("go".into())],
                InputDisposition::Pending,
                None,
                1,
            )
            .await
            .unwrap();
        wake.send(root).await.unwrap();
        Self {
            store,
            engine,
            brain,
            root,
            scheduler,
        }
    }

    async fn completions(&self) -> Vec<(String, ExecOutcome)> {
        self.store
            .messages_after(self.root, None)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|m| match m.body {
                MessageBody::Completion {
                    exec_id, outcome, ..
                } => Some((exec_id, outcome)),
                _ => None,
            })
            .collect()
    }

    /// 等到根会话收齐 `n` 条交付且全部 run 收尾。
    async fn settle(&self, n: usize) -> Vec<(String, ExecOutcome)> {
        wait(|| async {
            let done = self.completions().await;
            (done.len() >= n && self.store.executing_runs().await.unwrap().is_empty())
                .then_some(done)
        })
        .await
    }

    async fn agent(&self, exec_id: &str) -> (Job, SessionId) {
        let job = self
            .store
            .job(JobId(exec_id.parse().unwrap()))
            .await
            .unwrap()
            .unwrap();
        let JobKind::Agent { child_session_id } = job.kind else {
            panic!("不是 agent job");
        };
        (job, child_session_id)
    }

    fn seen(&self) -> Vec<Seen> {
        self.brain.seen.lock().unwrap().clone()
    }
}

/// 追问的来源：派出 `job` 的那个根会话 run。
fn origin(job: &Job) -> Origin {
    Origin {
        session: job.owner_session_id,
        run: job.origin_run_id,
        tool_call_id: "follow".into(),
        tool_name: "agent".into(),
    }
}

async fn wait<T, F: std::future::Future<Output = Option<T>>>(mut probe: impl FnMut() -> F) -> T {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            if let Some(v) = probe().await {
                return v;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("等待超时")
}

fn text(outcome: &ExecOutcome) -> &str {
    match outcome {
        ExecOutcome::Completed { output } => match &output[..] {
            [ContentPart::Text { text }] => text,
            _ => panic!("交付应为单段文字"),
        },
        ExecOutcome::Cancelled { message } | ExecOutcome::Failed { message, .. } => message,
    }
}

#[tokio::test]
async fn each_run_delivers_once_and_follow_up_delivers_again() {
    let fx = Fixture::new(script(&[("look", "find it")])).await;
    let done = fx.settle(1).await;
    assert_eq!(text(&done[0].1), "done: find it");
    let (job, child) = fx.agent(&done[0].0).await;

    fx.engine
        .jobs
        .clone()
        .follow_up(origin(&job), child, "and more".into())
        .await
        .unwrap();
    let done = fx.settle(2).await;
    assert_eq!(done.len(), 2);
    assert_eq!(done[0].0, done[1].0);
    assert_eq!(text(&done[1].1), "done: and more");

    let seen = fx.seen();
    let child_calls: Vec<_> = seen.iter().filter(|s| !s.root()).collect();
    assert_eq!(child_calls.len(), 2);
    let persona = fx
        .store
        .persona(fx.store.settings().await.unwrap().default_persona);
    let persona = persona.await.unwrap().unwrap().prompt;
    assert!(seen
        .iter()
        .find(|s| s.root())
        .unwrap()
        .system
        .contains(&persona));
    for call in child_calls {
        assert!(call.tools.iter().all(|t| t != "agent"));
        assert!(call.tools.iter().any(|t| t == "jobs"));
        assert!(!call.system.contains(&persona));
    }

    // trace 会话树与整树用量：跨 store/kernel 的递归 SQL。
    let task = fx.store.trace_sessions().await.unwrap();
    let task = task.iter().find(|s| s.session == child).unwrap();
    assert_eq!(task.parent, Some(job.owner_session_id));
    assert_eq!(task.label.as_deref(), Some(job.description.as_str()));
    let root = fx.store.session_usage(job.owner_session_id).await.unwrap();
    let own = fx.store.session_usage(child).await.unwrap().own;
    let tree = root.tree.unwrap();
    assert_eq!(tree.descendants, 1);
    assert_eq!(
        tree.usage.input_tokens,
        root.own.input_tokens + own.input_tokens
    );
    fx.scheduler.abort();
}

#[tokio::test]
async fn only_the_owner_can_follow_up() {
    let fx = Fixture::new(script(&[("look", "find it")])).await;
    let done = fx.settle(1).await;
    let (job, _) = fx.agent(&done[0].0).await;
    let tool = AgentTool {
        jobs: fx.engine.jobs.clone(),
        scope: ToolScope::All,
    };
    let mut other = origin(&job);
    other.session = SessionId(other.session.0 + 100);
    let err = tool.follow_up_target(&other, job.id).await.unwrap_err();
    assert!(err.to_string().contains("only the session that started"));
    fx.scheduler.abort();
}

#[tokio::test]
async fn user_stop_holds_queued_input_and_reports_once() {
    let fx = Fixture::new(script(&[("wait", "block")])).await;
    fx.brain.blocked.notified().await;
    let job = wait(|| async {
        fx.store
            .subtree_jobs(fx.root, None)
            .await
            .unwrap()
            .pop()
            .map(|(job, _)| job)
    })
    .await;
    let JobKind::Agent { child_session_id } = job.kind else {
        panic!("不是 agent job");
    };
    let origin = origin(&job);
    fx.store
        .follow_up_agent(child_session_id, origin.run, "queued".into(), now_ms())
        .await
        .unwrap();

    let outcome = fx.engine.jobs.stop(fx.root, job.id, true).await.unwrap();
    assert_eq!(outcome, StopJobOutcome::Stopped);
    let done = fx.settle(1).await;
    assert_eq!(text(&done[0].1), "Stopped by the user.");

    fx.engine
        .jobs
        .clone()
        .follow_up(origin, child_session_id, "again".into())
        .await
        .unwrap();
    let done = fx.settle(2).await;
    assert_eq!(text(&done[1].1), "done: again");
    assert!(fx.seen().iter().all(|s| s.last != "queued"));
    assert_eq!(
        fx.engine.jobs.stop(fx.root, job.id, true).await.unwrap(),
        StopJobOutcome::AlreadyIdle
    );
    fx.scheduler.abort();
}

#[tokio::test]
async fn owner_stop_writes_no_completion() {
    let fx = Fixture::new(script(&[("wait", "block")])).await;
    fx.brain.blocked.notified().await;
    let (job, _) =
        wait(|| async { fx.store.subtree_jobs(fx.root, None).await.unwrap().pop() }).await;
    let outcome = fx.engine.jobs.stop(fx.root, job.id, false).await.unwrap();
    assert_eq!(outcome, StopJobOutcome::Stopped);
    wait(|| async {
        fx.store
            .executing_runs()
            .await
            .unwrap()
            .is_empty()
            .then_some(())
    })
    .await;
    assert!(fx.completions().await.is_empty());
    fx.scheduler.abort();
}

#[tokio::test]
async fn recovery_interrupts_running_agents_with_one_notice() {
    let fx = Fixture::new(script(&[
        ("quick", "hello"),
        ("slow", "block"),
        ("slower", "block"),
    ]))
    .await;
    // 先等快的交付并让根会话收尾，再等两个慢的都挂在模型调用上。
    fx.settle_running(1, 2).await;
    // 三个派发并发执行，任务号不按调用顺序；被中断的是快的之外的两个。
    let quick = fx.completions().await.remove(0).0;
    let mut slow: Vec<_> = fx
        .store
        .subtree_jobs(fx.root, None)
        .await
        .unwrap()
        .into_iter()
        .map(|(job, _)| job.id.0)
        .filter(|id| id.to_string() != quick)
        .collect();
    slow.sort();
    fx.scheduler.abort();
    let _ = (&mut { fx.scheduler }).await;
    let (store, root) = (fx.store, fx.root);

    crate::recovery::recover(&store, std::env::temp_dir())
        .await
        .unwrap();
    let messages = store.messages_after(root, None).await.unwrap();
    let mut interrupted: Vec<_> = messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::Completion {
                exec_id,
                outcome: ExecOutcome::Cancelled { message },
                ..
            } if message == AGENT_INTERRUPTED => Some(exec_id.clone()),
            _ => None,
        })
        .collect();
    interrupted.sort();
    assert_eq!(
        interrupted,
        slow.iter().map(ToString::to_string).collect::<Vec<_>>()
    );
    let notices: Vec<_> = messages
        .iter()
        .filter_map(|m| match &m.body {
            MessageBody::Notification { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    assert_eq!(
        notices,
        [format!(
            "重启前仍在运行的后台任务已中断：子 agent #{}、子 agent #{}。",
            slow[0], slow[1]
        )]
    );
    assert!(messages.iter().all(|m| !matches!(
        &m.body,
        MessageBody::ToolResult {
            outcome: ToolResultOutcome::Terminal(_),
            ..
        }
    )));
}

impl Fixture {
    /// 等根会话收齐 `delivered` 条交付、且只剩 `running` 个子 run 在执行。
    async fn settle_running(&self, delivered: usize, running: usize) {
        wait(|| async {
            let executing = self.store.executing_runs().await.unwrap();
            let blocked = self
                .brain
                .seen
                .lock()
                .unwrap()
                .iter()
                .filter(|s| s.last == "block")
                .count();
            (self.completions().await.len() >= delivered
                && executing.len() == running
                && blocked == running)
                .then_some(())
        })
        .await
    }
}

#[test]
fn delivery_maps_terminal_states() {
    let (owner, child) = (SessionId(1), SessionId(2));
    let reply = |text: &str| Message {
        id: mic_message::MessageId(1),
        session_id: child,
        body: MessageBody::Reply {
            stats: mic_message::ReplyStats {
                model_ms: 0,
                usage: None,
            },
            model: "test".into(),
            blocks: vec![ReplyBlock::Text {
                text: text.into(),
                phase: None,
            }],
        },
        created_at: 0,
        delivered_at: None,
    };
    let empty = delivery(RunState::Completed, &[], None, owner, child).unwrap();
    assert!(matches!(
        empty,
        ExecOutcome::Failed {
            kind: ExecFailureKind::Business,
            ..
        }
    ));

    let capped = delivery(RunState::MaxTurns, &[reply("half")], None, owner, child).unwrap();
    assert!(text(&capped).starts_with("[The sub-agent used all its model calls"));
    assert!(text(&capped).ends_with("half"));

    let long = "x".repeat(AGENT_DELIVERY_MAX + 10);
    let clipped = delivery(RunState::Completed, &[reply(&long)], None, owner, child).unwrap();
    assert!(
        text(&clipped).ends_with("[truncated: 10 more bytes; the full reply is in sub-session 2]")
    );

    let timed = delivery(RunState::TimedOut, &[], None, owner, child).unwrap();
    assert!(text(&timed).contains("30-minute limit"));

    let by = |manager, by_user| Some(StopBy { manager, by_user });
    assert!(delivery(RunState::Stopped, &[], by(owner, false), owner, child).is_none());
    let other = delivery(
        RunState::Stopped,
        &[],
        by(SessionId(9), false),
        owner,
        child,
    )
    .unwrap();
    assert_eq!(text(&other), "Stopped by session 9.");
}
