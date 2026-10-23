use std::sync::{Arc, Mutex};

use mic_tool::BoxFuture;

use super::*;

#[derive(Clone, Default)]
struct Captured {
    output: Arc<Mutex<Vec<u8>>>,
    notices: Arc<Mutex<Vec<String>>>,
}

impl OutputSink for Captured {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, std::io::Result<()>> {
        self.output.lock().unwrap().extend_from_slice(bytes);
        Box::pin(async { Ok(()) })
    }

    fn notify<'a>(&'a mut self, text: &'a str) -> BoxFuture<'a, std::io::Result<()>> {
        self.notices.lock().unwrap().push(text.to_owned());
        Box::pin(async { Ok(()) })
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.output.lock().unwrap().clone()).unwrap()
    }

    fn notices(&self) -> Vec<String> {
        self.notices.lock().unwrap().clone()
    }
}

fn marker(outcome: ExecOutcome) -> String {
    match outcome {
        ExecOutcome::Completed { output } => match &output[..] {
            [ContentPart::Text { text }] => text.clone(),
            _ => panic!("unexpected output"),
        },
        other => panic!("unexpected outcome: {other:?}"),
    }
}

fn alive(pid: i32) -> bool {
    nix::sys::signal::kill(Pid::from_raw(pid), None).is_ok()
}

#[tokio::test]
async fn background_merges_output_and_reports_exit() {
    let sink = Captured::default();
    let outcome = run_background(
        "echo out; echo err >&2; exit 3".into(),
        std::env::temp_dir(),
        None,
        Box::new(sink.clone()),
    )
    .await;
    assert_eq!(marker(outcome), "[exit code: 3]");
    let text = sink.text();
    assert!(text.contains("out\n") && text.contains("err\n"));
}

#[tokio::test]
async fn dropping_background_kills_the_process_group() {
    let sink = Captured::default();
    let mut run = Box::pin(run_background(
        "sleep 30 & echo $!; wait".into(),
        std::env::temp_dir(),
        None,
        Box::new(sink.clone()),
    ));
    let pid = loop {
        tokio::select! {
            _ = &mut run => panic!("should keep running"),
            () = tokio::time::sleep(Duration::from_millis(20)) => {}
        }
        if let Ok(pid) = sink.text().trim().parse::<i32>() {
            break pid;
        }
    };
    assert!(alive(pid));
    drop(run);
    tokio::time::sleep(Duration::from_millis(100)).await;
    assert!(!alive(pid));
}

#[test]
fn background_timeout_defaults_and_unlimited() {
    assert_eq!(
        parse_timeout(None, true).unwrap(),
        Some(BACKGROUND_TIMEOUT_MS)
    );
    assert_eq!(parse_timeout(Some(0), true).unwrap(), None);
    assert_eq!(
        parse_timeout(Some(3_600_000), true).unwrap(),
        Some(3_600_000)
    );
    assert!(parse_timeout(Some(0), false).is_err());
    assert_eq!(
        parse_timeout(Some(3_600_000), false).unwrap(),
        Some(MAX_TIMEOUT_MS)
    );
}

struct NoBackground;

impl mic_tool::Background for NoBackground {
    fn dispatch(&self, _: BackgroundTask) -> BoxFuture<'_, Result<String, ToolError>> {
        panic!("foreground must not dispatch")
    }
}

#[tokio::test]
async fn foreground_timeout_has_a_terminal_timeout_with_partial_output() {
    let ctx = ToolContext::new(std::env::temp_dir(), std::sync::Arc::new(NoBackground));
    let args = Args {
        command: "echo before; sleep 30".into(),
        description: "timeout".into(),
        timeout_ms: Some(100),
        workdir: None,
        background: false,
        notify_pattern: None,
    };
    let result = Bash.execute(args, &ctx).await.unwrap();
    let ToolOutput::Terminal(ExecOutcome::TimedOut { output }) = result else {
        panic!("timeout expected")
    };
    assert!(
        matches!(&output[..], [ContentPart::Text { text }] if text.contains("before") && text.contains("timed out after 100ms"))
    );
}

#[tokio::test]
async fn background_notifies_matching_lines_before_exit() {
    let sink = Captured::default();
    let outcome = run_background(
        "echo starting; echo 'listening on :8080' >&2; printf 'ready'".into(),
        std::env::temp_dir(),
        parse_pattern(Some("listening|ready".into()), true).unwrap(),
        Box::new(sink.clone()),
    )
    .await;
    assert_eq!(marker(outcome), "[exit code: 0]");
    // 第二次命中落在冷却内被丢弃。
    assert_eq!(
        sink.notices(),
        ["stderr matched notify_pattern (notice 1/10):\nlistening on :8080"]
    );
}
