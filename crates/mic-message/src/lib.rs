//! L0：Message/MessageBody 等核心类型。零内部依赖，只被依赖。
//! 纯数据，无 I/O、无业务逻辑，不定义 Error 枚举。
//! 契约：docs/blueprints/mic-message.md。

use serde::{Deserialize, Serialize};

mod image;
pub mod limits;
mod model_view;

pub use image::{ImageData, ImageFormat, ImageId, ImageRef};
pub use model_view::{header_legend, ModelView};

/// ID 均由 `mic-store` 插入时用 SQLite rowid 回填。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub i64);

/// 全局序号，排序依据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MessageId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PersonId(pub i64);

/// 用户随输入附带、已由内核存入 `<data_dir>/files/<person>/` 的文件。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    /// 绝对路径。
    pub path: String,
    pub size_bytes: u64,
}

impl FileRef {
    /// 路径最后一段。
    pub fn name(&self) -> &str {
        self.path.rsplit(['/', '\\']).next().unwrap_or(&self.path)
    }
}

/// 消息里的片段：文本、文件引用或用户入站图片。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentPart {
    Text { text: String },
    File(FileRef),
    Image(ImageRef),
}

/// 工具失败按"谁改了才能成功"分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecFailureKind {
    /// 改参数就能成功：缺字段、类型错、未知字段、值越界、路径不存在、原文未匹配。
    Input,
    /// 参数合法但工具规则不允许：如目标是目录、文件非文本。
    Business,
    /// 外部环境出错，与参数无关：权限拒绝、磁盘/网络 I/O、子进程无法启动。
    Dependency,
}

impl ExecFailureKind {
    /// 呈现用的小写标签（模型视图的 `[failed kind=…]` 头、调试输出）。
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Input => "input",
            Self::Business => "business",
            Self::Dependency => "dependency",
        }
    }
}

/// 工具执行终态。`wait=true` 回填原 tool call 与 `wait=false` 的 completion 共用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecOutcome {
    TimedOut {
        output: Vec<ContentPart>,
    },
    Completed {
        output: Vec<ContentPart>,
    },
    Failed {
        kind: ExecFailureKind,
        message: String,
    },
    Cancelled {
        message: String,
    },
}

/// `wait=true` 得到 `Terminal`；`wait=false` 立即闭合于 `Dispatched`，终态稍后经
/// `MessageBody::Completion` 送达。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ToolResultOutcome {
    Terminal(ExecOutcome),
    Dispatched { exec_id: String },
}

/// 模型推理。上游字段收全：可见推理 + 可选签名；被上游加密的只有密文。
/// 是否回传给模型由 Provider 决定。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Reasoning {
    Visible {
        text: String,
        signature: Option<String>,
    },
    Redacted {
        data: String,
    },
}

/// assistant 正文的阶段：中间说明或最终回答。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AssistantPhase {
    Commentary,
    FinalAnswer,
}

/// 一次模型调用输出的一块，按服务商能给出的生成顺序（协议不区分先后时由 Provider 定序）。`ToolCall.args` 保持 `Value`：schema 属于各 Tool，
/// 解析发生在 `mic-tool` 边界（原文不是 JSON 对象时为 `Value::String`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplyBlock {
    Reasoning(Reasoning),
    Text {
        text: String,
        /// 上游给出的阶段标注，续轮原样回传；无此概念的协议为 `None`。
        phase: Option<AssistantPhase>,
    },
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
}

/// 压缩/`/clear` 产生的上下文边界。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextBoundary {
    Compaction(Compaction),
    UserClear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum SessionCommand {
    Compact { text: Option<String> },
    Clear,
    RevokeCompaction { boundary_id: MessageId },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Compaction {
    pub summary: String,
    pub keep_from: Option<MessageId>,
    pub files: Vec<String>,
    /// 保留段里换成占位的工具结果（`ToolResult` / `Completion`）。
    #[serde(default)]
    pub elided: Vec<MessageId>,
    /// 压掉范围内带回原文的用户输入。
    #[serde(default)]
    pub quoted: Vec<MessageId>,
}

/// 产出回复的单次模型请求事实，不进入模型上下文。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplyStats {
    pub model_ms: i64,
    /// 首个输出增量距请求开始的毫秒数；非流式或无输出时为空。
    pub first_output_ms: Option<i64>,
    pub usage: Option<ReplyUsage>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ReplyUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
}

/// 消息闭集：一个变体 = 一个产出者的一次产出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MessageBody {
    Command {
        person: PersonId,
        command: SessionCommand,
    },
    /// 一次入站：文字与文件同条。
    UserInput {
        person: PersonId,
        parts: Vec<ContentPart>,
    },
    /// 一次模型调用的全部输出；`model` 为请求模型名，Provider 据此判断推理能否回传。
    Reply {
        stats: ReplyStats,
        model: String,
        blocks: Vec<ReplyBlock>,
    },
    ToolResult {
        tool_name: String,
        tool_call_id: String,
        outcome: ToolResultOutcome,
        /// 工具开始执行的时刻（Unix 毫秒）；恢复补的 `Cancelled` 无。
        started_at: Option<i64>,
    },
    /// `wait=false` 任务的终态回报，靠 `exec_id` 关联闭合原 tool call 的 `Dispatched`。
    Completion {
        person: PersonId,
        tool_name: String,
        exec_id: String,
        outcome: ExecOutcome,
    },
    /// 运行中的后台任务给发起会话的中途通知；是 run 输入，不投递。字段同该任务的 `Completion`。
    JobNotice {
        person: PersonId,
        tool_name: String,
        exec_id: String,
        text: String,
    },
    /// 定时任务到点发给 bot 的指令；无发起人，独占一个 run。
    Trigger { source: String, text: String },
    /// 给模型看的框架备注，不投递。
    HarnessNote { text: String },
    /// 投递给用户的框架通知。
    Notification { source: String, text: String },
    /// 上下文截断，不进模型上下文。
    Boundary { boundary: ContextBoundary },
    /// 旁路调用的模型原文，只供 trace；不进上下文与聊天。
    SideOutput { purpose: SidePurpose, text: String },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SidePurpose {
    Title,
}

/// 排序依据是 `id`；`created_at`（unix millis）是发生时间，只用于展示与诊断。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: MessageId,
    pub session_id: SessionId,
    pub body: MessageBody,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}
