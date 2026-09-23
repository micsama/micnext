//! 模块装配框架与内核句柄；执行主路径（调度、Agent 循环）随 M6 加入。
//! 不依赖具体 Channel。契约：docs/blueprints/mic-core-module.md。

mod assembly;
mod error;
mod kernel;
mod module;

pub use assembly::Assembly;
pub use error::{AssembleError, KernelError, RunError};
pub use kernel::Kernel;
pub use module::{BoxError, Module, ModuleConfig, Registry, Service};
