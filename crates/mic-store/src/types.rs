use std::collections::BTreeSet;
use std::num::NonZeroU32;

use mic_message::{
    Compaction, Message, MessageId, PersonId, ReplyBlock, SessionCommand, SessionId,
};
use serde::{Deserialize, Serialize};

use crate::SecretValue;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct RunId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct ModelCallId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct PersonaId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ModelId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ReasoningEffort {
    None,
    Minimal,
    Low,
    Medium,
    High,
    Xhigh,
    Max,
}

/// 一次标题调用：取材消息在 `call.context.instruction`；`output` 为模型原文，上游失败时无。
#[derive(Debug, Clone, PartialEq)]
pub struct TitleCall {
    pub call: NewModelCall,
    pub output: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct TitleModelSelection {
    pub model_id: Option<ModelId>,
    pub reasoning_effort: Option<ReasoningEffort>,
}

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

/// 单价，美元 / 百万 token；`None` = 未填，`Some(0.0)` = 免费。
#[derive(Debug, Clone, Copy, PartialEq, Default, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ModelPrice {
    pub input: Option<f64>,
    pub cache_read: Option<f64>,
    pub cache_write: Option<f64>,
    pub output: Option<f64>,
}

impl ModelPrice {
    pub(crate) fn is_valid(&self) -> bool {
        [self.input, self.cache_read, self.cache_write, self.output]
            .into_iter()
            .flatten()
            .all(|v| v.is_finite() && v >= 0.0)
    }
}

/// 服务商下的一个模型：模型名 + 该 kind 自己解析的参数。
#[derive(Debug, Clone, PartialEq)]
pub struct ModelView {
    pub context_window: Option<NonZeroU32>,
    /// 关闭时请求里的图片换成文字占位。
    pub images: bool,
    pub price: ModelPrice,
    pub id: ModelId,
    pub endpoint_id: EndpointId,
    pub name: String,
    pub config_json: String,
}

pub struct ModelWrite {
    pub context_window: Option<NonZeroU32>,
    pub images: bool,
    pub price: ModelPrice,
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
    Selected(SelectedModel),
}

pub struct SelectedModel {
    pub context_window: Option<NonZeroU32>,
    pub images: bool,
    pub id: ModelId,
    /// 「服务商 / 模型」。
    pub name: String,
    pub kind: String,
    pub endpoint_json: String,
    pub model_name: String,
    pub model_json: String,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub key: Option<SecretValue>,
}

pub struct ClaimedRun {
    pub input: RunInput,
    pub run: Run,
    pub settings: RunSettings,
    pub model: ClaimedModel,
}

pub enum RunInput {
    Conversation {
        message_ids: Vec<MessageId>,
    },
    Command {
        message_id: MessageId,
        command: SessionCommand,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct CompactionSettings {
    pub trigger_percent: u8,
    /// 压缩时保留原文占当前历史的百分比；最新一步总是保留。
    pub keep_percent: u8,
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
    pub compaction: CompactionSettings,
    /// 始终是未删除的人设。
    pub default_persona: PersonaId,
    /// 对所有人设生效；空串 = 无。
    pub general_prompt: String,
    /// 新 Web 会话的工作目录，用户写法：绝对路径或 `~/` 开头。
    pub default_workdir: String,
    pub max_turns: u32,
}

/// 认领时定下的本轮设置，执行期间不变。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunSettings {
    pub reasoning_effort: Option<ReasoningEffort>,
    pub compaction: CompactionSettings,
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

/// 发送目标：`channel` 决定由哪个 Channel 投递；`payload` 是该 Channel 自己编解码的不透明数据。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeliveryTarget {
    pub channel: String,
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
    Task,
    /// 模块触发（cron、webhook…）。`ref_id` 由该模块解释，内核只当来源元数据。
    Triggered { module: String, ref_id: String },
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    pub archived_at: Option<i64>,
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
    /// `None` = 使用模型默认档位；`Some(ReasoningEffort::None)` = 显式关闭思考。
    pub reasoning_effort: Option<ReasoningEffort>,
    /// 临时明文：id 大于它的消息发给模型时不按规则脱敏；`None` = 全部脱敏。
    pub plain_after: Option<MessageId>,
}

/// 会话列表分页游标：上一页最后一项的排序键。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCursor {
    pub last_activity_at: i64,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub title: Option<String>,
    pub title_error: Option<String>,
    pub session: Session,
    /// 最新消息的 `created_at`，没有消息时为会话的 `created_at`。
    pub last_activity_at: i64,
    /// 首条用户输入的首个文本片段开头；没有则 `None`。
    pub preview: Option<String>,
    pub pinned_at: Option<i64>,
    /// Web Root 有晚于已读水位的 `Reply`/`Notification`；其它会话恒为 false。
    pub unread: bool,
    /// 自身有 `Executing` 的 run。
    pub running: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 还有更早的会话时给出，原样传回取下一页。
    pub next: Option<SessionCursor>,
}

