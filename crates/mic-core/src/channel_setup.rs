use mic_message::SessionId;
use serde::{Deserialize, Serialize};

use crate::{BoxError, BoxFuture};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct SetupAttemptId(pub String);

#[derive(Clone, Debug, Serialize)]
pub struct LinkedChannel {
    pub account_id: String,
    pub user_id: String,
    pub session_id: SessionId,
    pub connection: ChannelConnection,
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelConnection {
    Connected,
    NeedsLogin,
    Faulted,
}

#[derive(Clone, Debug, Serialize)]
#[serde(tag = "state", rename_all = "snake_case")]
pub enum SetupProgress {
    Preparing,
    Waiting { qr_content: String },
    Scanned { qr_content: String },
    NeedsCode { qr_content: String },
    Expired,
    Cancelled,
    Connected { account: LinkedChannel },
    Failed { reason: SetupFailure },
}

#[derive(Clone, Debug, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SetupFailure {
    Network,
    VerificationBlocked,
    ExistingBinding,
    Protocol,
}

#[derive(Clone, Debug, Serialize)]
pub struct SetupAttempt {
    pub id: SetupAttemptId,
    pub progress: SetupProgress,
}

#[derive(Clone, Debug, Serialize)]
pub struct ChannelSetupView {
    pub account: Option<LinkedChannel>,
    pub login: Option<SetupAttempt>,
}

#[derive(Debug, thiserror::Error)]
pub enum ChannelSetupError {
    #[error("登录已失效，请重新获取二维码")]
    AttemptNotFound,
    #[error("当前登录状态不接受此操作")]
    WrongPhase,
    #[error("验证码须为 1 到 128 个字符")]
    InvalidCode,
    #[error("微信登录暂不可用")]
    Unavailable,
    #[error("微信登录内部错误")]
    Internal { source: BoxError },
}

pub trait ChannelSetup: Send + Sync + 'static {
    fn status(&self) -> BoxFuture<Result<ChannelSetupView, ChannelSetupError>>;
    fn begin(&self) -> BoxFuture<Result<SetupAttempt, ChannelSetupError>>;
    fn submit_code(
        &self,
        id: SetupAttemptId,
        code: String,
    ) -> BoxFuture<Result<(), ChannelSetupError>>;
    fn cancel(&self, id: SetupAttemptId) -> BoxFuture<Result<(), ChannelSetupError>>;
}
