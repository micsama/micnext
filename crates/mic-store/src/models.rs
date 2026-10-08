//! 服务商下的模型、默认与会话选择。契约：docs/blueprints/model-settings.md §十。

use mic_message::SessionId;
use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::endpoints;
use crate::secrets::Cipher;
use crate::{
    ClaimedModel, EndpointId, ModelId, ModelSettingsError, ModelView, ModelWrite, Store, StoreError,
};

const MODEL_COLS: &str = "id, endpoint_id, name, config_json";

fn view(row: &rusqlite::Row<'_>) -> rusqlite::Result<ModelView> {
    Ok(ModelView {
        id: ModelId(row.get(0)?),
        endpoint_id: EndpointId(row.get(1)?),
        name: row.get(2)?,
        config_json: row.get(3)?,
    })
}

impl Store {
    /// 未删除的模型，按 id。
    pub async fn models(&self) -> Result<Vec<ModelView>, StoreError> {
        self.call(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {MODEL_COLS} FROM core_models WHERE deleted_at IS NULL ORDER BY id"
            ))?;
            let rows = stmt.query_map([], view)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    /// 仅未删除的条目；id 可能来自外部，不存在返回 `None`。
    pub async fn model(&self, id: ModelId) -> Result<Option<ModelView>, StoreError> {
        self.call(move |conn| Ok(find_live(conn, id)?)).await
    }

    pub async fn default_model(&self) -> Result<Option<ModelId>, StoreError> {
        self.call(|conn| Ok(read_default(conn)?)).await
    }

