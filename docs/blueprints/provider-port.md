# B2: 模型 port（Provider trait、流式事件、用量、失败分类）

**状态**: CLOSED（2026-09-23 批准并实现于 `crates/mic-core/src/provider.rs`、`crates/mic-message/src/model_view.rs`，随步 3' 实跑验收）；2026-09-24 按 [`storage-restructure.md`](storage-restructure.md) 修订（`model()`、`blocks`、`Option<Usage>`、视图按 `MessageBody`）
**来源**: [`v0a-module-map.md`](v0a-module-map.md) M5；[`product-roadmap.md`](../brainstorm/product-roadmap.md)
§四-8（Provider 失败语义）；[`mic-message.md`](mic-message.md) §三（一条 `Reply` 即一个 assistant turn）
**依赖不变量**: 定义在 `mic-core`；`ToolSpec` 定义在 `mic-tool`（§三.1）；历史消息的模型视图定义在
`mic-message`（§三.5）。Provider 实现
crate（M7）依赖 `mic-core` + `mic-message` + `mic-store`（`Usage`）+ `mic-tool`。

本文只写现行契约；修订过程见 git 历史。

## 一、用户视角的效果

- 所有模型配在 `[models]` 下（如 DeepSeek 官方、某个 OpenAI 兼容服务），
  `[models] default` 指定当前用哪个；Web 设置页切模型就是改这个名字。写错 → 启动报错。
- 模型回复边生成边显示（文本与推理分开）；结束后留下完整回复、用量。
- 模型调用失败分四类，各自给出人能看懂的原因：账户问题（key 不对、余额不足）、请求被拒
  （如上下文超长）、暂时性故障（限流/网络/服务端错误，可重试）、协议不符（实现问题）。
  是否重试、重试几次、失败怎么呈现由执行主路径（M6）决定。

## 二、范围

本文定：`Provider` trait、请求/事件/响应类型、失败分类、`Registry::provider`。
`[models]` 的解析与装配规则在 mic-core-module B2 §四。
不定：重试策略、流中断后已显示内容的处理、用量落盘时机（M6）；具体协议、条目字段、预设（M7）。

## 三、公开类型与签名

### 3.1 `mic-tool`：模型可见的工具说明

```rust
/// 发给模型的工具定义；由各 Tool 自带（M3），core 只汇总后放进请求。
#[derive(Debug, Clone, PartialEq)]
pub struct ToolSpec {
    pub name: String,
    pub description: String,
    /// JSON Schema（object）。
    pub parameters: serde_json::Value,
}
```

放在 `mic-tool` 是因为生产者是 Tool（M3），而 `mic-tool` 不依赖 `mic-core`。M3 的 B2
沿用此类型，不另定义。

`parameters` 用 `Value` 不违反"边界收严"：它是本进程产出、发往模型的 JSON Schema，不是入站数据，
框架不解读其结构，只原样放进请求。

### 3.2 `mic-core`：Provider

```rust
pub type BoxStream<T> = Pin<Box<dyn futures_core::Stream<Item = T> + Send>>;

pub trait Provider: Send + Sync + 'static {
    /// 请求时发给上游的模型名；调用记录与推理回传判定用同一口径。
    fn model(&self) -> &str;
    /// 发起一次流式调用。连接失败也以流里的第一个 `Err` 返回。
    /// 丢弃流 = 取消调用（实现须随之中止 HTTP 请求）。
    /// 流以恰好一个 `Finished` 或一个 `Err` 结束，之后不再产出。
    fn stream(&self, req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>>;
}

pub struct ModelRequest {
    /// 系统提示；空串 = 不发。
    pub system: String,
    /// 本次上下文内的历史消息，按 core 组装的顺序（run-execution §4.4）。Provider 逐条经
    /// `Message::model_view`（§三.5）映射，`None` 跳过；一条 `Reply` 即一个 assistant turn。
    pub messages: Vec<mic_message::Message>,
    /// 空 = 不开启工具调用。
    pub tools: Vec<mic_tool::ToolSpec>,
}

pub enum ModelEvent {
    /// 回复正文增量，仅供实时显示。
    TextDelta(String),
    /// 可见推理增量，仅供实时显示。
    ReasoningDelta(String),
    /// 最终结果；增量的累积以此为准（一处真相）。
    Finished(ModelResponse),
}

pub struct ModelResponse {
    /// 按生成顺序：推理、正文、工具调用。
    pub blocks: Vec<mic_message::ReplyBlock>,
    pub stop: StopReason,
    /// 上游没报为 `None`。
    pub usage: Option<mic_store::Usage>,
}

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
```

### 3.3 失败分类

```rust
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
    Transient { message: String, retry_after: Option<std::time::Duration> },
    /// 响应不符合协议（解析失败、事件顺序错）：实现与上游不匹配，不重试。
    #[error("模型响应不符合协议：{message}")]
    Protocol { message: String },
}
```

分类只按"重试有没有用、该谁去修"一个轴划分；HTTP 状态码、上游错误体到四类的映射
由各实现在边界完成。

### 3.4 装配

```rust
impl Registry {
    /// 模型模块对收到的每个 `[models.<name>]` 条目登记一个实例，`name` 即条目名。
    pub fn provider(&mut self, name: impl Into<String>, p: impl Provider);
}
```

模型模块的 `install` 收到 `{条目名 → 条目}`，典型写法
`cfg.parse::<BTreeMap<String, Entry>>()`，`Entry` 照常 `deny_unknown_fields`。
条目名唯一由 TOML 保证；模块只登记收到的名字（内部互信）。选中的 Provider 如何交给
执行主路径随 M6 B2 定。

