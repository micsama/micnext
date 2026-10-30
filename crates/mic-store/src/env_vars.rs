//! 命令环境变量：值加密存储，注入 `bash` 并一律精确脱敏。契约：docs/blueprints/env-vars.md。

use rusqlite::{params, Connection};

use crate::limits::ENV_VALUE_MIN_CHARS;
use crate::secrets::Cipher;
use crate::{SecretValue, SettingsError, Store, StoreError};

impl Store {
    /// 按名字排序的全部变量与明文值。
    pub async fn env_vars(&self) -> Result<Vec<(String, SecretValue)>, StoreError> {
        let cipher = self.cipher();
        self.call(move |conn| load(conn, &cipher)).await
    }

    /// 新建或覆盖。
    pub async fn set_env_var(&self, name: String, value: SecretValue) -> Result<(), SettingsError> {
        validate(&name, &value)?;
        let blob = self.cipher().encrypt_env(&name, &value);
        self.call_settings(move |conn| {
            conn.execute(
                "INSERT INTO core_env_vars(name, value_cipher) VALUES (?1, ?2)
                 ON CONFLICT(name) DO UPDATE SET value_cipher = excluded.value_cipher",
                params![name, blob],
            )?;
            Ok(())
        })
        .await
    }

    pub async fn delete_env_var(&self, name: String) -> Result<(), StoreError> {
        self.call(move |conn| {
            conn.execute("DELETE FROM core_env_vars WHERE name = ?1", [name])?;
            Ok(())
        })
        .await
    }
}

pub(crate) fn load(
    conn: &Connection,
    cipher: &Cipher,
) -> Result<Vec<(String, SecretValue)>, StoreError> {
    let mut stmt = conn.prepare("SELECT name, value_cipher FROM core_env_vars ORDER BY name")?;
    let rows = stmt.query_map([], |r| {
        Ok((r.get::<_, String>(0)?, r.get::<_, Vec<u8>>(1)?))
    })?;
    rows.map(|row| {
        let (name, blob) = row?;
        let value = cipher.decrypt_env(&name, &blob)?;
        Ok((name, value))
    })
    .collect()
}

/// 名字是 shell 变量名，不改变 `bash -c` 的启动行为，也不覆盖进程已有变量；值能作为环境变量传递，
/// 且够长，免得精确脱敏误遮正文。设置与备份导入共用。
pub(crate) fn validate(name: &str, value: &SecretValue) -> Result<(), SettingsError> {
    let invalid = |m: String| Err(SettingsError::EnvVarInvalid(m));
    let mut chars = name.chars();
    let shell_name = chars
        .next()
        .is_some_and(|c| c.is_ascii_alphabetic() || c == '_')
        && chars.all(|c| c.is_ascii_alphanumeric() || c == '_');
    if !shell_name {
        return invalid(format!(
            "变量名「{name}」不合法：只能用字母、数字和下划线，且不以数字开头"
        ));
    }
    if name.starts_with("BASH") || ["ENV", "SHELLOPTS", "IFS"].contains(&name) {
        return invalid(format!("{name} 会改变 bash 的启动行为，不能在这里设置"));
    }
    if std::env::var_os(name).is_some() {
        return invalid(format!(
            "{name} 是服务进程已有的环境变量，换个名字以免覆盖它"
        ));
    }
    if value.expose().contains('\0') {
        return invalid(format!(
            "{name} 的值含有空字符（NUL），无法作为环境变量传给命令"
        ));
    }
    if value.expose().chars().count() < ENV_VALUE_MIN_CHARS {
        return invalid(format!(
            "{name} 的值少于 {ENV_VALUE_MIN_CHARS} 个字符：这里只放密钥类的值，普通配置写进 skill 或命令里"
        ));
    }
    Ok(())
}
