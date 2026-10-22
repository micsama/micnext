//! 通知中心：推送列表、已读、实时流与默认推送 Channel。

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::Json;
use mic_core::PushSettings;
use mic_store::{PushId, PushPage};
use serde::Deserialize;

use crate::api::json;
use crate::error::ApiError;
use crate::limits::{PAGE_DEFAULT, PAGE_MAX};
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ListQuery {
    before: Option<i64>,
    limit: Option<u32>,
}

pub(crate) async fn list(
    State(app): State<Arc<App>>,
    query: Result<Query<ListQuery>, QueryRejection>,
) -> Result<Json<PushPage>, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    let limit = match q.limit {
        None => PAGE_DEFAULT,
        Some(n @ 1..=PAGE_MAX) => n,
        Some(_) => {
            return Err(ApiError::BadRequest(format!(
                "limit 须在 1 到 {PAGE_MAX} 之间"
            )))
        }
    };
    Ok(Json(app.kernel.pushes(q.before.map(PushId), limit).await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ReadBody {
    through: i64,
}

pub(crate) async fn read(
    State(app): State<Arc<App>>,
    body: Result<Json<ReadBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let body = json(body)?;
    app.kernel.mark_pushes_read(PushId(body.through)).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_settings(
    State(app): State<Arc<App>>,
) -> Result<Json<PushSettings>, ApiError> {
    Ok(Json(app.kernel.push_settings().await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsBody {
    channels: Vec<String>,
}

pub(crate) async fn put_settings(
    State(app): State<Arc<App>>,
    body: Result<Json<SettingsBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let body = json(body)?;
    app.kernel.set_push_channels(body.channels).await?;
    Ok(StatusCode::NO_CONTENT)
}
