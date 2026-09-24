use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use mic_core::KernelError;
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
    NotFound,
    /// 只读会话（非 web）不能发消息。
    ReadOnly,
    Internal(KernelError),
}

impl From<KernelError> for ApiError {
    fn from(e: KernelError) -> Self {
        Self::Internal(e)
    }
}

impl IntoResponse for ApiError {
    fn into_response(self) -> Response {
        let (status, message) = match self {
            Self::Unauthorized => (StatusCode::UNAUTHORIZED, "token 缺失或不正确".to_owned()),
            Self::BadRequest(m) => (StatusCode::BAD_REQUEST, m),
            Self::TooLarge => (StatusCode::PAYLOAD_TOO_LARGE, "消息太长".to_owned()),
            Self::NotFound => (StatusCode::NOT_FOUND, "会话不存在".to_owned()),
            Self::ReadOnly => (
                StatusCode::FORBIDDEN,
                "该会话来自其它渠道，Web 只能查看".to_owned(),
            ),
            Self::Internal(e) => {
                tracing::error!(error = %e, "gateway request failed");
                (StatusCode::INTERNAL_SERVER_ERROR, "内部错误".to_owned())
            }
        };
        (status, Json(ErrorBody { error: message })).into_response()
    }
}
