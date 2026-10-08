use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mic_core::KernelError;
use mic_store::SettingsError;
use serde::Serialize;

#[derive(Serialize)]
struct ErrorBody {
    error: String,
}

/// 接口错误；响应体 `{"error": "<中文说明>"}`。
pub(crate) enum ApiError {
    Unauthorized,
    BadRequest(String),
    TooLarge,
    /// 不存在的资源说明，如「会话不存在」。
    NotFound(&'static str),
    /// 只读会话（非 web）不能发消息。
    ReadOnly,
    Forbidden(String),
    /// 与当前状态冲突，文案说明怎么处理。
    Conflict(String),
    /// 新会话工作目录无法使用，文案含路径与原因。
    Workdir(String),
    Internal(KernelError),
}

pub(crate) const SESSION_NOT_FOUND: &str = "会话不存在";
pub(crate) const PERSONA_NOT_FOUND: &str = "人设不存在";

impl From<KernelError> for ApiError {
    fn from(e: KernelError) -> Self {
        match e {
            KernelError::Settings(e) => match e {
                SettingsError::PersonaNotFound => Self::NotFound(PERSONA_NOT_FOUND),
                SettingsError::Builtin => Self::Forbidden(e.to_string()),
                SettingsError::IsDefault | SettingsError::NameTaken(_) | SettingsError::Deleted => {
                    Self::Conflict(e.to_string())
                }
                SettingsError::Store(e) => Self::Internal(e.into()),
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
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "消息太长".to_owned()),
            Self::NotFound(what) => (StatusCode::NOT_FOUND, what.to_owned()),
            Self::ReadOnly => (
                StatusCode::FORBIDDEN,
                "该会话来自其它渠道，Web 只能查看".to_owned(),
            ),
            Self::Forbidden(m) => (StatusCode::FORBIDDEN, m),
            Self::Conflict(m) => (StatusCode::CONFLICT, m),
            Self::Workdir(m) => {
                tracing::error!(error = %m, "web session workdir unusable");
                (StatusCode::INTERNAL_SERVER_ERROR, m)
            }
            Self::Internal(e) => {
                tracing::error!(error = %e, "gateway request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "内部错误".to_owned())
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}
