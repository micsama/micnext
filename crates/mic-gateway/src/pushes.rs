//! 通知中心：推送列表、已读、实时流与默认推送 Channel。

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, QueryRejection};
use axum::extract::{Query, State};
use axum::http::{header, StatusCode};
use axum::response::sse::Event;
use axum::response::IntoResponse;
use axum::Json;
use mic_core::{KernelEventKind, PushSettings};
use mic_store::{PushId, PushPage};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;

use crate::api::json;
use crate::error::ApiError;
use crate::limits::{PAGE_DEFAULT, PAGE_MAX, STREAM_BUFFER};
use crate::service::App;
use crate::sse;
use crate::ticket::StreamTarget;

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
    ids: Vec<i64>,
}

pub(crate) async fn read(
    State(app): State<Arc<App>>,
    body: Result<Json<ReadBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let body = json(body)?;
    app.kernel
        .mark_pushes_read(body.ids.into_iter().map(PushId).collect())
        .await?;
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

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct StreamQuery {
    ticket: Option<String>,
}

#[derive(Serialize)]
struct Created {
    id: PushId,
}

/// 只报新推送的 id；客户端据此重新拉取列表与未读数。
pub(crate) async fn stream(
    State(app): State<Arc<App>>,
    query: Result<Query<StreamQuery>, QueryRejection>,
) -> Result<impl IntoResponse, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    app.tickets
        .redeem(q.ticket.as_deref(), StreamTarget::Pushes)?;
    let mut events = app.kernel.subscribe();
    let (tx, rx) = mpsc::channel(STREAM_BUFFER);
    tokio::spawn(async move {
        loop {
            let event = tokio::select! {
                () = app.stop.cancelled() => return,
                () = tx.closed() => return,
                event = events.recv() => match event {
                    Ok(event) => event,
                    Err(lagged) => return tracing::warn!(%lagged, "push stream lagged"),
                },
            };
            let KernelEventKind::PushCreated { push_id } = event.kind else {
                continue;
            };
            let event = Event::default()
                .event("push_created")
                .json_data(Created { id: push_id })
                .expect("SSE 数据可序列化");
            if tx.send(event).await.is_err() {
                return;
            }
        }
    });
    Ok(([(header::CACHE_CONTROL, "no-store")], sse::respond(rx)))
}
