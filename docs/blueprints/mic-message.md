# B2: mic-message

**状态**: CLOSED（2026-09-23 批准并实现于 `crates/mic-message/src/lib.rs`）；`ExecFailureKind` 与 `ExecOutcome::Failed.kind`（见 [mic-tool](mic-tool.md) §三.1）、`model_view`（见 provider-port §三.5）修订已批准并实现；2026-09-24 按 [`storage-restructure.md`](storage-restructure.md) 改为 `MessageBody` 闭集
**来源**: [`mic-message-types.md`](../brainstorm/mic-message-types.md)（B1）、
[`product-roadmap.md`](../brainstorm/product-roadmap.md) §二
**依赖不变量**: L0——零内部依赖，只被依赖。

本文只写现行契约；修订过程见 git 历史。

## 一、crate 概览

Session 内消息的核心数据类型。纯数据：无 I/O、无业务逻辑、
无可失败操作，不定义 Error 枚举。外部依赖仅 `serde`（derive）+ `serde_json`
（`ReplyBlock::ToolCall.args` 的 `Value`）。

## 二、公开类型

### 2.1 ID

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub i64);
/// 全局序号，排序依据。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct MessageId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PersonId(pub i64);
```

均由 `mic-store` 插入时用 SQLite rowid 回填。`RunId`、`ModelCallId` 在 `mic-store`：`run_id`、
`model_call_id` 是库内列，不进 `Message`。

### 2.2 片段与结果

```rust
/// 文件引用。`path` 相对 `<data_dir>/files`。
pub struct FileRef { pub path: String, pub mime: String, pub size_bytes: u64 }

/// 输入与工具输出的片段：文本或文件（截图、图片等可回给多模态模型）。
pub enum ContentPart {
    Text { text: String },
    File(FileRef),
}

/// 工具执行终态。`wait=true` 回填原调用与 `wait=false` 的 completion 共用。
pub enum ExecOutcome {
    Completed { output: Vec<ContentPart> },
    Failed { kind: ExecFailureKind, message: String },
    Cancelled { message: String },
}

/// `wait=true` 得到 `Terminal`；`wait=false` 立即闭合于 `Dispatched`，终态稍后经
/// `MessageBody::Completion` 送达。
pub enum ToolResultOutcome {
    Terminal(ExecOutcome),
    Dispatched { exec_id: String },
}

/// 模型推理。上游字段收全：可见推理 + 可选签名；被上游加密的只有密文。
/// 是否回传给模型由 Provider 决定。
pub enum Reasoning {
    Visible { text: String, signature: Option<String> },
    Redacted { data: String },
}

/// 一次模型调用输出的一块。`args` 保持 `Value`：schema 属于各 Tool，解析在 `mic-tool` 边界
/// （原文不是 JSON 对象时为 `Value::String`）。
pub enum ReplyBlock {
    Reasoning(Reasoning),
    Text { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
}

pub enum ContextBoundary {
    Compaction { summary: String },
    UserClear,
}
```

`ExecFailureKind` 见 [mic-tool](mic-tool.md) §三.1。

### 2.3 消息

```rust
/// 排序依据是 `id`；`created_at`（unix 毫秒）只用于展示与诊断。
pub struct Message {
    pub id: MessageId,
    pub session_id: SessionId,
    pub body: MessageBody,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}

/// 一条消息 = 一个产出者的一次产出；种类由变体唯一决定。serde 内部标签 `kind`。
#[serde(tag = "kind")]
pub enum MessageBody {
    /// 一次入站：文字与文件同条。
    UserInput { person: PersonId, parts: Vec<ContentPart> },
    /// 一次模型调用的全部输出，按生成顺序；`model` 为请求模型名。
    Reply { model: String, blocks: Vec<ReplyBlock> },
    ToolResult { tool_name: String, tool_call_id: String, outcome: ToolResultOutcome },
    /// `wait=false` 任务的终态回报；`person` 来自执行实例句柄。
    Completion { person: PersonId, tool_name: String, exec_id: String, outcome: ExecOutcome },
    /// 给模型看的框架备注，不投递。
    HarnessNote { text: String },
    /// 投递给用户的框架通知。
    Notification { source: String, text: String },
    /// 上下文截断，不进模型上下文。
    Boundary { boundary: ContextBoundary },
}
```

含 `serde_json::Value` 的类型只能 `PartialEq`。`HarnessRequest`（框架对模型的一次性请求）只活在
单次模型请求视图、不落盘，由 `mic-core` 自己定义，不进本类型。

## 三、跨模块语义（由消费方执行，本 crate 只定义类型）

| 种类 | 认领（触发 run） | 可靠投递 | 进模型上下文 |
|---|---|---|---|
| `UserInput`、`Completion` | 是 | 否 | 认领后 |
| `Reply`、`Notification` | 否 | 是 | 是 |
| `ToolResult`、`HarnessNote` | 否 | 否 | 是 |
| `Boundary` | 否 | 否 | 否（截断点） |

消息如何呈现给模型（`Message::model_view() -> Option<ModelView>`、方括号头）见
[`provider-port.md`](provider-port.md) §三.5。一条 `Reply` 即一个 assistant turn。

## 四、演进

- 新增枚举变体对已落盘数据兼容（旧数据不含新变体）。
- 给已有 struct variant 加字段**不兼容**：serde derive 默认要求字段存在，需
  `#[serde(default)]` 或迁移。
- 已知的增量方向：人工审批的请求/决定内容（roadmap §四-6，待定）。

## 五、副作用与依赖

无副作用。`mic-message ← (无)`。

## 六、调用方

| 调用方 | 用途 |
|---|---|
| `mic-store` | `MessageBody` 整条 JSON 落盘/读出；`kind`、`person` 由生成列投影 |
| `mic-tool` | 引用 ID newtype；自己的 `ToolOutcome`/`ToolError` 不依赖本 crate 的结果类型 |
| `mic-core` | 把工具执行结果转换成 `ToolResultOutcome`/`ExecOutcome` 后构造 `MessageBody`；Provider 在消息与模型请求格式之间转换 |
| `mic-gateway` / Channel | 读 `Message` 渲染与投递 |
