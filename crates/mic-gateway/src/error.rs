use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mic_core::{ChannelSetupError, InputError, KernelError, MediaError, WorkdirError};
use mic_store::{DiagnosticError, ModelSettingsError, SettingsError};
use serde::Serialize;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

/// 接口错误；响应体 `{"error": "<中文说明>"}`。
pub(crate) enum ApiError {
    Unauthorized,
    BadRequest(String),
    /// 413，文案说明哪一项超限。
    TooLarge(String),
    /// 不存在的资源说明，如「会话不存在」。
    NotFound(&'static str),
    /// 只读会话（非 web）不能发消息。
    ReadOnly,
    Forbidden(String),
    /// 与当前状态冲突，文案说明怎么处理。
    Conflict(String),
    /// 语法合法但语义不被接受，如模型配置校验失败。
    Unprocessable(String),
    /// 当前部署不提供该能力。
    Unavailable(String),
    /// 新会话工作目录无法使用，文案含路径与原因。
    Workdir(String),
    Internal(KernelError),
    Setup(ChannelSetupError),
}

pub(crate) const SESSION_NOT_FOUND: &str = "会话不存在";
pub(crate) const PERSONA_NOT_FOUND: &str = "人设不存在";
pub(crate) const IMAGE_NOT_FOUND: &str = "图片不存在";
pub(crate) const MODEL_NOT_FOUND: &str = "模型不存在";
pub(crate) const ENDPOINT_NOT_FOUND: &str = "服务商不存在";

impl From<ChannelSetupError> for ApiError {
    fn from(error: ChannelSetupError) -> Self {
        Self::Setup(error)
    }
}

impl From<KernelError> for ApiError {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::SessionNotFound => Self::NotFound(SESSION_NOT_FOUND),
            KernelError::Workdir(e) => {
                let hint = match &e {
                    WorkdirError::HomeMissing => "请在 设置 → 对话偏好 改用绝对路径",
                    WorkdirError::NonUtf8 { .. } => "请在 设置 → 对话偏好 修改",
                    WorkdirError::Create { .. } => "请在 设置 → 对话偏好 改为可写的目录",
                };
                Self::Workdir(format!("{e}。{hint}"))
            }
            KernelError::Settings(e) => match e {
                SettingsError::PersonaNotFound => Self::NotFound(PERSONA_NOT_FOUND),
                SettingsError::Builtin => Self::Forbidden(e.to_string()),
                SettingsError::IsDefault | SettingsError::NameTaken(_) | SettingsError::Deleted => {
                    Self::Conflict(e.to_string())
                }
                SettingsError::Store(e) => Self::Internal(e.into()),
            },
            KernelError::ModelSettings(e) => match e {
                ModelSettingsError::NotFound => Self::NotFound(MODEL_NOT_FOUND),
                ModelSettingsError::EndpointNotFound => Self::NotFound(ENDPOINT_NOT_FOUND),
                ModelSettingsError::InvalidSecretEdit => Self::Unprocessable(e.to_string()),
                ModelSettingsError::Deleted
                | ModelSettingsError::EndpointDeleted
                | ModelSettingsError::NameTaken(_)
                | ModelSettingsError::DefaultInUse
                | ModelSettingsError::SessionExecuting => Self::Conflict(e.to_string()),
                ModelSettingsError::Store(e) => Self::Internal(e.into()),
            },
            KernelError::Input(e) => match e {
                InputError::Empty => Self::BadRequest(e.to_string()),
                InputError::Image(MediaError::TooManyBytes) => Self::TooLarge(e.to_string()),
                InputError::ImageLimit | InputError::Image(_) => Self::Unprocessable(e.to_string()),
            },
            KernelError::Config(e) => Self::Unprocessable(e.to_string()),
            KernelError::Probe(e) => Self::Unprocessable(e.to_string()),
            KernelError::Diagnostic(e) => match e {
                DiagnosticError::Busy => Self::Conflict(e.to_string()),
                DiagnosticError::Timeout | DiagnosticError::Rejected(_) => {
                    Self::BadRequest(e.to_string())
                }
                DiagnosticError::Unavailable => Self::Unavailable(e.to_string()),
                DiagnosticError::Store(e) => Self::Internal(e.into()),
            },
            e => Self::Internal(e),
        }
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "token 缺失或不正确".to_owned()),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            Self::TooLarge(m) => (StatusCode::PAYLOAD_TOO_LARGE, m),
            Self::NotFound(what) => (StatusCode::NOT_FOUND, what.to_owned()),
            Self::ReadOnly => (
                StatusCode::FORBIDDEN,
                "该会话来自其它渠道，Web 只能查看".to_owned(),
            ),
            Self::Unprocessable(m) => (StatusCode::UNPROCESSABLE_ENTITY, m),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            Self::Conflict(m) => (StatusCode::CONFLICT, m),
            Self::Unavailable(m) => (StatusCode::SERVICE_UNAVAILABLE, m),
            Self::Workdir(m) => {
                tracing::error!(error = %m, "web session workdir unusable");
                (StatusCode::INTERNAL_SERVER_ERROR, m)
            }
            Self::Internal(e) => {
                tracing::error!(error = %e, "gateway request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "内部错误".to_owned())
            }
            Self::Setup(e) => {
                let status = match &e {
                    ChannelSetupError::AttemptNotFound => StatusCode::NOT_FOUND,
                    ChannelSetupError::WrongPhase => StatusCode::CONFLICT,
                    ChannelSetupError::InvalidCode => StatusCode::BAD_REQUEST,
                    ChannelSetupError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                    ChannelSetupError::Internal { source } => {
                        tracing::error!(error = %source, "channel setup failed");
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, e.to_string())
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}
