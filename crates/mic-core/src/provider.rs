//! 模型 port。契约：docs/blueprints/provider-port.md。

use std::pin::Pin;
use std::time::Duration;

use mic_message::{Message, ReplyBlock};
use mic_store::Usage;
use mic_tool::ToolSpec;

pub type BoxStream<T> = Pin<Box<dyn futures_core::Stream<Item = T> + Send>>;

pub trait Provider: Send + Sync + 'static {
    /// 请求时发给上游的模型名；调用记录与推理回传判定用同一口径。
    fn model(&self) -> &str;

    /// 发起一次流式调用。连接失败也以流里的第一个 `Err` 返回。
    /// 丢弃流 = 取消调用（实现须随之中止 HTTP 请求）。
    /// 流以恰好一个 `Finished` 或一个 `Err` 结束，之后不再产出。
    fn stream(&self, req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>>;
}

#[derive(Debug, Clone)]
pub struct ModelRequest {
    /// 系统提示；空串 = 不发。
    pub system: String,
    /// 本次上下文内的历史消息，按 core 组装的顺序。Provider 逐条经 `Message::model_view` 映射，
    /// `None` 跳过；一条 `Reply` 即一个 assistant turn。
    pub messages: Vec<Message>,
    /// 空 = 不开启工具调用。
    pub tools: Vec<ToolSpec>,
}

#[derive(Debug, Clone)]
pub enum ModelEvent {
    /// 回复正文增量，仅供实时显示。
    TextDelta(String),
    /// 可见推理增量，仅供实时显示。
    ReasoningDelta(String),
    /// 最终结果；增量的累积以此为准。
    Finished(ModelResponse),
}

#[derive(Debug, Clone)]
pub struct ModelResponse {
    /// 按生成顺序：推理、正文、工具调用。
    pub blocks: Vec<ReplyBlock>,
    pub stop: StopReason,
    /// 上游没报为 `None`。
    pub usage: Option<Usage>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StopReason {
    /// 正常说完。
    EndTurn,
    /// 要调用工具（`blocks` 含 `ToolCall`）。
    ToolUse,
    /// 撞到输出长度上限，回复不完整。
    MaxTokens,
    /// 被上游内容审核截断。
    ContentFilter,
}

/// 只按"重试有没有用、该谁去修"一个轴划分；状态码到四类的映射由各实现在边界完成。
#[derive(Debug, thiserror::Error)]
pub enum ProviderError {
    /// key 无效、无权限、余额不足：要人去处理账户或配置，不重试。
    #[error("模型账户不可用：{message}")]
    Account { message: String },
    /// 请求本身被拒（上下文超长、参数非法、含不支持的内容）：重试无用。
    #[error("模型拒绝了请求：{message}")]
    Rejected { message: String },
    /// 限流、过载、5xx、网络、超时、流中途断开：可重试。
    #[error("模型暂时不可用：{message}")]
    Transient {
        message: String,
        retry_after: Option<Duration>,
    },
    /// 响应不符合协议（解析失败、事件顺序错）：实现与上游不匹配，不重试。
    #[error("模型响应不符合协议：{message}")]
    Protocol { message: String },
}
