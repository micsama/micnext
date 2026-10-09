use std::collections::BTreeSet;

use mic_message::{Message, PersonId, ReplyBlock, SessionId};
use serde::{Deserialize, Serialize};

use crate::SecretValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ModelCallId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersonaId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct EndpointId(pub i64);

/// 服务商：连接配置 + 可选 API key。key 只暴露是否已设置。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EndpointView {
    pub id: EndpointId,
    pub name: String,
    pub kind: String,
    /// 该 kind 的 Provider 自己解析的连接配置，无秘密。
    pub config_json: String,
    pub key_set: bool,
}

pub enum CredentialWrite {
    /// 不动已存的 key；创建时无效。
    Keep,
    Clear,
    Set(SecretValue),
}

pub struct EndpointWrite {
    pub name: String,
    pub kind: String,
    pub config_json: String,
    pub credential: CredentialWrite,
}

/// 服务商下的一个模型：模型名 + 该 kind 自己解析的参数。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelView {
    pub id: ModelId,
    pub endpoint_id: EndpointId,
    pub name: String,
    pub config_json: String,
}

pub struct ModelWrite {
    pub endpoint_id: EndpointId,
    pub name: String,
    pub config_json: String,
}

/// 认领时对会话所选模型的读定结果；秘密只在此出现。
pub enum ClaimedModel {
    /// 会话与默认均无模型。
    Missing,
    Deleted {
        id: ModelId,
        name: String,
    },
    Selected {
        id: ModelId,
        /// 「服务商 / 模型」。
        name: String,
        kind: String,
        endpoint_json: String,
        model_name: String,
        model_json: String,
        key: Option<SecretValue>,
    },
}

pub struct ClaimedRun {
    pub run: Run,
    pub settings: RunSettings,
    pub model: ClaimedModel,
}

/// 人设：名字 + 提示词。内置的只读；删除为软删除，按 id 仍可读。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Persona {
    pub id: PersonaId,
    pub name: String,
    pub prompt: String,
    pub builtin: bool,
    pub deleted: bool,
}

/// 对话偏好，全局一份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Settings {
    /// 始终是未删除的人设。
    pub default_persona: PersonaId,
    /// 对所有人设生效；空串 = 无。
    pub general_prompt: String,
    /// 新 Web 会话的工作目录，用户写法：绝对路径或 `~/` 开头。
    pub default_workdir: String,
    pub max_turns: u32,
}

/// 认领时定下的本轮设置，执行期间不变；同时落为 run 快照。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSettings {
    pub persona: Persona,
    pub general_prompt: String,
    pub max_turns: u32,
}

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
    /// 下一轮用的人设；已删除时下一轮改用默认人设并写回。
    pub persona_id: PersonaId,
    /// 下一轮用的模型；`None` = 认领时取当时默认并写回。
    pub model_id: Option<ModelId>,
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
    /// 最新消息的 `created_at`，没有消息时为会话的 `created_at`。
    pub last_activity_at: i64,
    /// 首条用户输入的首个文本片段开头；没有则 `None`。
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 还有更早的会话时给出，原样传回取下一页。
    pub next: Option<SessionCursor>,
}

/// 一次 agent loop 的状态。失败详情看调用记录的 `error`。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RunState {
    Executing,
    Completed,
    /// 模型调用不可重试或重试用尽。
    ProviderFailed,
    /// 轮次用尽：模型已做过不带工具的总结，但任务未必完成。
    MaxTurns,
    /// 进程停止时仍在执行，启动时收尾。
    Interrupted,
}

impl RunState {
    /// 库列值，也是对外（SSE）的状态名。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Executing => "executing",
            Self::Completed => "completed",
            Self::ProviderFailed => "provider_failed",
            Self::MaxTurns => "max_turns",
            Self::Interrupted => "interrupted",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Run {
    pub id: RunId,
    pub session_id: SessionId,
    pub state: RunState,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ContextWindow {
    /// 最近一次 `Compaction` 的摘要。
    pub summary: Option<String>,
    /// 最近一次 `Boundary` 之后的消息，按 id；包含 held 输入，排除仍待调度的输入。
    pub messages: Vec<Message>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingDelivery {
    pub message: Message,
    pub target: DeliveryTarget,
}

/// 一次模型调用的 token 用量；上游没报的项为 `None`。输入含缓存命中，输出含推理。
/// 费用不落盘：展示时按当前价格配置换算。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

/// 一次模型调用尝试（含失败与重试）。
#[derive(Debug, Clone, PartialEq)]
pub struct NewModelCall {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    /// 请求模型名。
    pub model: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub outcome: ModelCallOutcome,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelCallOutcome {
    /// `usage` 为 `None` = 上游没报。`blocks` 非空时写成一条 `Reply`。
    Replied {
        usage: Option<Usage>,
        blocks: Vec<ReplyBlock>,
    },
    Failed {
        error: String,
    },
}

/// 模块迁移。`module` 不得为 `"core"`（内核保留）；同一模块 `version` 从 1 连续递增。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub module: &'static str,
    pub version: u32,
    pub sql: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewSession {
    pub kind: SessionKind,
    pub parent_session_id: Option<SessionId>,
    pub delivery_target: Option<DeliveryTarget>,
    pub pwd: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
    /// `None` = 建会话时的默认人设。
    pub persona: Option<PersonaId>,
    /// `None` = 建会话时的默认模型（可能仍为空）。
    pub model: Option<ModelId>,
}

/// 入站输入的片段；图片是已验证的原件，由 `append_input` 同事务落库。
pub enum NewInputPart {
    Text(String),
    Image(mic_message::ImageData),
}

/// 入站输入落盘后的执行处置。
pub enum InputDisposition {
    /// 未认领，等调度或当前 run 并入。
    Pending,
    /// 落盘即 held：进入后续上下文，永不被认领。
    Held,
}

/// 随输入同事务写入、解释该输入处置的通知。
pub struct NewNotice {
    pub source: String,
    pub text: String,
}
