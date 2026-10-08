use std::net::SocketAddr;

use mic_core::BoxError;
use serde::de::IgnoredAny;
use serde::Deserialize;

const DEFAULT_LISTEN: &str = "127.0.0.1:7878";

#[derive(Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub(crate) struct RawConfig {
    listen: Option<String>,
    token: Option<String>,
    /// 已迁到设置页，写了即报错指路。
    workdir: Option<IgnoredAny>,
}

pub(crate) struct Config {
    pub(crate) listen: SocketAddr,
    /// `None` = 启动时随机生成。
    pub(crate) token: Option<String>,
}

impl RawConfig {
    pub(crate) fn resolve(self) -> Result<Config, BoxError> {
        if self.workdir.is_some() {
            return Err("[gateway] workdir 已移到网页 设置 → 对话偏好 → 新会话默认工作目录，请从配置删掉这一行".into());
        }
        let listen = self.listen.as_deref().unwrap_or(DEFAULT_LISTEN);
        let listen = listen.parse().map_err(|_| {
            format!("[gateway] listen 应写成 \"IP:端口\"（如 \"{DEFAULT_LISTEN}\"），当前为 \"{listen}\"")
        })?;
        if self.token.as_deref().is_some_and(|t| t.trim().is_empty()) {
            return Err("[gateway] token 不能为空：删掉这一行则每次启动随机生成".into());
        }
        Ok(Config {
            listen,
            token: self.token,
        })
    }
}
