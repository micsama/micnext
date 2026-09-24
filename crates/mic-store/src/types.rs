use std::collections::BTreeSet;

use mic_message::{
    ContextBoundary, ExecOutcome, Message, MessageAuthor, MessageContent, PersonId, SessionEntryId,
    SessionId,
};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QueryId(pub i64);

/// 外部身份：某 Channel 上的某个发送者。
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct Identity {
    pub channel: String,
    pub external_id: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Person {
    pub id: PersonId,
    /// 配置里声明的 person 用配置名；自动注册的用 `{channel}:{external_id}`。
    pub name: String,
    pub created_at: i64,
}

/// 发送目标：`channel` 决定由哪个 Channel 投递；`payload` 是该 Channel 自己
/// 编解码的版本化不透明数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryTarget {
    pub channel: String,
    pub version: u32,
    pub payload: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolScope {
    All,
    Only(BTreeSet<String>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum SessionKind {
    /// Channel 入站创建；`(channel, chat)` 全局唯一。
    Root { channel: String, chat: String },
    /// 子 agent；父 session 在 `Session.parent_session_id`。
    Task { parent_tool_call_id: String },
    /// 模块触发（cron、webhook…）。`ref_id` 由该模块解释，内核只当来源元数据。
    Triggered { module: String, ref_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub id: SessionId,
    pub kind: SessionKind,
    pub parent_session_id: Option<SessionId>,
    pub delivery_target: Option<DeliveryTarget>,
    pub pwd: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
}

/// 会话列表分页游标：上一页最后一项的排序键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCursor {
    pub last_activity_at: i64,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub session: Session,
    /// 最新 entry 的 `created_at`。
    pub last_activity_at: i64,
    /// 第一条用户文字消息的开头；没有则 `None`。
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 还有更早的会话时给出，原样传回取下一页。
    pub next: Option<SessionCursor>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum QueryState {
    Executing,
    Completed,
    Failed { reason: FailureReason },
    Cancelled { reason: CancelReason },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum FailureReason {
    Timeout,
    Provider {
        message: String,
    },
    Interrupted,
    /// 轮次用尽：模型已做过不带工具的总结，但任务未必完成。
    MaxTurns {
        limit: u32,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum CancelReason {
    User,
    ParentCascade,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Query {
    pub id: QueryId,
    pub session_id: SessionId,
    pub person_id: PersonId,
    pub claimed_start_id: SessionEntryId,
    pub claimed_end_id: SessionEntryId,
    pub state: QueryState,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextWindow {
    pub summary: Option<String>,
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingDelivery {
    pub message: Message,
    pub target: DeliveryTarget,
}

/// 一次模型调用的 token 用量。费用不落盘：展示时按当前价格配置换算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ModelCallPurpose {
    Query(QueryId),
    Compaction,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ModelCallOutcome {
    Completed,
    Failed { message: String },
}

/// 模块迁移。`module` 不得为 `"core"`（内核保留）；同一模块 `version` 从 1 连续递增。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub module: &'static str,
    pub version: u32,
    pub sql: &'static str,
}

/// 可 claim 的用户输入：Channel 入站，以及 Task/Triggered 的起始消息。
#[derive(Debug, Clone, PartialEq)]
pub struct UserInput {
    pub session_id: SessionId,
    pub person_id: PersonId,
    pub content: MessageContent,
    pub created_at: i64,
}

/// `wait=false` 的终态回报（可 claim）。person 来自内存执行实例句柄。
#[derive(Debug, Clone, PartialEq)]
pub struct CompletionInput {
    pub session_id: SessionId,
    pub person_id: PersonId,
    pub tool_name: String,
    pub exec_id: String,
    pub outcome: ExecOutcome,
    pub created_at: i64,
}

/// 不可 claim 的产出：模型输出、工具结果、HarnessNote、Notification。
#[derive(Debug, Clone, PartialEq)]
pub struct OutputInput {
    pub session_id: SessionId,
    pub author: MessageAuthor,
    pub content: MessageContent,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BoundaryInput {
    pub session_id: SessionId,
    pub boundary: ContextBoundary,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    pub kind: SessionKind,
    pub parent_session_id: Option<SessionId>,
    pub delivery_target: Option<DeliveryTarget>,
    pub pwd: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelCallInput {
    pub session_id: SessionId,
    pub purpose: ModelCallPurpose,
    pub model: String,
    pub usage: Usage,
    pub outcome: ModelCallOutcome,
    pub started_at: i64,
    pub finished_at: i64,
}
