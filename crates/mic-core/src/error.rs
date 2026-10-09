use std::path::PathBuf;

use mic_message::SessionId;

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
    #[error("配置里的 [models] 已废弃：模型现在在网页 设置 → 模型 里添加，请从配置文件删掉整个 [models] 段")]
    RemovedModelsConfig,
    #[error("模型类型 `{kind}` 被重复登记")]
    DuplicateProviderKind { kind: &'static str },
    #[error("工具 `{name}` 被模块 `{first}` 和 `{second}` 重复登记")]
    DuplicateTool {
        name: String,
        first: &'static str,
        second: &'static str,
    },
}

#[derive(Debug, thiserror::Error)]
pub enum RunError {
    #[error("无法创建数据目录 {path}")]
    DataDir {
        path: PathBuf,
        source: std::io::Error,
    },
    #[error("数据目录 {path} 已被另一个 micnext 进程占用")]
    DataDirLocked { path: PathBuf },
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
    #[error("输入有误：{0}")]
    Input(#[from] crate::InputError),
    /// 执行路径里的 panic（core、工具或 Provider 的缺陷）：进程报错退出，重启后该轮收尾为 Interrupted。
    #[error("会话 {session_id:?} 的执行崩溃：{message}")]
    RunPanicked {
        session_id: SessionId,
        message: String,
    },
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
    #[error(transparent)]
    Settings(#[from] mic_store::SettingsError),
    #[error(transparent)]
    ModelSettings(#[from] mic_store::ModelSettingsError),
    #[error(transparent)]
    Input(#[from] crate::InputError),
    #[error("模型配置有误：{0}")]
    Config(#[from] crate::ConfigError),
    #[error(transparent)]
    Probe(#[from] crate::ProbeError),
}
