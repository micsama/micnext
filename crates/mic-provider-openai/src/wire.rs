//! Chat Completions 的线上格式。请求借用 `ModelRequest`；响应在边界一次 parse 成强类型。

use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub(crate) struct ChatRequest<'a> {
    pub(crate) model: &'a str,
    pub(crate) messages: Vec<WireMessage<'a>>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub(crate) tools: Vec<WireTool<'a>>,
    pub(crate) stream: bool,
    pub(crate) stream_options: StreamOptions,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) max_tokens: Option<u32>,
    /// DeepSeek：`none`（关闭思考）/`low`/`high`/`max`。
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) reasoning_effort: Option<&'static str>,
}

#[derive(Serialize)]
pub(crate) struct StreamOptions {
    pub(crate) include_usage: bool,
}

#[derive(Serialize)]
#[serde(tag = "role", rename_all = "lowercase")]
pub(crate) enum WireMessage<'a> {
    System {
        content: &'a str,
    },
    User {
        content: UserContent,
    },
    Assistant {
        /// 只有工具调用时为 `null`。
        content: Option<String>,
        #[serde(skip_serializing_if = "Option::is_none")]
        reasoning_content: Option<String>,
        #[serde(skip_serializing_if = "Vec::is_empty")]
        tool_calls: Vec<WireToolCall<'a>>,
    },
    Tool {
        tool_call_id: &'a str,
        content: String,
    },
}

/// 纯文本 user 消息保持字符串；含图时用分片数组。
#[derive(Serialize)]
#[serde(untagged)]
pub(crate) enum UserContent {
    Text(String),
    Parts(Vec<UserPart>),
}

#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub(crate) enum UserPart {
    Text { text: String },
    ImageUrl { image_url: ImageUrl },
}

#[derive(Serialize)]
pub(crate) struct ImageUrl {
    /// `data:<MIME>;base64,<数据>`。
    pub(crate) url: String,
}

#[derive(Serialize)]
pub(crate) struct WireToolCall<'a> {
    pub(crate) id: &'a str,
    #[serde(rename = "type")]
    pub(crate) kind: &'static str,
    pub(crate) function: WireFunctionCall<'a>,
}

#[derive(Serialize)]
pub(crate) struct WireFunctionCall<'a> {
    pub(crate) name: &'a str,
    pub(crate) arguments: String,
}

#[derive(Serialize)]
pub(crate) struct WireTool<'a> {
    #[serde(rename = "type")]
    pub(crate) kind: &'static str,
    pub(crate) function: WireFunction<'a>,
}

#[derive(Serialize)]
pub(crate) struct WireFunction<'a> {
    pub(crate) name: &'a str,
    pub(crate) description: &'a str,
    pub(crate) parameters: &'a serde_json::Value,
}

/// SSE `data:` 里的一个分片。
#[derive(Deserialize)]
pub(crate) struct Chunk {
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) id: Option<String>,
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) model: String,
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) created: Option<i64>,
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) system_fingerprint: Option<String>,
    #[serde(default)]
    pub(crate) choices: Vec<Choice>,
    pub(crate) usage: Option<WireUsage>,
}

#[derive(Deserialize)]
pub(crate) struct Choice {
    pub(crate) index: u32,
    pub(crate) delta: Delta,
    pub(crate) finish_reason: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct Delta {
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) role: Option<String>,
    pub(crate) content: Option<String>,
    /// DeepSeek 方言。
    pub(crate) reasoning_content: Option<String>,
    /// Ollama 方言。
    pub(crate) reasoning: Option<String>,
    #[serde(default)]
    pub(crate) tool_calls: Vec<ToolCallDelta>,
}

#[derive(Deserialize)]
pub(crate) struct ToolCallDelta {
    pub(crate) index: u32,
    pub(crate) id: Option<String>,
    #[serde(rename = "type")]
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) kind: Option<String>,
    pub(crate) function: Option<FunctionDelta>,
}

#[derive(Deserialize)]
pub(crate) struct FunctionDelta {
    pub(crate) name: Option<String>,
    pub(crate) arguments: Option<String>,
}

#[derive(Deserialize)]
pub(crate) struct WireUsage {
    pub(crate) prompt_tokens: u64,
    pub(crate) completion_tokens: u64,
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) total_tokens: Option<u64>,
    /// DeepSeek 方言。
    pub(crate) prompt_cache_hit_tokens: Option<u64>,
    #[expect(dead_code, reason = "上游字段完整接收，暂无消费者")]
    pub(crate) prompt_cache_miss_tokens: Option<u64>,
    pub(crate) prompt_tokens_details: Option<PromptTokensDetails>,
    pub(crate) completion_tokens_details: Option<CompletionTokensDetails>,
}

#[derive(Deserialize)]
pub(crate) struct PromptTokensDetails {
    pub(crate) cached_tokens: Option<u64>,
}

#[derive(Deserialize)]
pub(crate) struct CompletionTokensDetails {
    pub(crate) reasoning_tokens: Option<u64>,
}

/// 非 2xx 响应体。
#[derive(Deserialize)]
pub(crate) struct ErrorBody {
    pub(crate) error: ErrorDetail,
}

#[derive(Deserialize)]
pub(crate) struct ErrorDetail {
    pub(crate) message: String,
}
