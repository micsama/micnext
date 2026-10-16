use std::sync::{Arc, Mutex};

use mic_tool::BoxFuture;

use super::*;

#[derive(Clone, Default)]
struct Captured(Arc<Mutex<Vec<u8>>>);

impl OutputSink for Captured {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, std::io::Result<()>> {
        self.0.lock().unwrap().extend_from_slice(bytes);
        Box::pin(async { Ok(()) })
    }
}

impl Captured {
    fn text(&self) -> String {
        String::from_utf8(self.0.lock().unwrap().clone()).unwrap()
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
