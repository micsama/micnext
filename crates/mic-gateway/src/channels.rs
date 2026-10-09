use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mic_core::{ChannelSetup, ChannelSetupView, SetupAttempt, SetupAttemptId};
use serde::{Deserialize, Serialize};

use crate::api::json;
use crate::error::ApiError;
use crate::service::App;

#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum WechatView {
    Available {
        available: bool,
        #[serde(flatten)]
        view: ChannelSetupView,
    },
    Unavailable {
        available: bool,
    },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct BeginBody {}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct CodeBody {
    code: String,
}

fn setup(app: &App) -> Result<Arc<dyn ChannelSetup>, ApiError> {
    app.kernel
        .channel_setup("wechat")
        .ok_or(ApiError::NotFound("本构建未包含微信"))
}

fn attempt_id(id: Result<Path<String>, PathRejection>) -> Result<SetupAttemptId, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound("登录不存在"))?;
    Ok(SetupAttemptId(id))
}

pub(crate) async fn status(State(app): State<Arc<App>>) -> Result<Json<WechatView>, ApiError> {
    Ok(Json(match app.kernel.channel_setup("wechat") {
        Some(setup) => WechatView::Available {
            available: true,
            view: setup.status().await?,
        },
        None => WechatView::Unavailable { available: false },
    }))
}

pub(crate) async fn begin(
    State(app): State<Arc<App>>,
    body: Result<Json<BeginBody>, JsonRejection>,
) -> Result<(StatusCode, Json<SetupAttempt>), ApiError> {
    json(body)?;
    Ok((StatusCode::ACCEPTED, Json(setup(&app)?.begin().await?)))
}

pub(crate) async fn submit_code(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
    body: Result<Json<CodeBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let body = json(body)?;
    setup(&app)?.submit_code(attempt_id(id)?, body.code).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn cancel(
    State(app): State<Arc<App>>,
    id: Result<Path<String>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    setup(&app)?.cancel(attempt_id(id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}