/// 会话消息的一页，规则见 docs/blueprints/web-message-paging.md §二。
#[derive(Debug, Clone, PartialEq)]
pub struct MessagePage {
    /// id 升序。
    pub messages: Vec<Message>,
    /// 本页之前还有消息。
    pub has_earlier: bool,
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
    /// 用户主动停止。
    Stopped,
    /// 子 run 或任务会话的 run 超过墙钟上限被取消。
    TimedOut,
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
            Self::Stopped => "stopped",
            Self::TimedOut => "timed_out",
        }
    }
}

/// 启动收尾的一个遗留 run；`report` 为子 run 给父 session 的交付。
pub struct InterruptedRun {
    pub run: Run,
    pub closing: Vec<mic_message::MessageBody>,
    pub report: Option<mic_message::ExecOutcome>,
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
    pub boundary_id: Option<MessageId>,
    pub files: Vec<String>,
    /// 范围内、`messages` 末条之前因未认领且非 held 而排除的输入（held 输入留在 `messages`）。
    pub filtered: Vec<MessageId>,
    /// 当前有效压缩边界的摘要。
    pub summary: Option<String>,
    /// 当前有效历史与边界保留的输入，按 id；包含 held 输入，排除仍待调度的输入。
    /// 边界 `elided` 的工具结果已换成占位。
    pub messages: Vec<Message>,
    /// 当前有效压缩边界带回的用户原话，按 id。
    pub quoted: Vec<Message>,
    /// 读窗口时会话的临时明文水位，见 `Session::plain_after`。
    pub plain_after: Option<MessageId>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum CompactionState {
    Applied,
    Revoked { at: i64 },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct CompactionView {
    pub boundary_id: MessageId,
    pub result: Compaction,
    pub state: CompactionState,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct ContextView {
    pub active_boundary: Option<MessageId>,
    pub compactions: Vec<CompactionView>,
}

/// 调用时模型输入的描述；重建只靠这些事实，见 docs/blueprints/run-trace.md §五。
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ModelCallContext {
    pub boundary_id: Option<MessageId>,
    pub through: Option<MessageId>,
    /// 预算裁剪丢掉的窗口前段条数。
    pub skip: u32,
    /// 调用时被排除出窗口的输入（未认领且非 held；压缩另含本步待处理输入）。
    pub excluded: Vec<MessageId>,
    /// 压缩附加文字的来源消息。
    pub instruction: Vec<MessageId>,
    /// 压缩：保留段起点。
    pub keep_from: Option<MessageId>,
    /// 压缩：本次新增替换、要求摘要提炼的工具结果。
    pub elide: Vec<MessageId>,
}

/// 当前有效边界下最近一次成功对话调用的上游用量。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextUsage {
    pub call: ModelCallId,
    pub input_tokens: u64,
    pub output_tokens: u64,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PendingDelivery {
    pub message: Message,
    pub target: DeliveryTarget,
}

/// 一次模型调用的 token 用量。`Some(0)` = Provider 确认不适用或为零，`None` = 上游没报。
/// 输入含缓存命中，输出含推理。费用不落盘：展示时按当前价格配置换算。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
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
    /// 请求读取上下文的时刻；重试共用，用作回复的位置。
    pub context_at: i64,
    pub purpose: ModelCallPurpose,
    pub context: ModelCallContext,
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    /// 请求模型名。
    pub model: String,
    pub model_id: Option<ModelId>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub started_at: i64,
    /// 首个非空正文、推理或工具参数增量的时刻。
    pub first_output_at: Option<i64>,
    pub finished_at: i64,
    pub stop: Option<StopReason>,
    /// 成败独立：失败的调用也可能带已收到的用量。
    pub usage: Option<Usage>,
    pub outcome: ModelCallOutcome,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum StopReason {
    /// 正常说完。
    EndTurn,
    /// 要调用工具（`blocks` 含 `ToolCall`）。
    ToolUse,
    /// 撞到输出长度上限，回复不完整。
    MaxTokens,
    /// 被上游内容审核截断。
    ContentFilter,
    /// 被用户停止：`blocks` 只含已收到的正文。
    Stopped,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum ModelCallPurpose {
    Conversation,
    Compaction,
    Title,
}

#[derive(Debug, Clone, PartialEq)]
pub enum ModelCallOutcome {
    /// `blocks` 非空时写成一条 `Reply`。
    Replied {
        blocks: Vec<ReplyBlock>,
    },
    Failed {
        error: String,
    },
}

/// 模块迁移。`module` 不得为 `"core"`（内核保留）；同一模块 `version` 从基线起连续递增，首条即基线。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Migration {
    pub module: &'static str,
    pub version: u32,
    pub sql: &'static str,
    /// 本次迁移引入的明文凭据列；诊断查询读作 NULL。
    pub secret_columns: &'static [SecretColumn],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SecretColumn {
    pub table: &'static str,
    pub column: &'static str,
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
    pub reasoning_effort: Option<ReasoningEffort>,
}

/// 入站输入的片段；图片是已验证的原件，由 `append_input` 同事务落库；文件已由内核落盘。
pub enum NewInputPart {
    Text(String),
    Image(mic_message::ImageData),
    File(mic_message::FileRef),
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
