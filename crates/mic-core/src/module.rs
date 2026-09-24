use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use mic_store::Migration;
use mic_tool::{Tool, ToolHandle};
use tokio_util::sync::CancellationToken;

use crate::{Kernel, Provider};

pub type BoxError = Box<dyn std::error::Error + Send + Sync>;

pub trait Module {
    /// 配置段名、迁移 module 名、表前缀共用此名。
    fn name(&self) -> &'static str;
    /// 缺省 `WhenConfigured`。
    fn activation(&self) -> Activation {
        Activation::WhenConfigured
    }
    /// 第一阶段：同步、纯声明。解析自己的配置段并登记贡献；不做 I/O，拿不到 Store。
    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError>;
}

/// 模块何时装配。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// 配置里有同名段（或 `[models]` 里有对应 `kind` 的条目）才装配。
    WhenConfigured,
    /// 总是装配；没有同名段时收到空表。
    Always,
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
    pub(crate) providers: Vec<(String, Arc<dyn Provider>)>,
    pub(crate) tools: Vec<(&'static str, ToolHandle)>,
}

impl Registry {
    pub fn migrations(&mut self, m: &'static [Migration]) {
        self.migrations.extend_from_slice(m);
    }

    pub fn service(&mut self, s: impl Service) {
        self.services.push((self.current, Box::new(s)));
    }

    /// 模型模块对收到的每个 `[models.<name>]` 条目登记一个实例，`name` 即条目名。
    pub fn provider(&mut self, name: impl Into<String>, p: impl Provider) {
        self.providers.push((name.into(), Arc::new(p)));
    }

    /// 工具模块在 `install` 中登记；一个模块可登记多个工具。
    pub fn tool(&mut self, t: impl Tool) {
        self.tools.push((self.current, ToolHandle::new(t)));
    }
}
