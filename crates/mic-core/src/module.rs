use std::future::Future;
use std::pin::Pin;

use mic_store::Migration;
use tokio_util::sync::CancellationToken;

use crate::Kernel;

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub trait Module {
    /// 配置段名、迁移 module 名、表前缀共用此名。
    fn name(&self) -> &'static str;
    /// 第一阶段：同步、纯声明。解析自己的配置段并登记贡献；不做 I/O，拿不到 Store。
    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError>;
}

/// 本模块配置段的原始内容，只能由模块自己一次 parse 成强类型。
pub struct ModuleConfig(pub(crate) toml::Value);

impl ModuleConfig {
    /// `T` 应 `#[serde(deny_unknown_fields)]`，字段写错才会启动报错。
    pub fn parse<T: serde::de::DeserializeOwned>(self) -> Result<T, BoxError> {
        Ok(self.0.try_into()?)
    }
}

pub trait Service: Send + 'static {
    /// 第二阶段：Store 已打开、迁移已跑完。`stop` 触发后应尽快返回 `Ok(())`。
    /// 返回 `Err`、panic、或在未收到 `stop` 时返回 → 进程以错误退出。
    fn run(
        self: Box<Self>,
        kernel: Kernel,
        stop: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>>;
}

pub struct Registry {
    /// 正在 install 的模块，贡献归属于它。
    pub(crate) current: &'static str,
    pub(crate) migrations: Vec<Migration>,
    pub(crate) services: Vec<(&'static str, Box<dyn Service>)>,
}

impl Registry {
    pub fn migrations(&mut self, m: &'static [Migration]) {
        self.migrations.extend_from_slice(m);
    }

    pub fn service(&mut self, s: impl Service) {
        self.services.push((self.current, Box::new(s)));
    }
}
