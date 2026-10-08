//! 模型 port。契约：docs/blueprints/provider-port.md。

use std::collections::BTreeMap;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use mic_message::{Message, ReplyBlock};
use mic_store::{SecretValue, Usage};
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

/// 模型条目配置有误；`message` 不含秘密值。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{field}：{message}")]
pub struct ConfigError {
    /// 稳定的字段路径，如 `base_url`；整体问题用 `config`。
    pub field: String,
    pub message: String,
}

pub type BoxFuture<T> = Pin<Box<dyn std::future::Future<Output = T> + Send>>;

/// 测试服务商连接失败的原因；文案面向用户，不含秘密与上游响应体。
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ProbeError {
    #[error("连不上服务商（{0}）：请检查地址是否正确、服务是否已启动、网络是否通")]
    Network(String),
    #[error("鉴权没通过：API key 无效或没有权限，请检查 key")]
    Auth,
    #[error("服务商返回了意外的结果（{0}）：请确认地址是否正确，如是否以 /v1 结尾")]
    Unexpected(String),
}

/// 一类 Provider 的工厂；配置 JSON 只由实现自己解析。契约：docs/blueprints/model-settings.md §十。
pub trait ProviderFactory: Send + Sync + 'static {
    fn kind(&self) -> &'static str;
    fn display_name(&self) -> &'static str;
    /// 服务商连接配置的保存前校验，纯本地；返回规范化后的 JSON。
    fn check_endpoint(&self, json: &str) -> Result<String, ConfigError>;
    /// 模型参数的保存前校验（结合服务商配置）；返回规范化后的 JSON。
    fn check_model(
        &self,
        endpoint_json: &str,
        name: &str,
        model_json: &str,
    ) -> Result<String, ConfigError>;
    /// 服务商未保存 key 时读哪个环境变量；`None` = 无约定。
    fn key_env(&self, endpoint_json: &str) -> Option<&'static str>;
    /// 认领后创建本轮实例；`key` 已含环境变量回退，缺省即不发鉴权头。
    fn build(
        &self,
        endpoint_json: &str,
        key: Option<SecretValue>,
        name: &str,
        model_json: &str,
    ) -> Result<Arc<dyn Provider>, ConfigError>;
    /// 联网请求服务商的模型列表（测试连接）。
    fn list_models(
        &self,
        endpoint_json: &str,
        key: Option<SecretValue>,
    ) -> BoxFuture<Result<Vec<String>, ProbeError>>;
}

/// 已存 key 优先，否则读环境变量（非空）。
pub(crate) fn resolve_key(
    stored: Option<SecretValue>,
    env: Option<&'static str>,
) -> Option<SecretValue> {
    stored.or_else(|| {
        let value = std::env::var(env?).ok().filter(|v| !v.is_empty())?;
        Some(SecretValue::new(value))
    })
}

/// 启动时装配好的工厂表，按 `kind` 查找。
#[derive(Default)]
pub(crate) struct Factories(BTreeMap<&'static str, Box<dyn ProviderFactory>>);

impl Factories {
    /// 重复 kind 返回 `Err(kind)`。
    pub(crate) fn insert(&mut self, f: Box<dyn ProviderFactory>) -> Result<(), &'static str> {
        let kind = f.kind();
        match self.0.insert(kind, f) {
            None => Ok(()),
            Some(_) => Err(kind),
        }
    }

    pub(crate) fn get(&self, kind: &str) -> Result<&dyn ProviderFactory, ConfigError> {
        self.0
            .get(kind)
            .map(|f| f.as_ref())
            .ok_or_else(|| ConfigError {
                field: "kind".into(),
                message: format!("未知的模型类型 `{kind}`"),
            })
    }

    pub(crate) fn kinds(&self) -> Vec<ProviderKindView> {
        self.0
            .values()
            .map(|f| ProviderKindView {
                kind: f.kind().to_owned(),
                display_name: f.display_name().to_owned(),
            })
            .collect()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProviderKindView {
    pub kind: String,
    pub display_name: String,
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
