//! 服务商与模型接口。契约：docs/blueprints/model-settings.md §十。

use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mic_core::{EndpointDraft, ModelDraft};
use mic_store::{CredentialWrite, EndpointId, EndpointView, ModelId, ModelView, SecretValue};
use serde::{Deserialize, Serialize};
use serde_json::value::RawValue;

use crate::api::{self, json};
use crate::error::{ApiError, ENDPOINT_NOT_FOUND, MODEL_NOT_FOUND};
use crate::limits::{API_KEY_MAX_CHARS, ENDPOINT_NAME_MAX_CHARS, MODEL_NAME_MAX_CHARS};
use crate::service::App;

#[derive(Serialize)]
pub(crate) struct KindItem {
    kind: String,
    display_name: String,
}

#[derive(Serialize)]
pub(crate) struct KindList {
    items: Vec<KindItem>,
}

#[derive(Serialize)]
pub(crate) struct EndpointItem {
    id: EndpointId,
    name: String,
    kind: String,
    /// 该 kind 的连接配置，无秘密。
    config: Box<RawValue>,
    /// API key 只暴露是否已设置。
    key_set: bool,
    /// 未保存 key 时读取的环境变量名。
    key_env: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct EndpointList {
    items: Vec<EndpointItem>,
}

#[derive(Serialize)]
pub(crate) struct ModelItem {
    id: ModelId,
    endpoint_id: EndpointId,
    name: String,
    /// 该 kind 的模型参数。
    config: Box<RawValue>,
}

#[derive(Serialize)]
pub(crate) struct ModelList {
    items: Vec<ModelItem>,
    default_model_id: Option<ModelId>,
}

#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "lowercase", deny_unknown_fields)]
pub(crate) enum CredentialBody {
    Keep,
    Clear,
    Set { value: String },
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct EndpointBody {
    name: String,
    kind: String,
    config: Box<RawValue>,
    credential: CredentialBody,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct TestBody {
    /// 编辑已有服务商时带上，`Keep` 才有 key 可沿用。
    endpoint_id: Option<EndpointId>,
    kind: String,
    config: Box<RawValue>,
    credential: CredentialBody,
}

#[derive(Serialize)]
pub(crate) struct TestResult {
    models: Vec<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelBody {
    endpoint_id: EndpointId,
    name: String,
    config: Box<RawValue>,
}

#[derive(Serialize)]
pub(crate) struct EndpointCreated {
    id: EndpointId,
}

#[derive(Serialize)]
pub(crate) struct ModelCreated {
    id: ModelId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ModelRef {
    model_id: ModelId,
}

#[derive(Serialize)]
pub(crate) struct DefaultView {
    model_id: Option<ModelId>,
}

pub(crate) async fn list_kinds(State(app): State<Arc<App>>) -> Json<KindList> {
    let items = app
        .kernel
        .provider_kinds()
        .into_iter()
        .map(|k| KindItem {
            kind: k.kind,
            display_name: k.display_name,
        })
        .collect();
    Json(KindList { items })
}

pub(crate) async fn list_endpoints(
    State(app): State<Arc<App>>,
) -> Result<Json<EndpointList>, ApiError> {
    let items = app
        .kernel
        .endpoints()
        .await?
        .into_iter()
        .map(|v| endpoint_item(&app, v))
        .collect();
    Ok(Json(EndpointList { items }))
}

pub(crate) async fn create_endpoint(
    State(app): State<Arc<App>>,
    body: Result<Json<EndpointBody>, JsonRejection>,
) -> Result<(StatusCode, Json<EndpointCreated>), ApiError> {
    let id = app
        .kernel
        .create_endpoint(endpoint_draft(json(body)?)?)
        .await?;
    Ok((StatusCode::CREATED, Json(EndpointCreated { id })))
}

pub(crate) async fn update_endpoint(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<EndpointBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let id = endpoint_id(id)?;
    app.kernel
        .update_endpoint(id, endpoint_draft(json(body)?)?)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_endpoint(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    app.kernel.delete_endpoint(endpoint_id(id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn test_endpoint(
    State(app): State<Arc<App>>,
    body: Result<Json<TestBody>, JsonRejection>,
) -> Result<Json<TestResult>, ApiError> {
    let b = json(body)?;
    let draft = EndpointDraft {
        name: String::new(),
        kind: b.kind,
        config_json: b.config.get().to_owned(),
        credential: credential(b.credential)?,
    };
    let models = app.kernel.test_endpoint(draft, b.endpoint_id).await?;
    Ok(Json(TestResult { models }))
}

pub(crate) async fn list_models(State(app): State<Arc<App>>) -> Result<Json<ModelList>, ApiError> {
    let items = app.kernel.models().await?.into_iter().map(item).collect();
    let default_model_id = app.kernel.default_model().await?;
    Ok(Json(ModelList {
        items,
        default_model_id,
    }))
}

pub(crate) async fn create_model(
    State(app): State<Arc<App>>,
    body: Result<Json<ModelBody>, JsonRejection>,
) -> Result<(StatusCode, Json<ModelCreated>), ApiError> {
    let id = app.kernel.create_model(draft(json(body)?)?).await?;
    Ok((StatusCode::CREATED, Json(ModelCreated { id })))
}

pub(crate) async fn update_model(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<ModelBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let id = model_id(id)?;
    app.kernel.update_model(id, draft(json(body)?)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_model(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    app.kernel.delete_model(model_id(id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn get_default(
    State(app): State<Arc<App>>,
) -> Result<Json<DefaultView>, ApiError> {
    Ok(Json(DefaultView {
        model_id: app.kernel.default_model().await?,
    }))
}

pub(crate) async fn put_default(
    State(app): State<Arc<App>>,
    body: Result<Json<ModelRef>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    app.kernel.set_default_model(json(body)?.model_id).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn set_session_model(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<ModelRef>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let session = api::session(&app, id).await?;
    if !api::writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    app.kernel
        .set_session_model(session.id, json(body)?.model_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn model_id(id: Result<Path<i64>, PathRejection>) -> Result<ModelId, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(MODEL_NOT_FOUND))?;
    Ok(ModelId(id))
}

fn endpoint_id(id: Result<Path<i64>, PathRejection>) -> Result<EndpointId, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(ENDPOINT_NOT_FOUND))?;
    Ok(EndpointId(id))
}

fn endpoint_item(app: &App, v: EndpointView) -> EndpointItem {
    // 库内 config_json 由工厂 check 规范化后写入，必为合法 JSON。
    let config = RawValue::from_string(v.config_json).expect("库内配置是合法 JSON");
    let key_env = app.kernel.key_env(&v.kind, config.get());
    EndpointItem {
        id: v.id,
        name: v.name,
        kind: v.kind,
        config,
        key_set: v.key_set,
        key_env,
    }
}

fn item(v: ModelView) -> ModelItem {
    let config = RawValue::from_string(v.config_json).expect("库内配置是合法 JSON");
    ModelItem {
        id: v.id,
        endpoint_id: v.endpoint_id,
        name: v.name,
        config,
    }
}

fn credential(b: CredentialBody) -> Result<CredentialWrite, ApiError> {
    Ok(match b {
        CredentialBody::Keep => CredentialWrite::Keep,
        CredentialBody::Clear => CredentialWrite::Clear,
        CredentialBody::Set { value } => {
            let value = value.trim().to_owned();
            if value.is_empty() || value.chars().count() > API_KEY_MAX_CHARS {
                return Err(ApiError::BadRequest(format!(
                    "API key 须为 1 到 {API_KEY_MAX_CHARS} 字"
                )));
            }
            CredentialWrite::Set(SecretValue::new(value))
        }
    })
}

fn endpoint_draft(b: EndpointBody) -> Result<EndpointDraft, ApiError> {
    let name = b.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > ENDPOINT_NAME_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "服务商名称须为 1 到 {ENDPOINT_NAME_MAX_CHARS} 字"
        )));
    }
    Ok(EndpointDraft {
        name,
        kind: b.kind,
        config_json: b.config.get().to_owned(),
        credential: credential(b.credential)?,
    })
}

fn draft(b: ModelBody) -> Result<ModelDraft, ApiError> {
    let name = b.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > MODEL_NAME_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "模型名须为 1 到 {MODEL_NAME_MAX_CHARS} 字"
        )));
    }
    Ok(ModelDraft {
        endpoint_id: b.endpoint_id,
        name,
        config_json: b.config.get().to_owned(),
    })
}
