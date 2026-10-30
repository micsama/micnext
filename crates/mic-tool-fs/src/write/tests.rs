use std::future::Future;
use std::sync::Arc;
use std::task::Poll;

use super::*;

struct NoBackground;

impl mic_tool::Background for NoBackground {
    fn dispatch(
        &self,
        _: mic_tool::BackgroundTask,
    ) -> mic_tool::BoxFuture<'_, Result<String, ToolError>> {
        panic!("write must not dispatch")
    }
}

#[test]
fn cancelled_queued_write_never_changes_the_file() {
    let runtime = tokio::runtime::Builder::new_current_thread()
        .max_blocking_threads(1)
        .build()
        .unwrap();
    runtime.block_on(async {
        let dir = tempfile::tempdir().unwrap();
        let target = dir.path().join("result");
        let ctx = ToolContext::new(dir.path().to_owned(), Arc::new(NoBackground), Arc::new([]));
        let (entered, ready) = tokio::sync::oneshot::channel();
        let (release, blocked) = std::sync::mpsc::channel();
        let blocker = tokio::task::spawn_blocking(move || {
            entered.send(()).unwrap();
            blocked.recv().unwrap();
        });
        ready.await.unwrap();
        let mut write = Box::pin(Write.execute(
            Args {
                file_path: "result".into(),
                content: "must not appear".into(),
            },
            &ctx,
        ));
        std::future::poll_fn(|cx| {
            assert!(write.as_mut().poll(cx).is_pending());
            Poll::Ready(())
        })
        .await;
        ctx.cancel();
        drop(write);
        release.send(()).unwrap();
        blocker.await.unwrap();
        ctx.wait_commits().await;
        assert!(!target.exists());
    });
}
