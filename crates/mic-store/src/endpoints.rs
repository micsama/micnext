//! 服务商：连接配置与加密 key。契约：docs/blueprints/model-settings.md §十。

use rusqlite::{params, Connection, OptionalExtension, Transaction};

use crate::models::read_default;
use crate::secrets::Cipher;
use crate::{
    CredentialWrite, EndpointId, EndpointView, EndpointWrite, ModelSettingsError, SecretValue,
    Store, StoreError,
};

const COLS: &str = "id, name, kind, config_json, key_cipher IS NOT NULL";

fn view(row: &rusqlite::Row<'_>) -> rusqlite::Result<EndpointView> {
    Ok(EndpointView {
        id: EndpointId(row.get(0)?),
        name: row.get(1)?,
        kind: row.get(2)?,
        config_json: row.get(3)?,
        key_set: row.get(4)?,
    })
}

impl Store {
    /// 未删除的服务商，按 id。
    pub async fn endpoints(&self) -> Result<Vec<EndpointView>, StoreError> {
        self.call(|conn| {
            let mut stmt = conn.prepare(&format!(
                "SELECT {COLS} FROM core_endpoints WHERE deleted_at IS NULL ORDER BY id"
            ))?;
            let rows = stmt.query_map([], view)?;
            Ok(rows.collect::<rusqlite::Result<_>>()?)
        })
        .await
    }

    /// 仅未删除的；id 可能来自外部，不存在返回 `None`。
    pub async fn endpoint(&self, id: EndpointId) -> Result<Option<EndpointView>, StoreError> {
        self.call(move |conn| Ok(find_live(conn, id)?)).await
    }

    /// 已存的 key（测试连接用）；无 key 返回 `None`。
    pub async fn endpoint_key(
        &self,
        id: EndpointId,
    ) -> Result<Option<SecretValue>, ModelSettingsError> {
        let cipher = self.cipher();
        self.blocking(move |conn| {
            live(conn, id)?;
            let blob: Option<Vec<u8>> = conn.query_row(
                "SELECT key_cipher FROM core_endpoints WHERE id = ?1",
                [id.0],
                |r| r.get(0),
            )?;
            Ok(blob.map(|b| cipher.decrypt(id.0, &b)).transpose()?)
        })
        .await
    }

    pub async fn create_endpoint(
        &self,
        e: EndpointWrite,
        now: i64,
    ) -> Result<EndpointId, ModelSettingsError> {
        let cipher = self.cipher();
        self.blocking(move |conn| {
            let tx = conn.transaction()?;
            ensure_name_free(&tx, &e.name, None)?;
            tx.execute(
                "INSERT INTO core_endpoints (name, kind, config_json, created_at, updated_at)
                 VALUES (?1, ?2, ?3, ?4, ?4)",
                params![e.name, e.kind, e.config_json, now],
            )?;
            let id = EndpointId(tx.last_insert_rowid());
            match e.credential {
                CredentialWrite::Keep => return Err(ModelSettingsError::InvalidSecretEdit),
                CredentialWrite::Clear => {}
                CredentialWrite::Set(secret) => write_key(&tx, &cipher, id, Some(&secret))?,
            }
            tx.commit()?;
            Ok(id)
        })
        .await
    }

    pub async fn update_endpoint(
        &self,
        id: EndpointId,
        e: EndpointWrite,
        now: i64,
    ) -> Result<(), ModelSettingsError> {
        let cipher = self.cipher();
        self.blocking(move |conn| {
            let tx = conn.transaction()?;
            live(&tx, id)?;
            ensure_name_free(&tx, &e.name, Some(id))?;
            tx.execute(
                "UPDATE core_endpoints SET name = ?1, kind = ?2, config_json = ?3, updated_at = ?4
                 WHERE id = ?5",
                params![e.name, e.kind, e.config_json, now, id.0],
            )?;
            match e.credential {
                CredentialWrite::Keep => {}
                CredentialWrite::Clear => write_key(&tx, &cipher, id, None)?,
                CredentialWrite::Set(secret) => write_key(&tx, &cipher, id, Some(&secret))?,
            }
            tx.commit()?;
            Ok(())
        })
        .await
    }

