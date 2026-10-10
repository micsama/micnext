//! 网关：HTTP API、固定 token 鉴权、SSE 实时流与稳定回放的交接。
//! 契约：docs/blueprints/gateway.md。

mod api;
mod channels;
mod config;
mod error;
mod limits;
mod log_stream;
mod logs;
mod models;
mod service;
mod settings;
mod sql;
mod stream;
mod web;

use mic_core::{Activation, BoxError, Module, ModuleConfig, Registry};

pub use crate::logs::DeveloperLogs;

use crate::config::RawConfig;
use crate::service::Gateway;

/// Web 会话所属的 Channel。
pub(crate) const WEB_CHANNEL: &str = "web";

pub struct GatewayModule {
    logs: DeveloperLogs,
}

impl GatewayModule {
    /// `logs` 由装配根创建并接入 tracing；本模块只读取。
    pub fn new(logs: DeveloperLogs) -> Self {
        Self { logs }
    }
}

impl Module for GatewayModule {
    fn name(&self) -> &'static str {
        "gateway"
    }

    fn activation(&self) -> Activation {
        Activation::Always
    }

    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError> {
        let config = cfg.parse::<RawConfig>()?.resolve()?;
        reg.channel_prompt(
            WEB_CHANNEL,
            "Web: Markdown is supported; tool activity is visible live.",
        );
        reg.service(Gateway {
            config,
            logs: self.logs.clone(),
        });
        Ok(())
    }
}
