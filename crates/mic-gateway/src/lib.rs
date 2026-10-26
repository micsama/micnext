//! 网关：HTTP API、固定 token 鉴权、SSE 实时流与稳定回放的交接。
//! 契约：docs/blueprints/gateway.md。

mod api;
mod backup;
mod channels;
mod config;
mod context;
mod error;
mod global_stream;
mod jobs;
mod limits;
mod log_store;
mod log_stream;
mod logs;
mod models;
mod pushes;
mod schedules;
mod service;
mod settings;
mod sql;
mod sse;
mod stream;
mod tags;
mod ticket;
mod trace;
mod update;
mod web;

use mic_core::{BoxError, Module, ModuleConfig, Registry};

pub use crate::logs::DeveloperLogs;
pub use crate::update::{BuildVersion, RestartRequest};

use tokio::sync::mpsc;

use crate::config::RawConfig;
use crate::service::Gateway;

/// Web 会话所属的 Channel。
pub(crate) const WEB_CHANNEL: &str = "web";

pub struct GatewayModule {
    logs: DeveloperLogs,
    restart: mpsc::Sender<RestartRequest>,
    version: Option<BuildVersion>,
}

impl GatewayModule {
    /// `logs` 由装配根创建并接入 tracing；本模块只读取。`version` 为本程序构建时的提交，未知为 `None`。
    pub fn new(
        logs: DeveloperLogs,
        restart: mpsc::Sender<RestartRequest>,
        version: Option<BuildVersion>,
    ) -> Self {
        Self {
            logs,
            restart,
            version,
        }
    }
}

impl Module for GatewayModule {
    fn name(&self) -> &'static str {
        "gateway"
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
            restart: self.restart.clone(),
            version: self.version,
        });
        Ok(())
    }
}