    /// 软删除并清掉 key，同事务软删其全部模型。默认模型在其中时：别处还有模型须先换默认，
    /// 否则默认回到空。
    pub async fn delete_endpoint(
        &self,
        id: EndpointId,
        now: i64,
    ) -> Result<(), ModelSettingsError> {
        self.blocking(move |conn| {
            let tx = conn.transaction()?;
            live(&tx, id)?;
            let default_inside: bool = tx.query_row(
                "SELECT EXISTS(SELECT 1 FROM core_models
                               WHERE endpoint_id = ?1 AND deleted_at IS NULL AND id IS ?2)",
                params![id.0, read_default(&tx)?.map(|m| m.0)],
                |r| r.get(0),
            )?;
            if default_inside {
                let others: bool = tx.query_row(
                    "SELECT EXISTS(SELECT 1 FROM core_models
                                   WHERE endpoint_id != ?1 AND deleted_at IS NULL)",
                    [id.0],
                    |r| r.get(0),
                )?;
                if others {
                    return Err(ModelSettingsError::DefaultInUse);
                }
                tx.execute("UPDATE core_settings SET default_model_id = NULL", [])?;
            }
            tx.execute(
                "UPDATE core_models SET deleted_at = ?1 WHERE endpoint_id = ?2 AND deleted_at IS NULL",
                params![now, id.0],
            )?;
            tx.execute(
                "UPDATE core_endpoints SET deleted_at = ?1, key_cipher = NULL WHERE id = ?2",
                params![now, id.0],
            )?;
            tx.commit()?;
            Ok(())
        })
        .await
    }
}

fn find_live(conn: &Connection, id: EndpointId) -> rusqlite::Result<Option<EndpointView>> {
    conn.query_row(
        &format!("SELECT {COLS} FROM core_endpoints WHERE id = ?1 AND deleted_at IS NULL"),
        [id.0],
        view,
    )
    .optional()
}

/// 服务商存在且未删除。
pub(crate) fn live(conn: &Connection, id: EndpointId) -> Result<(), ModelSettingsError> {
    let deleted: Option<bool> = conn
        .query_row(
            "SELECT deleted_at IS NOT NULL FROM core_endpoints WHERE id = ?1",
            [id.0],
            |r| r.get(0),
        )
        .optional()?;
    match deleted {
        None => Err(ModelSettingsError::EndpointNotFound),
        Some(true) => Err(ModelSettingsError::EndpointDeleted),
        Some(false) => Ok(()),
    }
}

fn ensure_name_free(
    conn: &Connection,
    name: &str,
    except: Option<EndpointId>,
) -> Result<(), ModelSettingsError> {
    let taken: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM core_endpoints
                       WHERE name = ?1 AND deleted_at IS NULL AND id IS NOT ?2)",
        params![name, except.map(|id| id.0)],
        |r| r.get(0),
    )?;
    if taken {
        return Err(ModelSettingsError::NameTaken(name.to_owned()));
    }
    Ok(())
}

fn write_key(
    tx: &Transaction<'_>,
    cipher: &Cipher,
    id: EndpointId,
    secret: Option<&SecretValue>,
) -> rusqlite::Result<()> {
    let blob = secret.map(|s| cipher.encrypt(id.0, s));
    tx.execute(
        "UPDATE core_endpoints SET key_cipher = ?1 WHERE id = ?2",
        params![blob, id.0],
    )?;
    Ok(())
}

/// 启动自检：全部现有密文可用当前主密钥解开。
pub(crate) fn verify_keys(conn: &Connection, cipher: &Cipher) -> Result<(), StoreError> {
    let mut stmt =
        conn.prepare("SELECT id, key_cipher FROM core_endpoints WHERE key_cipher IS NOT NULL")?;
    let rows = stmt.query_map([], |r| Ok((r.get::<_, i64>(0)?, r.get::<_, Vec<u8>>(1)?)))?;
    for row in rows {
        let (id, blob) = row?;
        cipher.decrypt(id, &blob)?;
    }
    Ok(())
}

pub(crate) fn has_ciphertext(conn: &Connection) -> rusqlite::Result<bool> {
    conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM core_endpoints WHERE key_cipher IS NOT NULL)",
        [],
        |r| r.get(0),
    )
}
