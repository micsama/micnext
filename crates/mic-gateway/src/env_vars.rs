//! 命令环境变量：值只返回遮住中段的样子。契约：docs/blueprints/env-vars.md。

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mic_core::EnvVarView;
use mic_store::SecretValue;
use serde::Deserialize;

use crate::api::json;
use crate::error::ApiError;
use crate::service::App;

pub(crate) async fn list(State(app): State<Arc<App>>) -> Json<Vec<EnvVarView>> {
    Json(app.kernel.env_vars())
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ValueBody {
    value: String,
}

pub(crate) async fn put(
    State(app): State<Arc<App>>,
    name: Result<Path<String>, PathRejection>,
    body: Result<Json<ValueBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(name) = name.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    let value = SecretValue::new(json(body)?.value);
    app.kernel.set_env_var(name, value).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete(
    State(app): State<Arc<App>>,
    name: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(name) = name.map_err(|e| ApiError::BadRequest(e.to_string()))?;
    app.kernel.delete_env_var(name).await?;
    Ok(StatusCode::NO_CONTENT)
}
