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
    #[error("模型类型 `{kind}` 被重复登记")]
    DuplicateProviderKind { kind: &'static str },
    #[error("Channel `{channel}` 的登录能力被重复登记")]
    DuplicateChannelSetup { channel: &'static str },
    #[error("Channel `{channel}` 的渠道提示被重复登记")]
    DuplicateChannelPrompt { channel: &'static str },
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
    #[error("无法读取主密钥文件 {path}")]
    KeyFile {
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
    Schedule(#[from] crate::ScheduleError),
    #[error("设置备份无效：{0}")]
    BackupInvalid(String),
    #[error(transparent)]
    Title(#[from] crate::TitleError),
    #[error(transparent)]
    CommandParse(#[from] crate::CommandParseError),
    #[error(transparent)]
    ContextCommand(#[from] mic_store::ContextCommandError),
    #[error("会话不存在")]
    SessionNotFound,
    #[error("只有主会话可以归档")]
    SessionNotRoot,
    #[error("子任务会话只接收派出它的会话的追问，不能直接发送")]
    TaskSessionInput,
    #[error(transparent)]
    Workdir(#[from] WorkdirError),
    #[error(transparent)]
    Store(#[from] mic_store::StoreError),
    #[error(transparent)]
    Settings(#[from] mic_store::SettingsError),
    #[error(transparent)]
    ModelSettings(#[from] mic_store::ModelSettingsError),
    #[error(transparent)]
    Tag(#[from] mic_store::TagError),
    #[error(transparent)]
    Input(#[from] crate::InputError),
    #[error("模型配置有误：{0}")]
    Config(#[from] crate::ConfigError),
    #[error(transparent)]
    Probe(#[from] crate::ProbeError),
    #[error(transparent)]
    Diagnostic(#[from] mic_store::DiagnosticError),
    #[error(transparent)]
    Job(#[from] crate::JobAccessError),
    #[error(transparent)]
    Push(#[from] crate::PushError),
}

#[derive(Debug, thiserror::Error)]
pub enum WorkdirError {
    #[error("环境变量 HOME 未设置，无法展开工作目录")]
    HomeMissing,
    #[error("工作目录 {} 不是 UTF-8 路径", path.display())]
    NonUtf8 { path: PathBuf },
    #[error("无法创建工作目录 {}：{source}", path.display())]
    Create {
        path: PathBuf,
        source: std::io::Error,
    },
}

pub(crate) fn chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        let detail = cause.to_string();
        if !message.ends_with(&detail) {
            message.push_str(": ");
            message.push_str(&detail);
        }
        source = cause.source();
    }
    message
}
