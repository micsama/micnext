use std::future::Future;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use mic_message::{ContentPart, ExecFailureKind, ExecOutcome};

pub type BoxFuture<'a, T> = Pin<Box<dyn Future<Output = T> + Send + 'a>>;

/// 发给模型的工具定义；由各 Tool 自带，core 只汇总后放进请求。
#[derive(Debug, Clone, PartialEq, serde::Serialize)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema（object）。本进程产出、发往模型，框架不解读其结构。
    pub parameters: serde_json::Value,
}

pub trait Tool: Send + Sync + 'static {
    /// 模型参数在边界一次 parse 成此类型；schema 也由它生成。
    /// 必须 `#[serde(deny_unknown_fields)]`；兼容 DSH 的 bool 等参数在这里直接解析成强类型。
    type Args: serde::de::DeserializeOwned + schemars::JsonSchema + Send;

    /// 模型可见的工具名，全局唯一。
    fn name(&self) -> &str;
    /// 工具定义里的一句话简介。
    fn description(&self) -> &str;

    /// 丢弃返回的 future = 取消：实现须随之释放资源（如杀掉子进程组），不得留下后台副作用。
    /// 已经 `dispatch` 提交的后台任务归 core 所有，不受此约束。
    fn execute(
        &self,
        args: Self::Args,
        ctx: &ToolContext,
    ) -> impl Future<Output = Result<ToolOutput, ToolError>> + Send;
}

pub enum ToolOutput {
    Terminal(ExecOutcome),
    /// 已交给后台执行，结果日后以 `Completion` 回注。
    Dispatched {
        exec_id: String,
    },
}

impl ToolOutput {
    pub fn completed(output: Vec<ContentPart>) -> Self {
        Self::Terminal(ExecOutcome::Completed { output })
    }
}

/// 后台任务的输出去处；core 只保留最近一段。
pub trait OutputSink: Send {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, std::io::Result<()>>;
    /// 运行中给发起会话的中途通知，core 补上任务身份；返回即已落盘。任务收尾后到达的丢弃。
    fn notify<'a>(&'a mut self, text: &'a str) -> BoxFuture<'a, std::io::Result<()>>;
}

pub struct BackgroundTask {
    pub description: String,
    /// `None` = 不限时。
    pub timeout: Option<Duration>,
    /// 丢弃返回的 future = 取消，须释放全部资源（如杀进程组）。
    /// 结论只描述工具自身（如退出码），输出尾部由 core 拼进回注。
    #[allow(clippy::type_complexity)]
    pub run: Box<dyn FnOnce(Box<dyn OutputSink>) -> BoxFuture<'static, ExecOutcome> + Send>,
}

/// 后台派出 port，由 core 按调用绑定会话、run、发起人与工具调用；core 的工具可向下转型取绑定。
pub trait Background: std::any::Any + Send + Sync {
    /// 登记即提交；返回 exec_id。
    fn dispatch(&self, task: BackgroundTask) -> BoxFuture<'_, Result<String, ToolError>>;
}

/// 单次调用的执行上下文，由 core 构造。
#[derive(Clone)]
pub struct ToolContext {
    cwd: PathBuf,
    background: Arc<dyn Background>,
}

impl ToolContext {
    /// `cwd` 为绝对路径（会话 `pwd`），由 core 保证。
    pub fn new(cwd: PathBuf, background: Arc<dyn Background>) -> Self {
        Self { cwd, background }
    }

    /// 相对路径的解析基准、`bash` 的默认工作目录。
    pub fn cwd(&self) -> &Path {
        &self.cwd
    }

    pub fn background(&self) -> &dyn Background {
        &*self.background
    }
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ToolError {
    pub kind: ExecFailureKind,
    /// 给模型看，用英文。
    pub message: String,
}

impl ToolError {
    pub fn input(message: impl Into<String>) -> Self {
        Self::new(ExecFailureKind::Input, message)
    }

    pub fn business(message: impl Into<String>) -> Self {
        Self::new(ExecFailureKind::Business, message)
    }

    pub fn dependency(message: impl Into<String>) -> Self {
        Self::new(ExecFailureKind::Dependency, message)
    }

    fn new(kind: ExecFailureKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
        }
    }
}
