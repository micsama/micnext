use std::time::Duration;

use mic_message::ContentPart;
use mic_store::{InputDisposition, NewInputPart, NewSession, ToolScope};
use mic_tool::OutputSink;

use super::*;

struct Fixture {
    store: Store,
    jobs: Arc<Jobs>,
    wakes: mpsc::Receiver<SessionId>,
    session: SessionId,
    run: RunId,
    dir: PathBuf,
}

impl Fixture {
    async fn new() -> Self {
        let store = Store::open_in_memory(&[]).await.unwrap();
        let owner = store.ensure_person("test", 0).await.unwrap();
        let session = store
            .create_session(NewSession {
                kind: SessionKind::Root {
                    channel: "web".into(),
                    chat: "jobs".into(),
                },
                parent_session_id: None,
                delivery_target: None,
                pwd: "/tmp".into(),
                tool_scope: ToolScope::All,
                created_at: 0,
                persona: None,
                model: None,
                reasoning_effort: None,
            })
            .await
            .unwrap();
        store
            .append_input(
                session.id,
                owner,
                vec![NewInputPart::Text("go".into())],
                InputDisposition::Pending,
                None,
                1,
            )
            .await
            .unwrap();
        let run = store
            .claim_next(session.id, 2)
            .await
            .unwrap()
            .unwrap()
            .run
            .id;
        let dir = std::env::temp_dir().join(format!(
            "micnext-jobs-{}-{}",
            std::process::id(),
            now_ms() as u128 * 1000 + rand_suffix()
        ));
        let (wake, wakes) = mpsc::channel(16);
        let jobs = Arc::new(Jobs::new(
            store.clone(),
            Events::new(),
            Some(wake),
            dir.clone(),
        ));
        Self {
            store,
            jobs,
            wakes,
            session: session.id,
            run,
            dir,
        }
    }

    fn port(&self, call: &str) -> JobPort {
        JobPort::new(
            self.jobs.clone(),
            Origin {
                session: self.session,
                run: self.run,
                tool_call_id: call.into(),
                tool_name: "bash".into(),
            },
        )
    }

    async fn completions(&self) -> Vec<ExecOutcome> {
        self.store
            .messages_after(self.session, None)
            .await
            .unwrap()
            .into_iter()
            .filter_map(|m| match m.body {
                MessageBody::Completion { outcome, .. } => Some(outcome),
                _ => None,
            })
            .collect()
    }

    async fn state(&self, id: JobId) -> JobState {
        self.store.job(id).await.unwrap().unwrap().state
    }
}

fn rand_suffix() -> u128 {
    static NEXT: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
    NEXT.fetch_add(1, std::sync::atomic::Ordering::Relaxed) as u128
}

/// 写一段输出后按 `then` 结束；`None` 永不结束。
fn task(timeout: Option<Duration>, out: &'static str, then: Option<ExecOutcome>) -> BackgroundTask {
    BackgroundTask {
        description: "test".into(),
        timeout,
        run: Box::new(move |mut sink: Box<dyn OutputSink>| {
            Box::pin(async move {
                sink.write(out.as_bytes()).await.unwrap();
                match then {
                    Some(outcome) => outcome,
                    None => std::future::pending().await,
                }
            })
        }),
    }
}

fn exit0() -> Option<ExecOutcome> {
    Some(ExecOutcome::Completed {
        output: vec![ContentPart::Text {
            text: "[exit code: 0]".into(),
        }],
    })
}

fn texts(outcome: &ExecOutcome) -> String {
    match outcome {
        ExecOutcome::Completed { output } => output
            .iter()
            .map(|p| match p {
                ContentPart::Text { text } => text.as_str(),
                _ => "",
            })
            .collect::<Vec<_>>()
            .join("\n"),
        ExecOutcome::Failed { message, .. } | ExecOutcome::Cancelled { message } => message.clone(),
    }
}

