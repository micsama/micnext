//! 设置备份的 core 数据与事务覆盖。契约：docs/blueprints/settings-backup.md。

use std::collections::{BTreeMap, BTreeSet};
use std::num::NonZeroU32;

use rusqlite::{params, Connection};
use serde::{Deserialize, Serialize};

use crate::{
    CompactionSettings, ModelPrice, ReasoningEffort, SecretValue, SettingsError, Store, StoreError,
};

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SettingsExport {
    pub endpoints: Vec<ExportEndpoint>,
    pub personas: Vec<ExportPersona>,
    pub preferences: ExportPreferences,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportEndpoint {
    pub name: String,
    pub kind: String,
    pub config_json: String,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub key: Option<String>,
    pub models: Vec<ExportModel>,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportModel {
    pub name: String,
    pub config_json: String,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub context_window: Option<NonZeroU32>,
    pub images: bool,
    pub price: ModelPrice,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPersona {
    pub name: String,
    pub prompt: String,
}

#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportModelRef {
    pub endpoint: String,
    pub model: String,
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ExportPreferences {
    pub default_persona: String,
    pub general_prompt: String,
    pub default_workdir: String,
    pub max_turns: u32,
    pub compaction: CompactionSettings,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub default_model: Option<ExportModelRef>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub title_model: Option<ExportModelRef>,
    #[serde(deserialize_with = "Deserialize::deserialize")]
    pub title_reasoning_effort: Option<ReasoningEffort>,
}

impl Store {
    pub fn seal_export(&self, plain: &SecretValue) -> String {
        self.cipher().seal_export(plain)
    }

    pub fn open_export(&self, text: &str) -> Result<SecretValue, StoreError> {
        self.cipher().open_export(text)
    }

    pub async fn export_settings(&self) -> Result<SettingsExport, StoreError> {
        let cipher = self.cipher();
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let mut endpoints = Vec::new();
            let mut stmt = tx.prepare("SELECT id, name, kind, config_json, key_cipher FROM core_endpoints WHERE deleted_at IS NULL ORDER BY id")?;
            let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, String>(1)?, r.get::<_, String>(2)?, r.get::<_, String>(3)?, r.get::<_, Option<Vec<u8>>>(4)?)))?;
            for row in rows {
                let (id, name, kind, config_json, key) = row?;
                let key = key.map(|blob| cipher.decrypt(id, &blob).map(|s| s.expose().to_owned())).transpose()?;
                let models = tx.prepare("SELECT name, config_json, context_window, images, price_input, price_cache_read, price_cache_write, price_output FROM core_models WHERE endpoint_id = ?1 AND deleted_at IS NULL ORDER BY id")?
                    .query_map([id], |r| Ok(ExportModel {
                        name: r.get(0)?, config_json: r.get(1)?, context_window: r.get(2)?, images: r.get(3)?,
                        price: ModelPrice { input: r.get(4)?, cache_read: r.get(5)?, cache_write: r.get(6)?, output: r.get(7)? },
                    }))?.collect::<rusqlite::Result<Vec<_>>>()?;
                endpoints.push(ExportEndpoint { name, kind, config_json, key, models });
            }
            drop(stmt);
            let personas = tx.prepare("SELECT name, prompt FROM core_personas WHERE builtin = 0 AND deleted_at IS NULL ORDER BY id")?
                .query_map([], |r| Ok(ExportPersona { name: r.get(0)?, prompt: r.get(1)? }))?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let (preferences, default_model, title_model) = tx.query_row(
                "SELECT p.name, s.general_prompt, s.default_workdir, s.max_turns, s.compaction_trigger_percent, s.compaction_keep_percent, s.default_model_id, s.title_model_id, s.title_reasoning_effort FROM core_settings s JOIN core_personas p ON p.id = s.default_persona_id",
                [], |r| Ok((ExportPreferences {
                    default_persona: r.get(0)?, general_prompt: r.get(1)?, default_workdir: r.get(2)?, max_turns: r.get(3)?,
                    compaction: CompactionSettings { trigger_percent: r.get(4)?, keep_percent: r.get(5)? },
                    default_model: None, title_model: None, title_reasoning_effort: r.get(8)?,
                }, r.get::<_, Option<i64>>(6)?, r.get::<_, Option<i64>>(7)?)))?;
            let mut preferences = preferences;
            preferences.default_model = default_model.map(|id| model_reference(&tx, id)).transpose()?;
            preferences.title_model = title_model.map(|id| model_reference(&tx, id)).transpose()?;
            tx.commit()?;
            Ok(SettingsExport { endpoints, personas, preferences })
        }).await
    }

    pub async fn import_settings(&self, s: SettingsExport, now: i64) -> Result<(), SettingsError> {
        let cipher = self.cipher();
        self.call_settings(move |conn| {
            let tx = conn.transaction()?;
            let mut endpoints = names(&tx, "SELECT name, id FROM core_endpoints WHERE deleted_at IS NULL")?;
            let mut personas = names(&tx, "SELECT name, id FROM core_personas WHERE builtin = 0 AND deleted_at IS NULL")?;
            let builtin = names(&tx, "SELECT name, id FROM core_personas WHERE builtin = 1 AND deleted_at IS NULL")?;
            let mut old_models = BTreeMap::new();
            for row in tx.prepare("SELECT endpoint_id, name, id FROM core_models WHERE deleted_at IS NULL")?
                .query_map([], |r| Ok(((r.get::<_, i64>(0)?, r.get::<_, String>(1)?), r.get::<_, i64>(2)?)))? {
                let (key, id) = row?;
                old_models.insert(key, id);
            }
            tx.execute("UPDATE core_models SET deleted_at = ?1 WHERE deleted_at IS NULL", [now])?;
            tx.execute("UPDATE core_endpoints SET deleted_at = ?1 WHERE deleted_at IS NULL", [now])?;
            tx.execute("UPDATE core_personas SET deleted_at = ?1 WHERE builtin = 0 AND deleted_at IS NULL", [now])?;
            let mut endpoint_names = BTreeSet::new();
            let mut model_ids = BTreeMap::new();
            for e in s.endpoints {
                if e.name.trim().is_empty() || !endpoint_names.insert(e.name.clone()) {
                    return Err(invalid("服务商名称为空或重复"));
                }
                let id = match endpoints.remove(&e.name) {
                    Some(id) => {
                        tx.execute("UPDATE core_endpoints SET kind = ?1, config_json = ?2, deleted_at = NULL, updated_at = ?3 WHERE id = ?4", params![e.kind, e.config_json, now, id])?;
                        id
                    }
                    None => {
                        tx.execute("INSERT INTO core_endpoints(name, kind, config_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)", params![e.name, e.kind, e.config_json, now])?;
                        tx.last_insert_rowid()
                    }
                };
                let key = e.key.map(|s| cipher.encrypt(id, &SecretValue::new(s)));
                tx.execute("UPDATE core_endpoints SET key_cipher = ?1 WHERE id = ?2", params![key, id])?;
                for m in e.models {
                    let reference = ExportModelRef { endpoint: e.name.clone(), model: m.name.clone() };
                    if m.name.trim().is_empty() || model_ids.contains_key(&reference) || !m.price.is_valid() {
                        return Err(invalid("模型名称为空、重复或单价无效"));
                    }
                    let model = match old_models.remove(&(id, m.name.clone())) {
                        Some(model) => model,
                        None => {
                            tx.execute("INSERT INTO core_models(endpoint_id, name, config_json, created_at, updated_at) VALUES (?1, ?2, ?3, ?4, ?4)", params![id, m.name, m.config_json, now])?;
                            tx.last_insert_rowid()
                        }
                    };
                    tx.execute("UPDATE core_models SET config_json = ?1, context_window = ?2, images = ?3, price_input = ?4, price_cache_read = ?5, price_cache_write = ?6, price_output = ?7, deleted_at = NULL, updated_at = ?8 WHERE id = ?9", params![m.config_json, m.context_window, m.images, m.price.input, m.price.cache_read, m.price.cache_write, m.price.output, now, model])?;
                    model_ids.insert(reference, model);
                }
            }
            tx.execute("UPDATE core_endpoints SET key_cipher = NULL WHERE deleted_at IS NOT NULL", [])?;
            let mut persona_ids = builtin;
            for p in s.personas {
                if p.name.trim().is_empty() || persona_ids.contains_key(&p.name) || p.prompt.trim().is_empty() {
                    return Err(invalid("人设名称为空、重复、与内置人设同名或提示词为空"));
                }
                let id = match personas.remove(&p.name) {
                    Some(id) => {
                        tx.execute("UPDATE core_personas SET prompt = ?1, deleted_at = NULL, updated_at = ?2 WHERE id = ?3", params![p.prompt, now, id])?;
                        id
                    }
                    None => {
                        tx.execute("INSERT INTO core_personas(name, prompt, builtin, created_at, updated_at) VALUES (?1, ?2, 0, ?3, ?3)", params![p.name, p.prompt, now])?;
                        tx.last_insert_rowid()
                    }
                };
                persona_ids.insert(p.name, id);
            }
            let p = s.preferences;
            let persona = persona_ids.get(&p.default_persona).ok_or_else(|| invalid("默认人设不存在"))?;
            let resolve = |r: Option<ExportModelRef>| -> Result<Option<i64>, SettingsError> {
                r.map(|r| model_ids.get(&r).copied().ok_or_else(|| invalid("引用的模型不存在"))).transpose()
            };
            let default_model = resolve(p.default_model)?;
            let title_model = resolve(p.title_model)?;
            if !(1..=80).contains(&p.compaction.trigger_percent) || p.compaction.keep_percent > 50 || p.max_turns == 0
                || !(p.default_workdir.starts_with('/') || p.default_workdir.starts_with("~/"))
                || (title_model.is_none() && p.title_reasoning_effort.is_some()) {
                return Err(invalid("通用设置或标题模型选择无效"));
            }
            tx.execute("UPDATE core_settings SET default_persona_id = ?1, general_prompt = ?2, default_workdir = ?3, max_turns = ?4, compaction_trigger_percent = ?5, compaction_keep_percent = ?6, default_model_id = ?7, title_model_id = ?8, title_reasoning_effort = ?9", params![persona, p.general_prompt, p.default_workdir, p.max_turns, p.compaction.trigger_percent, p.compaction.keep_percent, default_model, title_model, p.title_reasoning_effort])?;
            tx.commit()?;
            Ok(())
        }).await
    }
}

fn invalid(reason: &str) -> SettingsError {
    SettingsError::ImportInvalid(reason.into())
}

fn names(conn: &Connection, sql: &str) -> rusqlite::Result<BTreeMap<String, i64>> {
    conn.prepare(sql)?
        .query_map([], |r| Ok((r.get(0)?, r.get(1)?)))?
        .collect()
}

fn model_reference(conn: &Connection, id: i64) -> rusqlite::Result<ExportModelRef> {
    conn.query_row("SELECT e.name, m.name FROM core_models m JOIN core_endpoints e ON e.id = m.endpoint_id WHERE m.id = ?1 AND m.deleted_at IS NULL AND e.deleted_at IS NULL", [id], |r| Ok(ExportModelRef { endpoint: r.get(0)?, model: r.get(1)? }))
}

#[cfg(test)]
mod tests;
