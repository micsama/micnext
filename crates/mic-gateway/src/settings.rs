//! 设置页与人设接口。契约：docs/blueprints/runtime-settings.md §六。

use std::path::PathBuf;
use std::sync::Arc;

use axum::extract::rejection::{JsonRejection, PathRejection};
use axum::extract::{Path, State};
use axum::http::StatusCode;
use axum::Json;
use mic_store::{PersonaId, Settings};
use serde::{Deserialize, Serialize};

use crate::api::{self, json};
use crate::error::{ApiError, PERSONA_NOT_FOUND};
use crate::limits::{MAX_TURNS_LIMIT, PERSONA_NAME_MAX_CHARS, PROMPT_MAX_CHARS};
use crate::service::App;

#[derive(Serialize)]
pub(crate) struct SettingsView {
    default_persona_id: PersonaId,
    general_prompt: String,
    default_workdir: String,
    max_turns: u32,
    /// 系统块，只读。
    system_prompt: &'static str,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SettingsBody {
    default_persona_id: PersonaId,
    general_prompt: String,
    default_workdir: String,
    max_turns: u32,
}

#[derive(Serialize)]
pub(crate) struct PersonaItem {
    id: PersonaId,
    name: String,
    prompt: String,
    builtin: bool,
}

#[derive(Serialize)]
pub(crate) struct PersonaList {
    items: Vec<PersonaItem>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PersonaBody {
    name: String,
    prompt: String,
}

#[derive(Serialize)]
pub(crate) struct PersonaCreated {
    id: PersonaId,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SessionPersonaBody {
    persona_id: PersonaId,
}

pub(crate) async fn get_settings(
    State(app): State<Arc<App>>,
) -> Result<Json<SettingsView>, ApiError> {
    let s = app.kernel.settings().await?;
    Ok(Json(SettingsView {
        default_persona_id: s.default_persona,
        general_prompt: s.general_prompt,
        default_workdir: s.default_workdir,
        max_turns: s.max_turns,
        system_prompt: app.kernel.system_prompt(),
    }))
}

pub(crate) async fn put_settings(
    State(app): State<Arc<App>>,
    body: Result<Json<SettingsBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let b = json(body)?;
    if b.general_prompt.chars().count() > PROMPT_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "通用偏好最多 {PROMPT_MAX_CHARS} 字"
        )));
    }
    let default_workdir = b.default_workdir.trim().to_owned();
    if !(default_workdir.starts_with('/') || default_workdir.starts_with("~/")) {
        return Err(ApiError::BadRequest(
            "新会话默认工作目录须是绝对路径（以 / 开头）或以 ~/ 开头".into(),
        ));
    }
    if !(1..=MAX_TURNS_LIMIT).contains(&b.max_turns) {
        return Err(ApiError::BadRequest(format!(
            "单轮调用上限须在 1 到 {MAX_TURNS_LIMIT} 之间"
        )));
    }
    app.kernel
        .update_settings(Settings {
            default_persona: b.default_persona_id,
            general_prompt: b.general_prompt,
            default_workdir,
            max_turns: b.max_turns,
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn list_personas(
    State(app): State<Arc<App>>,
) -> Result<Json<PersonaList>, ApiError> {
    let items = app
        .kernel
        .personas()
        .await?
        .into_iter()
        .map(|p| PersonaItem {
            id: p.id,
            name: p.name,
            prompt: p.prompt,
            builtin: p.builtin,
        })
        .collect();
    Ok(Json(PersonaList { items }))
}

pub(crate) async fn create_persona(
    State(app): State<Arc<App>>,
    body: Result<Json<PersonaBody>, JsonRejection>,
) -> Result<(StatusCode, Json<PersonaCreated>), ApiError> {
    let (name, prompt) = persona_fields(json(body)?)?;
    let id = app.kernel.create_persona(name, prompt).await?;
    Ok((StatusCode::CREATED, Json(PersonaCreated { id })))
}

pub(crate) async fn update_persona(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<PersonaBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let id = persona_id(id)?;
    let (name, prompt) = persona_fields(json(body)?)?;
    app.kernel.update_persona(id, name, prompt).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn delete_persona(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
) -> Result<StatusCode, ApiError> {
    app.kernel.delete_persona(persona_id(id)?).await?;
    Ok(StatusCode::NO_CONTENT)
}

pub(crate) async fn set_session_persona(
    State(app): State<Arc<App>>,
    id: Result<Path<i64>, PathRejection>,
    body: Result<Json<SessionPersonaBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let session = api::session(&app, id).await?;
    if !api::writable(&session) {
        return Err(ApiError::ReadOnly);
    }
    let b = json(body)?;
    app.kernel
        .set_session_persona(session.id, b.persona_id)
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

/// 设置里的默认工作目录展开为 UTF-8 绝对路径，并确保存在。
pub(crate) async fn new_session_workdir(app: &App) -> Result<String, ApiError> {
    let raw = app.kernel.settings().await?.default_workdir;
    let path = match raw.strip_prefix("~/") {
        Some(rest) => std::env::var_os("HOME")
            .filter(|h| !h.is_empty())
            .map(|h| PathBuf::from(h).join(rest))
            .ok_or_else(|| {
                ApiError::Workdir(format!(
                    "环境变量 HOME 未设置，无法展开「{raw}」：请在 设置 → 对话偏好 改用绝对路径"
                ))
            })?,
        None => PathBuf::from(&raw),
    };
    let path = path.into_os_string().into_string().map_err(|p| {
        ApiError::Workdir(format!(
            "工作目录 {} 不是 UTF-8 路径，请在 设置 → 对话偏好 修改",
            p.to_string_lossy()
        ))
    })?;
    tokio::fs::create_dir_all(&path).await.map_err(|e| {
        ApiError::Workdir(format!(
            "无法创建工作目录 {path}：{e}。请在 设置 → 对话偏好 改为可写的目录"
        ))
    })?;
    Ok(path)
}

fn persona_id(id: Result<Path<i64>, PathRejection>) -> Result<PersonaId, ApiError> {
    let Path(id) = id.map_err(|_| ApiError::NotFound(PERSONA_NOT_FOUND))?;
    Ok(PersonaId(id))
}

fn persona_fields(b: PersonaBody) -> Result<(String, String), ApiError> {
    let name = b.name.trim().to_owned();
    if name.is_empty() || name.chars().count() > PERSONA_NAME_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "人设名字须为 1 到 {PERSONA_NAME_MAX_CHARS} 字"
        )));
    }
    if b.prompt.trim().is_empty() {
        return Err(ApiError::BadRequest("人设提示词不能为空".into()));
    }
    if b.prompt.chars().count() > PROMPT_MAX_CHARS {
        return Err(ApiError::BadRequest(format!(
            "人设提示词最多 {PROMPT_MAX_CHARS} 字"
        )));
    }
    Ok((name, b.prompt))
}
