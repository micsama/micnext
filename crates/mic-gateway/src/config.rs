use std::net::SocketAddr;
use std::path::PathBuf;

use mic_core::BoxError;
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
}

pub(crate) struct Config {
    pub(crate) listen: SocketAddr,
    /// 一键更新的部署仓库；`None` = 不启用。
    pub(crate) update_repo: Option<PathBuf>,
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
            .map(|repo| -> Result<PathBuf, BoxError> {
                let repo = match repo.strip_prefix("~") {
                    Ok(rest) if repo.to_str().is_some_and(|path| path.starts_with("~/")) => {
                        let home = std::env::var_os("HOME")
                            .filter(|home| !home.is_empty())
                            .ok_or("环境变量 HOME 未设置，无法展开 [gateway] update_repo")?;
                        PathBuf::from(home).join(rest)
                    }
                    _ => repo,
                };
                if !repo.is_absolute() {
                    return Err(format!(
                        "[gateway] update_repo 必须是绝对路径或以 ~/ 开头，展开后为 \"{}\"",
                        repo.display()
                    )
                    .into());
                }
                Ok(repo)
            })
            .transpose()?;
        Ok(Config {
            listen,
            update_repo,
        })
    }
}
