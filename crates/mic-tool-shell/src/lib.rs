//! 基础工具 `bash`。契约：docs/blueprints/tools-basic.md §五.6；port 见 docs/blueprints/mic-tool.md。

mod bash;
mod limits;

use mic_core::{Activation, BoxError, Module, ModuleConfig, Registry};
use serde::Deserialize;

/// 常驻模块，登记 `bash`。
pub struct ShellModule;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {}

impl Module for ShellModule {
    fn name(&self) -> &'static str {
        "shell"
    }

    fn activation(&self) -> Activation {
        Activation::Always
    }

    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError> {
        let Config {} = cfg.parse()?;
        reg.tool(bash::Bash);
        Ok(())
    }
}
