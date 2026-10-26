use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mic_core::{
    ChannelSetupError, InputError, JobAccessError, KernelError, MediaError, WorkdirError,
};
use mic_store::{DiagnosticError, ModelSettingsError, SettingsError, TagError};
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
    ImportPartial(String),
    Internal(KernelError),
    Setup(ChannelSetupError),
}

pub(crate) const SESSION_NOT_FOUND: &str = "会话不存在";
pub(crate) const PERSONA_NOT_FOUND: &str = "人设不存在";
pub(crate) const IMAGE_NOT_FOUND: &str = "图片不存在";
pub(crate) const FILE_NOT_FOUND: &str = "文件不存在";
pub(crate) const FILE_CLEANED: &str = "文件已清理";
pub(crate) const MODEL_NOT_FOUND: &str = "模型不存在";
pub(crate) const ENDPOINT_NOT_FOUND: &str = "服务商不存在";
pub(crate) const JOB_NOT_FOUND: &str = "任务不存在";
pub(crate) const TAG_NOT_FOUND: &str = "标签不存在";

impl From<ChannelSetupError> for ApiError {
    fn from(error: ChannelSetupError) -> Self {
        Self::Setup(error)
    }
}

impl From<KernelError> for ApiError {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::Schedule(error) => match error {
                mic_core::ScheduleError::Unavailable => Self::Unavailable(error.to_string()),
                mic_core::ScheduleError::NotFound => Self::NotFound("定时任务不存在"),
                mic_core::ScheduleError::Invalid(reason) => Self::Unprocessable(reason),
                mic_core::ScheduleError::Store(e) => Self::Internal(e.into()),
            },
            KernelError::Title(error) => match error {
                mic_core::TitleError::NotRoot => Self::Forbidden(error.to_string()),
                mic_core::TitleError::Generating => Self::Conflict(error.to_string()),
                mic_core::TitleError::EmptyInput => Self::BadRequest(error.to_string()),
                mic_core::TitleError::Stopped => Self::Unavailable(error.to_string()),
            },
            KernelError::CommandParse(error) => Self::BadRequest(error.to_string()),
            KernelError::ContextCommand(error) => match error {
                mic_store::ContextCommandError::BoundaryNotFound => {
                    Self::NotFound("压缩边界不存在")
                }
                mic_store::ContextCommandError::NotCurrentCompaction => {
                    Self::Conflict(error.to_string())
                }
                mic_store::ContextCommandError::Store(error) => Self::Internal(error.into()),
            },
            KernelError::Store(mic_store::StoreError::SessionArchived) => {
                Self::Conflict(e.to_string())
            }
            KernelError::Store(mic_store::StoreError::MessageNotInSession) => {
                Self::BadRequest(e.to_string())
            }
            KernelError::BackupInvalid(reason) => Self::BadRequest(reason),
            KernelError::SessionNotFound => Self::NotFound(SESSION_NOT_FOUND),
            KernelError::FileCleaned => Self::NotFound(FILE_CLEANED),
            KernelError::SessionNotRoot | KernelError::TaskSessionInput => {
                Self::Forbidden(e.to_string())
            }
            KernelError::Workdir(e) => {
                let hint = match &e {
                    WorkdirError::HomeMissing => "请在 设置 → 对话偏好 改用绝对路径",
                    WorkdirError::NonUtf8 { .. } => "请在 设置 → 对话偏好 修改",
                    WorkdirError::Create { .. } => "请在 设置 → 对话偏好 改为可写的目录",
                };
                Self::Workdir(format!("{e}。{hint}"))
            }
            KernelError::Settings(e) => match e {
                SettingsError::ImportInvalid(reason) => Self::BadRequest(reason),
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
                ModelSettingsError::InvalidSecretEdit | ModelSettingsError::InvalidPrice => {
                    Self::Unprocessable(e.to_string())
                }
                ModelSettingsError::Deleted
                | ModelSettingsError::EndpointDeleted
                | ModelSettingsError::NameTaken(_)
                | ModelSettingsError::DefaultInUse => Self::Conflict(e.to_string()),
                ModelSettingsError::Store(e) => Self::Internal(e.into()),
            },
            KernelError::Tag(e) => match e {
                TagError::NotFound => Self::NotFound(TAG_NOT_FOUND),
                TagError::Empty | TagError::TooLong | TagError::Separator | TagError::TooMany => {
                    Self::BadRequest(e.to_string())
                }
                TagError::NameTaken(_) => Self::Conflict(e.to_string()),
                TagError::Store(mic_store::StoreError::SessionArchived) => {
                    Self::Conflict(e.to_string())
                }
                TagError::Store(e) => Self::Internal(e.into()),
            },
            KernelError::Input(e) => match e {
                InputError::Empty => Self::BadRequest(e.to_string()),
                InputError::Image(MediaError::TooManyBytes) => Self::TooLarge(e.to_string()),
                InputError::ImageLimit | InputError::Image(_) => Self::Unprocessable(e.to_string()),
                InputError::FileSave => Self::Unavailable(e.to_string()),
            },
            KernelError::Config(e) if e.field == "reasoning_effort" => {
                Self::BadRequest(e.to_string())
            }
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
            KernelError::Job(e) => match e {
                JobAccessError::NotFound => Self::NotFound(JOB_NOT_FOUND),
                JobAccessError::Forbidden => Self::Forbidden(e.to_string()),
                JobAccessError::ReadLimit | JobAccessError::AgentOffset => {
                    Self::BadRequest(e.to_string())
                }
                JobAccessError::Io(_) | JobAccessError::Store(_) => {
                    Self::Internal(KernelError::Job(e))
                }
            },
            KernelError::Push(mic_core::PushError::UnknownChannel(_)) => {
                Self::BadRequest(e.to_string())
            }
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
            Self::ImportPartial(message) => (StatusCode::INTERNAL_SERVER_ERROR, message),
            Self::Internal(e) => {
                tracing::error!(error = %crate::error::chain(&e), "gateway request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "内部错误".to_owned())
            }
            Self::Setup(e) => {
                let status = match &e {
                    ChannelSetupError::AttemptNotFound => StatusCode::NOT_FOUND,
                    ChannelSetupError::WrongPhase => StatusCode::CONFLICT,
                    ChannelSetupError::BackupInvalid { .. } => StatusCode::BAD_REQUEST,
                    ChannelSetupError::InvalidCode => StatusCode::BAD_REQUEST,
                    ChannelSetupError::Unavailable => StatusCode::SERVICE_UNAVAILABLE,
                    ChannelSetupError::Internal { source } => {
                        tracing::error!(error = %crate::error::chain(source.as_ref()), "channel setup failed");
                        StatusCode::INTERNAL_SERVER_ERROR
                    }
                };
                (status, e.to_string())
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}

pub(crate) fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !message.ends_with(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = cause.source();
    }
    message
}