    /// 首个模型创建时同事务设为默认。
    pub async fn create_model(
        &self,
        m: ModelWrite,
        now: i64,
    ) -> Result<ModelId, ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            endpoints::live(&tx, m.endpoint_id)?;
            ensure_name_free(&tx, m.endpoint_id, &m.name, None)?;
            tx.execute(
                "INSERT INTO core_models (endpoint_id, name, config_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![m.endpoint_id.0, m.name, m.config_json, now],
            )?;
            let id = ModelId(tx.last_insert_rowid());
            if read_default(&tx)?.is_none() {
                tx.execute("UPDATE core_settings SET default_model_id = ?1", [id.0])?;
            }
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    /// 只改模型名与参数，不换服务商。
    pub async fn update_model(
        &self,
        id: ModelId,
        m: ModelWrite,
        now: i64,
    ) -> Result<(), ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            let current = live(&tx, id)?;
            if current.endpoint_id != m.endpoint_id {
                return Err(ModelSettingsError::NotFound);
            }
            ensure_name_free(&tx, m.endpoint_id, &m.name, Some(id))?;
            tx.execute(
                "UPDATE core_models SET name = ?1, config_json = ?2, updated_at = ?3
                 WHERE id = ?4",
                params![m.name, m.config_json, now, id.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 软删除；会话与 run 快照仍引用原行。默认须先换，最后一条删除后默认回到空。
    pub async fn delete_model(&self, id: ModelId, now: i64) -> Result<(), ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            live(&tx, id)?;
            if read_default(&tx)? == Some(id) {
                let others: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM core_models WHERE deleted_at IS NULL AND id != ?1)",
                    [id.0],
                    |r| r.get(0),
                )?;
                if others {
                    return Err(ModelSettingsError::DefaultInUse);
                }
                tx.execute("UPDATE core_settings SET default_model_id = NULL", [])?;
            }
            tx.execute(
                "UPDATE core_models SET deleted_at = ?1 WHERE id = ?2",
                params![now, id.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    pub async fn set_default_model(&self, id: ModelId) -> Result<(), ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            live(&tx, id)?;
            tx.execute("UPDATE core_settings SET default_model_id = ?1", [id.0])?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 会话存在由调用方保证。执行中拒绝。
    pub async fn set_session_model(
        &self,
        session: SessionId,
        id: ModelId,
    ) -> Result<(), ModelSettingsError> {
        self.call_models(move |conn| {
            let tx = conn.transaction()?;
            live(&tx, id)?;
            let executing: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM core_runs
                               WHERE session_id = ?1 AND state = 'executing')",
                [session.0],
                |r| r.get(0),
            )?;
            if executing {
                return Err(ModelSettingsError::SessionExecuting);
            }
            tx.execute(
                "UPDATE core_sessions SET model_id = ?1 WHERE id = ?2",
                params![id.0, session.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }

    async fn call_models<R, F>(&self, f: F) -> Result<R, ModelSettingsError>
    where
        R: Send + 'static,
        F: FnOnce(&mut Connection) -> Result<R, ModelSettingsError> + Send + 'static,
    {
        self.blocking(f).await
    }
}

pub(crate) fn read_default(conn: &Connection) -> rusqlite::Result<Option<ModelId>> {
    Ok(conn
        .query_row("SELECT default_model_id FROM core_settings", [], |r| {
            r.get::<_, Option<i64>>(0)
        })?
        .map(ModelId))
}

fn find_live(conn: &Connection, id: ModelId) -> rusqlite::Result<Option<ModelView>> {
    conn.query_row(
        &format!("SELECT {MODEL_COLS} FROM core_models WHERE id = ?1 AND deleted_at IS NULL"),
        [id.0],
        view,
    )
    .optional()
}

fn live(conn: &Connection, id: ModelId) -> Result<ModelView, ModelSettingsError> {
    let deleted: Option<bool> = conn
        .query_row(
            "SELECT deleted_at IS NOT NULL FROM core_models WHERE id = ?1",
            [id.0],
            |r| r.get(0),
        )
        .optional()?;
    match deleted {
        None => Err(ModelSettingsError::NotFound),
        Some(true) => Err(ModelSettingsError::Deleted),
        Some(false) => Ok(find_live(conn, id)?.expect("刚确认过未删除")),
    }
}

fn ensure_name_free(
    conn: &Connection,
    endpoint: EndpointId,
    name: &str,
    except: Option<ModelId>,
) -> Result<(), ModelSettingsError> {
    let taken: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM core_models
                       WHERE endpoint_id = ?1 AND name = ?2 AND deleted_at IS NULL
                         AND id IS NOT ?3)",
        params![endpoint.0, name, except.map(|id| id.0)],
        |r| r.get(0),
    )?;
    if taken {
        return Err(ModelSettingsError::NameTaken(name.to_owned()));
    }
    Ok(())
}

/// 认领时读定会话所选模型：会话未选则取当时默认并写回；已删除不换成别的模型。
pub(crate) fn claim_model(
    tx: &Transaction<'_>,
    cipher: &Cipher,
    session: SessionId,
) -> Result<ClaimedModel, StoreError> {
    let chosen: Option<i64> = tx.query_row(
        "SELECT model_id FROM core_sessions WHERE id = ?1",
        [session.0],
        |r| r.get(0),
    )?;
    let id = match chosen {
        Some(id) => id,
        None => {
            let Some(default) = read_default(tx)? else {
                return Ok(ClaimedModel::Missing);
            };
            tx.execute(
                "UPDATE core_sessions SET model_id = ?1 WHERE id = ?2",
                params![default.0, session.0],
            )?;
            default.0
        }
    };
    let (endpoint_name, kind, endpoint_json, blob, endpoint_id, model_name, model_json, deleted): (
        String,
        String,
        String,
        Option<Vec<u8>>,
        i64,
        String,
        String,
        bool,
    ) = tx.query_row(
        "SELECT e.name, e.kind, e.config_json, e.key_cipher, e.id, m.name, m.config_json,
                m.deleted_at IS NOT NULL
         FROM core_models m JOIN core_endpoints e ON e.id = m.endpoint_id WHERE m.id = ?1",
        [id],
        |r| {
            Ok((
                r.get(0)?,
                r.get(1)?,
                r.get(2)?,
                r.get(3)?,
                r.get(4)?,
                r.get(5)?,
                r.get(6)?,
                r.get(7)?,
            ))
        },
    )?;
    let id = ModelId(id);
    let name = format!("{endpoint_name} / {model_name}");
    if deleted {
        return Ok(ClaimedModel::Deleted { id, name });
    }
    let key = blob.map(|b| cipher.decrypt(endpoint_id, &b)).transpose()?;
    Ok(ClaimedModel::Selected {
        id,
        name,
        kind,
        endpoint_json,
        model_name,
        model_json,
        key,
    })
}
