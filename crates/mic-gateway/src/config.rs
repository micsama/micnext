use std::collections::BTreeMap;
use std::net::SocketAddr;
use std::path::PathBuf;

use mic_core::BoxError;
use mic_store::MAIN_SQL_DB;
use serde::de::IgnoredAny;
use serde::Deserialize;

const DEFAULT_LISTEN: &str = "127.0.0.1:7878";

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    listen: Option<String>,
    /// 已改为登录制，写了即报错指路。
    token: Option<IgnoredAny>,
    update_repo: Option<PathBuf>,
    /// 已迁到设置页，写了即报错指路。
    workdir: Option<IgnoredAny>,
    #[serde(default)]
    sql_databases: BTreeMap<String, PathBuf>,
}

pub(crate) struct Config {
    pub(crate) listen: SocketAddr,
    /// 一键更新的部署仓库；`None` = 不启用。
    pub(crate) update_repo: Option<PathBuf>,
    /// SQL 页可切换的额外库：名字 → 路径。
    pub(crate) sql_databases: BTreeMap<String, PathBuf>,
}

impl RawConfig {
    pub(crate) fn resolve(self) -> Result<Config, BoxError> {
        if self.workdir.is_some() {
            return Err("[gateway] workdir 已移到网页 设置 → 对话偏好 → 新会话默认工作目录，请从配置删掉这一行".into());
        }
        if self.token.is_some() {
            return Err("[gateway] token 已取消，网页改为登录：请从配置删掉这一行，再运行 micnext --set-password 设置密码".into());
        }
        let listen = self.listen.as_deref().unwrap_or(DEFAULT_LISTEN);
        let listen = listen.parse().map_err(|_| {
            format!("[gateway] listen 应写成 \"IP:端口\"（如 \"{DEFAULT_LISTEN}\"），当前为 \"{listen}\"")
        })?;
        let update_repo = self
            .update_repo
            .map(|repo| absolute(repo, "[gateway] update_repo"))
            .transpose()?;
        let sql_databases = self
            .sql_databases
            .into_iter()
            .map(|(name, path)| {
                if name == MAIN_SQL_DB
                    || !name.chars().all(|c| c.is_ascii_alphanumeric() || c == '_')
                {
                    return Err(format!(
                        "[gateway.sql_databases] 的名字只能用字母、数字、下划线，且不能叫 {MAIN_SQL_DB}，当前为 \"{name}\""
                    )
                    .into());
                }
                let path = absolute(path, &format!("[gateway.sql_databases] {name}"))?;
                Ok((name, path))
            })
            .collect::<Result<_, BoxError>>()?;
        Ok(Config {
            listen,
            update_repo,
            sql_databases,
        })
    }
}

/// 展开 `~/`，要求绝对路径。
fn absolute(path: PathBuf, field: &str) -> Result<PathBuf, BoxError> {
    let path = match path.strip_prefix("~") {
        Ok(rest) if path.to_str().is_some_and(|p| p.starts_with("~/")) => {
            let home = std::env::var_os("HOME")
                .filter(|home| !home.is_empty())
                .ok_or_else(|| format!("环境变量 HOME 未设置，无法展开 {field}"))?;
            PathBuf::from(home).join(rest)
        }
        _ => path,
    };
    if !path.is_absolute() {
        return Err(format!(
            "{field} 必须是绝对路径或以 ~/ 开头，展开后为 \"{}\"",
            path.display()
        )
        .into());
    }
    Ok(path)
}
