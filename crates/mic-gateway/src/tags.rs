//! 标签词表与会话标签。契约：docs/blueprints/session-tags.md。

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mic_message::SessionId;
use mic_store::{Tag, TagId, TagName};
use serde::{Deserialize, Serialize};

use crate::api::json;
use crate::error::{ApiError, SESSION_NOT_FOUND, TAG_NOT_FOUND};
use crate::service::App;

pub(crate) async fn list(State(app): State<Arc<App>>) -> Result<Json<Vec<Tag>>, ApiError> {
    Ok(Json(app.kernel.tags().await?))
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct NameBody {
    name: String,
}

#[derive(Serialize)]
pub(crate) struct Created {
    id: TagId,
}

pub(crate) async fn create(
    State(app): State<Arc<App>>,
    body: Result<Json<NameBody>, JsonRejection>,
) -> Result<Json<Created>, ApiError> {
    let name =
        TagName::parse(&json(body)?.name).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    Ok(Json(Created {
        id: app.kernel.create_tag(name).await?,
    }))
}

pub(crate) async fn rename(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<NameBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(TAG_NOT_FOUND))?;
    let name =
        TagName::parse(&json(body)?.name).map_err(|e| ApiError::BadRequest(e.to_string()))?;
    app.kernel.rename_tag(TagId(id), name).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(TAG_NOT_FOUND))?;
    app.kernel.delete_tag(TagId(id)).await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionTagsBody {
    tag_ids: Vec<TagId>,
}

pub(crate) async fn set_session(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<SessionTagsBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let body = json(body)?;
    app.kernel
        .set_session_tags(SessionId(id), body.tag_ids)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}
