//! 工具 port：`Tool` trait、参数边界、结果与失败分类。不依赖 mic-store / mic-core。
//! 契约：docs/blueprints/mic-tool.md；`ToolSpec` 见 docs/blueprints/provider-port.md §三.1。

mod handle;
mod schema;
mod tool;

pub use handle::ToolHandle;
pub use tool::{Tool, ToolContext, ToolError, ToolSpec};
