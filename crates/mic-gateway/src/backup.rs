//! 设置备份 HTTP 编排；所有内容先解析，再提交 core 与恢复 Channel。

use std::collections::BTreeMap;
use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use mic_core::{ChannelRestore, ChannelSetup, SettingsBackup};
use mic_store::SettingsExport;
use serde::{Deserialize, Serialize};

use crate::api::json;
use crate::error::ApiError;
use crate::limits::{
    API_KEY_MAX_CHARS, ENDPOINT_NAME_MAX_CHARS, MAX_TURNS_LIMIT, MODEL_NAME_MAX_CHARS,
    PERSONA_NAME_MAX_CHARS, PROMPT_MAX_CHARS,
};
use crate::service::App;

#[derive(Serialize)]
pub(crate) struct ExportView {
    text: String,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct ImportBody {
    text: String,
}

pub(crate) async fn export(State(app): State<Arc<App>>) -> Result<Json<ExportView>, ApiError> {
    let core = app.kernel.export_settings().await?;
    let mut channels = BTreeMap::new();
    for (name, setup) in app.kernel.channel_setups() {
        if let Some(account) =
            setup
                .export()
                .await
                .map_err(|source| mic_core::ChannelSetupError::Internal {
                    source: Box::new(source),
                })?
        {
            channels.insert(name.to_owned(), account);
        }
    }
    Ok(Json(ExportView {
        text: app.kernel.seal_backup(&SettingsBackup { core, channels }),
    }))
}

pub(crate) async fn import(
    State(app): State<Arc<App>>,
    body: Result<Json<ImportBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let body = json(body)?;
    let backup = app.kernel.open_backup(&body.text)?;
    check_fields(&backup.core)?;
    let prepared = prepare_channels(app.kernel.channel_setups(), backup.channels)?;
    app.kernel.import_settings(backup.core).await?;
    restore_channels(prepared).await?;
    Ok(StatusCode::NO_CONTENT)
}

type PreparedChannels = Vec<(&'static str, Box<dyn ChannelRestore>)>;

fn prepare_channels(
    setups: BTreeMap<&'static str, Arc<dyn ChannelSetup>>,
    mut accounts: BTreeMap<String, serde_json::Value>,
) -> Result<PreparedChannels, ApiError> {
    for name in accounts.keys() {
        if !setups.contains_key(name.as_str()) {
            return Err(ApiError::BadRequest(format!(
                "备份包含未注册的 Channel「{name}」"
            )));
        }
    }
    setups
        .into_iter()
        .map(|(name, setup)| {
            setup
                .prepare_restore(accounts.remove(name))
                .map(|action| (name, action))
                .map_err(ApiError::from)
        })
        .collect()
}

async fn restore_channels(prepared: PreparedChannels) -> Result<(), ApiError> {
    let mut failures = Vec::new();
    for (name, action) in prepared {
        if let Err(error) = action.restore().await {
            if let mic_core::ChannelSetupError::Internal { source } = &error {
                tracing::error!(channel = name, error = %source, "settings channel restore failed");
            }
            let label = if name == "wechat" { "微信" } else { name };
            failures.push(format!("{label}账号恢复失败：{error}"));
        }
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(ApiError::ImportPartial(format!(
            "设置已导入；{}",
            failures.join("；")
        )))
    }
}

fn check_fields(s: &SettingsExport) -> Result<(), ApiError> {
    let invalid =
        || ApiError::BadRequest("备份中的名称、提示词、密钥或通用设置不符合设置限制".into());
    let name_valid = |name: &str, max: usize| {
        !name.trim().is_empty() && name == name.trim() && name.chars().count() <= max
    };
    for e in &s.endpoints {
        if !name_valid(&e.name, ENDPOINT_NAME_MAX_CHARS)
            || e.key
                .as_ref()
                .is_some_and(|key| key.trim().is_empty() || key.chars().count() > API_KEY_MAX_CHARS)
        {
            return Err(invalid());
        }
        if e.models
            .iter()
            .any(|m| !name_valid(&m.name, MODEL_NAME_MAX_CHARS))
        {
            return Err(invalid());
        }
    }
    if s.personas.iter().any(|p| {
        !name_valid(&p.name, PERSONA_NAME_MAX_CHARS) || p.prompt.chars().count() > PROMPT_MAX_CHARS
    }) {
        return Err(invalid());
    }
    let p = &s.preferences;
    if p.general_prompt.chars().count() > PROMPT_MAX_CHARS
        || !(1..=MAX_TURNS_LIMIT).contains(&p.max_turns)
    {
        return Err(invalid());
    }
    Ok(())
}

#[cfg(test)]
mod tests;
