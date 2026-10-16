use super::*;
use mic_core::{BoxFuture, ChannelSetupError, ChannelSetupView, SetupAttempt, SetupAttemptId};
use std::sync::Mutex;

struct Setup {
    name: &'static str,
    seen: Arc<Mutex<Vec<&'static str>>>,
    fail: bool,
}
struct Restore {
    name: &'static str,
    seen: Arc<Mutex<Vec<&'static str>>>,
    fail: bool,
}
impl ChannelRestore for Restore {
    fn restore(self: Box<Self>) -> BoxFuture<Result<(), ChannelSetupError>> {
        Box::pin(async move {
            self.seen.lock().unwrap().push(self.name);
            if self.fail {
                Err(ChannelSetupError::Unavailable)
            } else {
                Ok(())
            }
        })
    }
}
impl ChannelSetup for Setup {
    fn prepare_restore(
        &self,
        value: Option<serde_json::Value>,
    ) -> Result<Box<dyn ChannelRestore>, ChannelSetupError> {
        if value.is_some_and(|v| v != serde_json::json!({"valid":true})) {
            return Err(ChannelSetupError::BackupInvalid {
                reason: "fields".into(),
            });
        }
        Ok(Box::new(Restore {
            name: self.name,
            seen: self.seen.clone(),
            fail: self.fail,
        }))
    }
    fn export(&self) -> BoxFuture<Result<Option<serde_json::Value>, ChannelSetupError>> {
        unimplemented!()
    }
    fn status(&self) -> BoxFuture<Result<ChannelSetupView, ChannelSetupError>> {
        unimplemented!()
    }
    fn begin(&self) -> BoxFuture<Result<SetupAttempt, ChannelSetupError>> {
        unimplemented!()
    }
    fn submit_code(
        &self,
        _: SetupAttemptId,
        _: String,
    ) -> BoxFuture<Result<(), ChannelSetupError>> {
        unimplemented!()
    }
    fn cancel(&self, _: SetupAttemptId) -> BoxFuture<Result<(), ChannelSetupError>> {
        unimplemented!()
    }
}

fn setups(seen: Arc<Mutex<Vec<&'static str>>>) -> BTreeMap<&'static str, Arc<dyn ChannelSetup>> {
    [("first", true), ("second", false)]
        .into_iter()
        .map(|(name, fail)| {
            (
                name,
                Arc::new(Setup {
                    name,
                    fail,
                    seen: seen.clone(),
                }) as Arc<dyn ChannelSetup>,
            )
        })
        .collect()
}

#[tokio::test]
async fn preparation_has_no_effect_and_runtime_failure_does_not_skip_remaining_channels() {
    let seen = Arc::new(Mutex::new(vec![]));
    for accounts in [
        BTreeMap::from([("unknown".into(), serde_json::json!({}))]),
        BTreeMap::from([("second".into(), serde_json::json!({"bad":true}))]),
    ] {
        assert!(prepare_channels(setups(seen.clone()), accounts).is_err());
        assert!(seen.lock().unwrap().is_empty());
    }
    let prepared = prepare_channels(setups(seen.clone()), BTreeMap::new())
        .unwrap_or_else(|_| panic!("prepare failed"));
    assert!(seen.lock().unwrap().is_empty());
    let error = restore_channels(prepared).await.err().unwrap();
    assert!(
        matches!(error, ApiError::ImportPartial(message) if message.contains("设置已导入") && message.contains("first"))
    );
    assert_eq!(*seen.lock().unwrap(), vec!["first", "second"]);
}
