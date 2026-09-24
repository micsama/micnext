use std::future::Future;
use std::path::{Path, PathBuf};

use mic_message::{ContentPart, ExecFailureKind};

/// 发给模型的工具定义；由各 Tool 自带，core 只汇总后放进请求。
#[derive(Debug, Clone, PartialEq)]
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
    /// 使用提示；工具在本次请求中可用时拼入 system prompt。`None` = 无提示。
    fn prompt_hint(&self) -> Option<&str>;

    /// 丢弃返回的 future = 取消：实现须随之释放资源（如杀掉子进程组），不得留下后台副作用。
    fn execute(
        &self,
        args: Self::Args,
        ctx: &ToolContext,
    ) -> impl Future<Output = Result<Vec<ContentPart>, ToolError>> + Send;
}

/// 单次调用的执行上下文，由 core 构造。
#[derive(Debug, Clone)]
pub struct ToolContext {
    cwd: PathBuf,
}

impl ToolContext {
    /// `cwd` 为绝对路径（会话 `pwd`），由 core 保证。
    pub fn new(cwd: PathBuf) -> Self {
        Self { cwd }
    }

    /// 相对路径的解析基准、`bash` 的默认工作目录。
    pub fn cwd(&self) -> &Path {
        &self.cwd
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