async fn wait_finished(f: &Fixture, id: JobId) {
    for _ in 0..200 {
        if f.state(id).await != JobState::Running {
            return;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    panic!("job {id:?} did not finish");
}

#[tokio::test]
async fn completion_reports_tail_once_and_wakes_owner() {
    let mut f = Fixture::new().await;
    let port = f.port("c1");
    let exec_id = port.dispatch(task(None, "hello\n", exit0())).await.unwrap();
    let id = JobId(exec_id.parse().unwrap());
    assert_eq!(port.registered().await, Some(id));
    assert!(port.dispatch(task(None, "", exit0())).await.is_err());
    wait_finished(&f, id).await;
    assert_eq!(f.wakes.recv().await, Some(f.session));
    let reports = f.completions().await;
    assert_eq!(reports.len(), 1);
    let text = texts(&reports[0]);
    assert!(text.contains("[exit code: 0]") && text.contains("[output: 6 bytes]\nhello\n"));
    assert_eq!(f.store.job_by_origin(f.run, "c1").await.unwrap(), Some(id));
    let (_, out) = f.jobs.output(f.session, id, Some(0), 3).await.unwrap();
    assert_eq!(
        (out.text.as_str(), out.next_offset, out.eof),
        ("hel", 3, false)
    );
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn timeout_and_stops_settle_exactly_once() {
    let f = Fixture::new().await;
    let timed = JobId(
        f.port("t")
            .dispatch(task(Some(Duration::from_millis(30)), "x", None))
            .await
            .unwrap()
            .parse()
            .unwrap(),
    );
    wait_finished(&f, timed).await;
    assert_eq!(f.state(timed).await, JobState::TimedOut);

    let by_model = JobId(
        f.port("m")
            .dispatch(task(None, "", None))
            .await
            .unwrap()
            .parse()
            .unwrap(),
    );
    let (a, b) = tokio::join!(
        f.jobs.stop(f.session, by_model, false),
        f.jobs.stop(f.session, by_model, false)
    );
    let mut outcomes = [a.unwrap(), b.unwrap()];
    outcomes.sort_by_key(|o| *o == StopJobOutcome::Stopped);
    assert_eq!(
        outcomes,
        [StopJobOutcome::AlreadyFinished, StopJobOutcome::Stopped]
    );
    assert_eq!(f.state(by_model).await, JobState::Cancelled);

    let by_user = JobId(
        f.port("u")
            .dispatch(task(None, "", None))
            .await
            .unwrap()
            .parse()
            .unwrap(),
    );
    assert_eq!(
        f.jobs.stop(f.session, by_user, true).await.unwrap(),
        StopJobOutcome::Stopped
    );
    let reports = f.completions().await;
    // 超时与用户终止各一条，模型终止不回注。
    assert_eq!(reports.len(), 2);
    assert!(texts(&reports[0]).contains("[timed out after 30ms]"));
    assert!(texts(&reports[1]).starts_with("Stopped by the user."));
    assert!(matches!(
        f.jobs.stop(f.session, JobId(999), true).await,
        Err(JobAccessError::NotFound)
    ));
    let _ = std::fs::remove_dir_all(&f.dir);
}

#[tokio::test]
async fn shutdown_leaves_running_for_recovery() {
    let f = Fixture::new().await;
    let id = JobId(
        f.port("r")
            .dispatch(task(None, "partial", None))
            .await
            .unwrap()
            .parse()
            .unwrap(),
    );
    tokio::time::sleep(Duration::from_millis(20)).await;
    f.jobs.shutdown().await;
    assert_eq!(f.state(id).await, JobState::Running);
    assert_eq!(recover(&f.store, f.dir.clone()).await.unwrap(), 1);
    assert_eq!(f.state(id).await, JobState::Interrupted);
    let reports = f.completions().await;
    assert_eq!(reports.len(), 1);
    assert!(texts(&reports[0]).contains("partial"));
    assert_eq!(recover(&f.store, f.dir.clone()).await.unwrap(), 0);
    let _ = std::fs::remove_dir_all(&f.dir);
}
