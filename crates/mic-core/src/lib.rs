//! 模块装配框架、内核句柄、模型 port 与执行主路径（调度、Agent 循环、崩溃收尾、实时事件）。
//! 不依赖具体 Channel。契约：docs/blueprints/mic-core-module.md。

mod assembly;
mod error;
mod event;
mod kernel;
mod limits;
mod module;
mod provider;
mod query;
mod recovery;
mod request;
mod scheduler;

pub use assembly::{Assembly, OnceOutcome, OneShot};
pub use error::{AssembleError, KernelError, RunError};
pub use event::{EventReceiver, KernelEvent, KernelEventKind, Lagged};
pub use kernel::Kernel;
pub use module::{Activation, BoxError, Module, ModuleConfig, Registry, Service};
pub use provider::{
    BoxStream, ModelEvent, ModelRequest, ModelResponse, Provider, ProviderError, StopReason,
};
