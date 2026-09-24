//! L0：Message/MessageAuthor 等核心类型。零内部依赖，只被依赖。
//! 纯数据，无 I/O、无业务逻辑，不定义 Error 枚举。
//! 契约：docs/blueprints/mic-message.md。

use serde::{Deserialize, Serialize};

mod model_view;

pub use model_view::ModelView;

/// ID 均由 `mic-store` 插入时用 SQLite rowid 回填。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub i64);

/// `Message` 与 `BoundaryEntry` 共享的严格顺序。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionEntryId(pub i64);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PersonId(pub i64);

/// `SessionEntry` 的作者/来源。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageAuthor {
    User {
        id: PersonId,
    },
    /// 产出这条内容的模型。Provider 据此判断推理签名能否回传（换模型后丢弃）。
    Assistant {
        model: String,
    },
    Tool {
        name: String,
    },
    /// 框架事实型标记：进历史供后续上下文读，不投递、不触发 Query。
    HarnessNote,
    /// 要投递给用户的通知：不触发 Query。
    Notification {
        source: String,
    },
}

/// 文件引用。`path` 相对 `<data_dir>/files`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub path: String,
    pub mime: String,
    pub size_bytes: u64,
}

/// 工具输出的片段：文本或文件（截图、图片等可回给多模态模型）。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContentPart {
    Text { text: String },
    File(FileRef),
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
/// `MessageContent::Completion` 送达。
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

/// 内容闭集。`ToolCall.args` 保持 `Value`：schema 属于各 Tool，解析发生在 `mic-tool` 边界。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageContent {
    Text {
        content: String,
    },
    Reasoning(Reasoning),
    ToolCall {
        id: String,
        name: String,
        args: serde_json::Value,
    },
    ToolResult {
        tool_call_id: String,
        outcome: ToolResultOutcome,
    },
    /// `wait=false` 任务的终态回报，靠 `exec_id` 关联闭合原 tool call 的 `Dispatched`。
    Completion {
        exec_id: String,
        outcome: ExecOutcome,
    },
    Attachment(FileRef),
}

/// 排序依据是 `id`；`created_at`（unix millis）是发生时间，只用于展示与诊断。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Message {
    pub id: SessionEntryId,
    pub session_id: SessionId,
    pub author: MessageAuthor,
    pub content: MessageContent,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}

/// 压缩/`/clear` 产生的上下文边界。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ContextBoundary {
    Compaction { summary: String },
    UserClear,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct BoundaryEntry {
    pub id: SessionEntryId,
    pub session_id: SessionId,
    pub boundary: ContextBoundary,
    pub created_at: i64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum SessionEntry {
    Message(Message),
    Boundary(BoundaryEntry),
}
