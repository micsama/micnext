use axum::extract::rejection::PathRejection;
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mic_message::{MessageId, SessionCommand, SessionId};
use serde::Serialize;
use std::sync::Arc;

use crate::api;
use crate::error::{ApiError, SESSION_NOT_FOUND};
use crate::service::App;

#[derive(Serialize)]
pub(crate) struct Commands {
    items: &'static [mic_core::CommandSpec],
}

pub(crate) async fn commands() -> Json<Commands> {
    Json(Commands {
        items: mic_core::session_commands(),
    })
}

pub(crate) async fn skills(
    State(app): State<Arc<App>>,
) -> Result<Json<mic_core::SkillList>, ApiError> {
    Ok(Json(app.kernel.skills().await?))
}

pub(crate) async fn get_context(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<mic_core::ContextOverview>, ApiError> {
    let session = api::session(&app, id).await?;
    Ok(Json(app.kernel.context_view(session.id).await?))
}

pub(crate) async fn get_breakdown(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<Json<Option<mic_core::ContextBreakdown>>, ApiError> {
    let session = api::session(&app, id).await?;
    Ok(Json(app.kernel.context_breakdown(session.id).await?))
}

pub(crate) async fn revoke(
    State(app): State<Arc<App>>,
    path: Result<Path<(i64, i64)>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    let Path((session_id, boundary_id)) =
        path.map_err(|_| ApiError::NotFound(SESSION_NOT_FOUND))?;
    let session = app
        .kernel
        .session(SessionId(session_id))
        .await?
        .ok_or(ApiError::NotFound(SESSION_NOT_FOUND))?;
    if !api::writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    app.kernel
        .append_session_command(
            session.id,
            app.kernel.owner(),
            SessionCommand::RevokeCompaction {
                boundary_id: MessageId(boundary_id),
            },
        )
        .await?;
    Ok(StatusCode::ACCEPTED)
}
