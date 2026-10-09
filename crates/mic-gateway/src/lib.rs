//! 网关：HTTP API、固定 token 鉴权、SSE 实时流与稳定回放的交接。
//! 契约：docs/blueprints/gateway.md。

mod api;
mod channels;
mod config;
mod error;
mod limits;
mod models;
mod service;
mod settings;
mod stream;
mod web;

use mic_core::{Activation, BoxError, Module, ModuleConfig, Registry};

use crate::config::RawConfig;
use crate::service::Gateway;

/// Web 会话所属的 Channel。
pub(crate) const WEB_CHANNEL: &str = "web";

pub struct GatewayModule;

impl Module for GatewayModule {
    fn name(&self) -> &'static str {
        "gateway"
    }

    fn activation(&self) -> Activation {
        Activation::Always
    }

    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError> {
        let config = cfg.parse::<RawConfig>()?.resolve()?;
        reg.service(Gateway { config });
        Ok(())
    }
}