### 3.5 `mic-message`：消息的模型视图

框架消息（运行时备注、通知、后台任务完成）在请求里一律用 user role，首行方括号头标明来源，
模型据此区分真实用户和框架；方案沿用 micbot（`Message::wire_content`）。呈现规则只此一处，
所有 Provider 调用它，不各自拼接。

```rust
/// 一条历史消息在模型请求里的呈现。
pub enum ModelView<'a> {
    /// user role。首个片段是 `Text`，以方括号头开头。
    User(Vec<ContentPart>),
    /// 一条 `Reply` 即一个 assistant turn；`model` 为请求模型名。
    Assistant { model: &'a str, blocks: &'a [ReplyBlock] },
    /// 工具结果，对应 `tool_call_id` 的那次调用。
    Tool { tool_call_id: &'a str, output: Vec<ContentPart> },
}

impl Message {
    /// `Boundary` 不进上下文，返回 `None`。
    pub fn model_view(&self) -> Option<ModelView<'_>>;
}
```

| 消息 | 视图 | 首行头 |
|---|---|---|
| `UserInput` | `User` | `[user id=<id> at=<时间>]` |
| `HarnessNote` | `User` | `[runtime-note]` |
| `Notification{source}` | `User` | `[notification src=<source> at=<时间>]` |
| `Completion{exec_id, outcome}` | `User` | `[completion exec_id=<id>]`，后接 outcome |
| `Reply` | `Assistant` | — |
| `Boundary` | `None` | — |
| `ToolResult{Terminal(outcome)}` | `Tool` | outcome 为 `Completed` 时无头 |
| `ToolResult{Dispatched{exec_id}}` | `Tool` | `[dispatched exec_id=<id>]`，说明结果稍后以 completion 送达 |

- outcome：`Completed` 原样给出 `output`；`Failed{kind}` → `[failed kind=input|business|dependency]` + message；
  `Cancelled` → `[cancelled]` + message（分类定义见 [mic-tool](mic-tool.md) §三.1、§四.3）。
- 头的格式与说明由 `model_view.rs` 内私有 `Header` 唯一定义，`header_legend()` 对外提供说明
  （[`system-prompt-layout.md`](system-prompt-layout.md) §四）。
- `at=` 只给用户与通知：它们的发生时刻可能明显早于在历史中的位置（排队、后台任务）；其余消息
  位置即时序。格式为带本地时区偏移的 RFC 3339（秒精度）。
- 头部取值里的 `]` 与控制字符替换为 `_`；正文逐字追加，不转义。头部只是提示格式，可审计的来源
  以落盘的 `kind` 为准。
- `File` 片段原样进视图；Provider 按自身能力决定能否发送（多模态的准备）。
- 头部文本面向模型，用英文。

## 四、规则

- **内容支持**：视图里含实现不支持的片段（v0a 为 `File`）→ `Rejected`，不静默丢弃。
- **推理回传**：是否把 `Reasoning` 回传给上游由实现按 `ModelView::Assistant.model == self.model()`
  判断（换模型后不回传）；两端都是请求模型名，不受上游回报名的版本后缀干扰。
- **用量**：`usage` 只在 `Finished` 里给；整个没报为 `None`，报了时输入输出必有，其余上游不报即
  `None`，不编造 0。`input_tokens` 是本次全部输入
  （含缓存命中），`cache_read_tokens`/`cache_write_tokens` 是其中的缓存部分；`output_tokens`
  含推理，`reasoning_tokens` 是其中的推理部分。各实现在边界换算成这一口径。失败调用的用量全空。
- **超时**：连接与流空闲超时由实现在自己的配置里定，超时归 `Transient`。

## 五、副作用与依赖

- `Provider::stream` 发起网络请求；本文其余部分无副作用。
- `mic-core` 新增外部依赖 `futures-core`（仅 `Stream` trait）。
- `mic-tool` 新增外部依赖 `serde_json`（`ToolSpec.parameters`）。
- `mic-message` 新增外部依赖 `chrono`（`at=` 时间格式化）；`model_view` 无副作用。

## 六、调用方

| 调用方 | 用途 | 兼容性 |
|---|---|---|
| `mic-provider-openai`（M7） | `name() = "openai"`；实现 `Provider`，按收到的条目逐个登记 | 新 crate，M7 B2 |
| `mic-core` 装配 | `[models]` 解析、按 `kind` 分发、核对 `default` | mic-core-module B2 修订 |
| `mic-core` 执行主路径（M6） | 取 `default` 对应 Provider、组请求、转发增量为实时事件、`Finished` 落盘为调用行 + `Reply`、按分类重试 | [run-execution](run-execution.md) |
| `mic-tool` 各工具（M3/M8） | 产出 `ToolSpec` | 新契约，[mic-tool](mic-tool.md) 沿用 |
| `mic-message` | `ModelView`、`Message::model_view` | — |
| `bin/micnext` | 无改动（Provider 模块照常放进模块列表） | — |


## 七、已知演进

- 框架对模型的一次性请求（`HarnessRequest`，mic-message §2.2）需要进请求视图时，
  `ModelRequest` 加字段，随 M6 定。
- 采样参数（温度、最大输出长度、推理强度）先作为各实现的配置项，出现按次调整的
  真实需求再进 `ModelRequest`。
- 模型路由（fallback、按复杂度选小模型）：一种新 `kind`，自身实现 `Provider`，按条目名
  引用其它模型；需要的"按名取 Provider"接口届时随路由 B2 定。
- 模型能力探针（是否支持图片等多模态）：不支持时可配 OCR 类工具兜底，届时定能力
  查询接口；v0a 不支持的内容一律 `Rejected`。
