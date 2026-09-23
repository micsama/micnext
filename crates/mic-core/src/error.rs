use std::path::PathBuf;

use crate::BoxError;

#[derive(Debug, thiserror::Error)]
pub enum AssembleError {
    #[error("配置段 [{name}] 没有对应的模块（名字写错，或该模块未编译进来）")]
    UnknownModule { name: String },
    #[error("模块 `{name}` 重复注册")]
    DuplicateModule { name: &'static str },
    #[error("[core] 配置有误")]
    Core { source: BoxError },
    #[error("模块 `{module}` 装配失败")]
    Install {
        module: &'static str,
        source: BoxError,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("无法创建数据目录 {path}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
    /// `Err` 返回或 panic。
    #[error("模块 `{module}` 的后台任务失败")]
    Service {
        module: &'static str,
        source: BoxError,
    },
    #[error("模块 `{module}` 的后台任务未收到停止信号就退出了")]
    ServiceExited { module: &'static str },
}

#[derive(Debug, thiserror::Error)]
pub enum KernelError {
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
}
