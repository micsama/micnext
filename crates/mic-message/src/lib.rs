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

/// 文件引用。`path` 相对 `<data_dir>/files`。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileRef {
    pub path: String,
    pub mime: String,
    pub size_bytes: u64,
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

/// 一次模型调用输出的一块，按服务商能给出的生成顺序（协议不区分先后时由 Provider 定序）。`ToolCall.args` 保持 `Value`：schema 属于各 Tool，
/// 解析发生在 `mic-tool` 边界（原文不是 JSON 对象时为 `Value::String`）。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ReplyBlock {
    Reasoning(Reasoning),
    Text {
        text: String,
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
    Compaction { summary: String },
    UserClear,
}

/// 消息闭集：一个变体 = 一个产出者的一次产出。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum MessageBody {
    /// 一次入站：文字与文件同条。
    UserInput {
        person: PersonId,
        parts: Vec<ContentPart>,
    },
    /// 一次模型调用的全部输出；`model` 为请求模型名，Provider 据此判断推理能否回传。
    Reply {
        model: String,
        blocks: Vec<ReplyBlock>,
    },
    ToolResult {
        tool_name: String,
        tool_call_id: String,
        outcome: ToolResultOutcome,
    },
    /// `wait=false` 任务的终态回报，靠 `exec_id` 关联闭合原 tool call 的 `Dispatched`。
    Completion {
        person: PersonId,
        tool_name: String,
        exec_id: String,
        outcome: ExecOutcome,
    },
    /// 给模型看的框架备注，不投递。
    HarnessNote { text: String },
    /// 投递给用户的框架通知。
    Notification { source: String, text: String },
    /// 上下文截断，不进模型上下文。
    Boundary { boundary: ContextBoundary },
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
