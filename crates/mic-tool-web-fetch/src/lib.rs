//! 基础工具 `web_fetch`。契约：docs/blueprints/tools-basic.md §五.7；port 见 docs/blueprints/mic-tool.md。

mod fetch;
mod limits;

use mic_core::{Activation, BoxError, Module, ModuleConfig, Registry};
use serde::Deserialize;

/// 常驻模块，登记 `web_fetch`。
pub struct WebFetchModule;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {}

impl Module for WebFetchModule {
    fn name(&self) -> &'static str {
        "web_fetch"
    }

    fn activation(&self) -> Activation {
        Activation::Always
    }

    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError> {
        let Config {} = cfg.parse()?;
        reg.tool(fetch::WebFetch::new()?);
        Ok(())
    }
}
