//! 基础文件工具 `read`、`write`、`edit`、`glob`、`grep`。
//! 契约：docs/blueprints/tools-basic.md §五.1～5.5；port 见 docs/blueprints/mic-tool.md。

mod common;
mod edit;
mod glob;
mod grep;
mod limits;
mod read;
mod write;

use mic_core::{Activation, BoxError, Module, ModuleConfig, Registry};
use serde::Deserialize;

/// 常驻模块，按顺序登记 `read`、`write`、`edit`、`glob`、`grep`。
pub struct FsModule;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Config {}

impl Module for FsModule {
    fn name(&self) -> &'static str {
        "fs"
    }

    fn activation(&self) -> Activation {
        Activation::Always
    }

    fn install(&self, reg: &mut Registry, cfg: ModuleConfig) -> Result<(), BoxError> {
        let Config {} = cfg.parse()?;
        reg.tool(read::Read);
        reg.tool(write::Write);
        reg.tool(edit::Edit);
        reg.tool(glob::Glob);
        reg.tool(grep::Grep);
        Ok(())
    }
}
