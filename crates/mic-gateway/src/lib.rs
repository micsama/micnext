//! 网关：HTTP API、登录鉴权、SSE 实时流与稳定回放的交接。
//! 契约：docs/blueprints/archive/gateway.md，鉴权见 docs/blueprints/web-login.md。

mod api;
mod auth;
mod backup;
mod channels;
mod config;
mod context;
mod env_vars;
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
mod schema;
mod search;
mod service;
mod settings;
mod sql;
mod sse;
mod stream;
mod tags;
mod trace;
mod update;
mod web;
mod web_push;

use mic_core::{BoxError, Module, ModuleConfig, Registry};

pub use crate::auth::set_password;
pub use crate::logs::DeveloperLogs;
pub use crate::update::{BuildVersion, RestartRequest};

use tokio::sync::mpsc;

use crate::config::RawConfig;
use crate::service::Gateway;

/// Web 会话所属的 Channel。
pub(crate) const WEB_CHANNEL: &str = "web";

/// micnext-guide 正文：定位与索引链接在前，网页功能地图在后。契约：docs/blueprints/micnext-guide.md。
const GUIDE: &str = concat!(
    include_str!("../../../docs/guide/SKILL.md"),
    "\n\n",
    include_str!("../../../docs/web-features.md"),
);

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
            "Web：支持 Markdown，工具活动实时可见。页面有会话、定时任务、设置、Trace、开发者诊断；功能在哪、怎么用先加载 `micnext-guide`。",
        );
        // 与开发文档同一来源；附带文档只在部署了源码仓库时可读。
        reg.skill(
            "micnext-guide",
            "micnext 的网页功能、能力边界与实现索引。回答功能在哪、怎么用，或做功能对比、开发维护、问题排查之前先加载，不凭印象答。",
            GUIDE,
            config.update_repo.as_ref().map(|repo| repo.join("docs/guide")),
        );
        reg.input_prediction(WEB_CHANNEL);
        reg.schemas(schema::SCHEMAS);
        reg.service(Gateway {
            config,
            logs: self.logs.clone(),
            restart: self.restart.clone(),
            version: self.version,
        });
        Ok(())
    }
}

#[cfg(test)]
mod tests;
