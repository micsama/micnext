# B2: mic-message

**状态**: CLOSED（2026-09-23 批准并实现于 `crates/mic-message/src/lib.rs`）；`ExecFailureKind` 与 `ExecOutcome::Failed.kind`（见 [mic-tool](mic-tool.md) §三.1）、`model_view`（见 provider-port §三.5）修订已批准并实现
**来源**: [`mic-message-types.md`](../brainstorm/mic-message-types.md)（B1）、
[`product-roadmap.md`](../brainstorm/product-roadmap.md) §二
**依赖不变量**: L0——零内部依赖，只被依赖。

本文只写现行契约；修订过程见 git 历史。

## 一、crate 概览

Session 内条目（消息与上下文边界）的核心数据类型。纯数据：无 I/O、无业务逻辑、
无可失败操作，不定义 Error 枚举。外部依赖仅 `serde`（derive）+ `serde_json`
（`ToolCall.args` 的 `Value`）。

## 二、公开类型

### 2.1 ID

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct SessionEntryId(pub i64);
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct PersonId(pub i64);
```

均由 `mic-store` 插入时用 SQLite rowid 回填。`SessionEntryId` 是 `Message` 与
`BoundaryEntry` 共享的严格顺序。`QueryId` 不在这里——`Message` 不归属 Query。

### 2.2 作者

```rust
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum MessageAuthor {
    User { id: PersonId },
    /// 产出这条内容的模型。Provider 据此判断推理签名能否回传（换模型后丢弃）。
    Assistant { model: String },
    Tool { name: String },
    /// 框架事实型标记：进历史供后续上下文读，不投递、不触发 Query。
    HarnessNote,
    /// 要投递给用户的通知：不触发 Query。
    Notification { source: String },
}
```

`HarnessRequest`（框架对模型的一次性请求）只活在单次模型请求视图、不落盘，
由 `mic-core` 自己定义，不进本类型。

### 2.3 内容

```rust
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

/// 工具执行终态。`wait=true` 回填原 tool call 与 `wait=false` 的 completion 共用。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecOutcome {
    Completed { output: Vec<ContentPart> },
    Failed { message: String },
    Cancelled { message: String },
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
    Visible { text: String, signature: Option<String> },
    Redacted { data: String },
}

/// 内容闭集。`ToolCall.args` 保持 `Value`：schema 属于各 Tool，解析发生在 `mic-tool` 边界。
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum MessageContent {
    Text { content: String },
    Reasoning(Reasoning),
    ToolCall { id: String, name: String, args: serde_json::Value },
    ToolResult { tool_call_id: String, outcome: ToolResultOutcome },
    /// `wait=false` 任务的终态回报，靠 `exec_id` 关联闭合原 tool call 的 `Dispatched`。
    Completion { exec_id: String, outcome: ExecOutcome },
    Attachment(FileRef),
}
```

`MessageContent` 及包含它的类型只能 `PartialEq`（`serde_json::Value` 不实现 `Eq`）。

### 2.4 条目

```rust
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
```

`Message` 不含 `query_id`：claim 关系只由 Query 一侧记录。`BoundaryEntry` 不含
author：boundary 不参与 claim 与投递。

## 三、跨模块语义（由消费方执行，本 crate 只定义类型）

| 内容 | 触发新 Query（claim） | 可靠投递 |
|---|---|---|
| `User` + `Text`/`Attachment` | 是 | 否 |
| `Completion` | 是 | 否 |
| `Assistant`/`Notification` + `Text`/`Attachment` | 否 | 是 |
| `Reasoning`、`ToolCall`、`ToolResult` | 否 | 否 |
| `HarnessNote` | 否 | 否 |
| `BoundaryEntry` | 否 | 否 |

`author` 与 `content` 的合法组合不做类型层或运行时校验，由唯一写者的构造路径保证。

条目如何呈现给模型（`ModelView`、方括号头）见 [`provider-port.md`](provider-port.md) §三.5。

**一次模型响应的落盘形状**：拆成若干条连续的 `author=Assistant` entry（`Reasoning`
→ `Text` → `ToolCall`…）。Provider 构建请求时把它们重组为一个 assistant turn；
两次响应之间必然隔着 `ToolResult` 或新 Query 的输入，边界无歧义。

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
| `mic-store` | 整条 JSON 落盘/读出；判别列从类型投影 |
| `mic-tool` | 引用 ID newtype；自己的 `ToolOutcome`/`ToolError` 不依赖本 crate 的结果类型 |
| `mic-core` | 把工具执行结果转换成 `ToolResultOutcome`/`ExecOutcome` 后构造 `Message`；Provider 在消息与模型请求格式之间转换 |
| `mic-gateway` / Channel | 读 `SessionEntry` 渲染与投递 |

当前无落盘数据、无下游代码，改动不需要 parallel change。
