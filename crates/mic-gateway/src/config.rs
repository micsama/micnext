use std::net::SocketAddr;
use std::path::PathBuf;

use mic_core::BoxError;
use serde::Deserialize;

const DEFAULT_LISTEN: &str = "127.0.0.1:7878";
const DEFAULT_WORKDIR: &str = "~/workspace/mic";

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    listen: Option<String>,
    token: Option<String>,
    workdir: Option<String>,
}

pub(crate) struct Config {
    pub(crate) listen: SocketAddr,
    /// `None` = 启动时随机生成。
    pub(crate) token: Option<String>,
    /// UTF-8 绝对路径。
    pub(crate) workdir: String,
}

impl RawConfig {
    pub(crate) fn resolve(self) -> Result<Config, BoxError> {
        let listen = self.listen.as_deref().unwrap_or(DEFAULT_LISTEN);
        let listen = listen.parse().map_err(|_| {
            format!("[gateway] listen 应写成 \"IP:端口\"（如 \"{DEFAULT_LISTEN}\"），当前为 \"{listen}\"")
        })?;
        if self.token.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err("[gateway] token 不能为空：删掉这一行则每次启动随机生成".into());
        }
        let workdir = self.workdir.as_deref().unwrap_or(DEFAULT_WORKDIR);
        let workdir = match workdir.strip_prefix("~/") {
            Some(rest) => home()?.join(rest),
            None if workdir.starts_with('/') => PathBuf::from(workdir),
            None => {
                return Err(format!(
                    "[gateway] workdir 必须是绝对路径或以 ~/ 开头，当前为 \"{workdir}\""
                )
                .into())
            }
        };
        let workdir = workdir
            .into_os_string()
            .into_string()
            .map_err(|p| format!("[gateway] workdir 不是 UTF-8 路径：{}", p.to_string_lossy()))?;
        Ok(Config {
            listen,
            token: self.token,
            workdir,
        })
    }
}

fn home() -> Result<PathBuf, BoxError> {
    std::env::var_os("HOME")
        .filter(|h| !h.is_empty())
        .map(PathBuf::from)
        .ok_or_else(|| "环境变量 HOME 未设置".into())
}
