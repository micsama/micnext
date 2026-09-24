# B2: 工具 port（Tool trait、参数边界、结果与失败分类）

**状态**: CLOSED（2026-09-23；`crates/mic-tool`、`mic-message`（`ExecFailureKind`）、`mic-core`（`Registry::tool`），§八验收随 M8 通过）
**来源**: [`v0a-module-map.md`](v0a-module-map.md) M3；[`product-roadmap.md`](../brainstorm/product-roadmap.md) §2.3；
[`next-gen-architecture.md`](../brainstorm/next-gen-architecture.md) §六（沿用 micbot `Tool` 形状与 `ToolError` 三分类）；
[`provider-port.md`](provider-port.md) §三.1（`ToolSpec`）、§三.5（模型视图）
**依赖不变量**: 定义在 `mic-tool`，只依赖 `mic-message`；工具实现 crate（M8）依赖 `mic-core`（登记）+ `mic-tool`。
本文同时修订 `mic-message`（`ExecOutcome::Failed` 加分类）与 `mic-core`（`Registry::tool`），合成一份。

本文只写现行契约；修订过程见 git 历史。

## 一、用户视角的效果

- 工具失败时模型能看出该谁改：参数写错（改参数重试）、工具规则不允许（换做法）、外部环境出错
  （稍后或换环境）。三类原样落盘，事后可查。
- 模型调用时多写、写错参数（如 v0 的 `bash` 传 `run_in_background`）→ 立即得到参数错误并自行纠正，
  不会被静默忽略。
- 每个工具的一句话简介、各参数说明、使用提示都由该工具自带；工具没启用时，它的提示也不出现在
  system prompt 里（与 DSH 相同）。
- 两个模块登记了同名工具 → 启动报错，指明是哪两个模块。

## 二、范围

本文定：`Tool` trait、参数解析与 schema 生成、`ToolContext`、`ToolError`、类型擦除后的 `ToolHandle`、
失败分类的落盘形状与模型视图、`Registry::tool`。

不定：
- 具体工具的参数、描述、提示、结果形状与尺寸上限（M8，以 DSH `@deepseek-ai/dsh-tool-*` 0.1.5-rc.3 为底稿）。
- 未知工具名、并行调用、工具 panic、按 `tool_scope` 筛选、提示拼接进 system prompt 的具体位置（M6）。
- 后台执行（`Dispatched`）：v2 随 job 运行时加入，v0a 没有生产者，本文不预留（§七）。
- 人工审批（HITL）：以后作为工具执行前的独立 port，不进本 trait（roadmap §2.3）。

## 三、公开类型与签名

### 3.1 `mic-message`：失败分类（修订）

```rust
/// 工具失败按"谁改了才能成功"分类。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum ExecFailureKind {
    /// 改参数就能成功：缺字段、类型错、未知字段、值越界、路径不存在、`edit` 原文未匹配。
    Input,
    /// 参数合法但工具规则不允许：如目标是目录、文件非文本。
    Business,
    /// 外部环境出错，与参数无关：权限拒绝、磁盘/网络 I/O、子进程无法启动。
    Dependency,
}

impl ExecFailureKind {
    /// 呈现用的小写标签：`input` / `business` / `dependency`（模型视图头、`-p` 输出共用）。
    pub fn as_str(self) -> &'static str;
}

pub enum ExecOutcome {
    Completed { output: Vec<ContentPart> },
    Failed { kind: ExecFailureKind, message: String },   // 新增 kind
    Cancelled { message: String },
}
```

命令本身跑失败（`bash` 退出码非 0）不是工具失败：工具已如实完成，退出状态作为输出的一部分
`Completed`（M8 定格式）。

### 3.2 `mic-tool`：Tool

```rust
pub trait Tool: Send + Sync + 'static {
    /// 模型参数在边界一次 parse 成此类型；schema 也由它生成（一处真相，§四.1）。
    /// 必须 `#[serde(deny_unknown_fields)]`；兼容 DSH 的 bool 等参数在这里直接解析成强类型。
    type Args: serde::de::DeserializeOwned + schemars::JsonSchema + Send;

    /// 模型可见的工具名，全局唯一。
    fn name(&self) -> &str;
    /// 工具定义里的一句话简介。
    fn description(&self) -> &str;
    /// 使用提示；工具在本次请求中可用时拼入 system prompt。`None` = 无提示。
    fn prompt_hint(&self) -> Option<&str>;

    /// 丢弃返回的 future = 取消：实现须随之释放资源（如杀掉子进程组），不得留下后台副作用。
    fn execute(
        &self,
        args: Self::Args,
        ctx: &ToolContext,
    ) -> impl Future<Output = Result<Vec<mic_message::ContentPart>, ToolError>> + Send;
}
```

`description`、`prompt_hint` 返回 `&str` 而非 `&'static str`：文本可随工具配置生成（如 `read` 的行数上限），
在工具构造时算好。

### 3.3 `mic-tool`：执行上下文与错误

```rust
/// 单次调用的执行上下文，由 core 构造。
pub struct ToolContext { /* 私有 */ }
impl ToolContext {
    /// `cwd` 为绝对路径（会话 `pwd`），由 core 保证。
    pub fn new(cwd: PathBuf) -> Self;
    /// 相对路径的解析基准、`bash` 的默认工作目录。
    pub fn cwd(&self) -> &Path;
}

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub struct ToolError {
    pub kind: mic_message::ExecFailureKind,
    /// 给模型看，用英文。
    pub message: String,
}
impl ToolError {
    pub fn input(message: impl Into<String>) -> Self;
    pub fn business(message: impl Into<String>) -> Self;
    pub fn dependency(message: impl Into<String>) -> Self;
}
```

分类只有 `ExecFailureKind` 一处定义，`ToolError` 直接复用，不在 mic-tool 另立枚举。

### 3.4 `mic-tool`：类型擦除后的句柄

core 需要把不同 `Args` 的工具放进同一个集合。擦除在 crate 内部完成，对外只有一个不透明句柄，
不再有 micbot 公开的 `DynTool` 第二契约（它唯一的消费者 MCP 已砍）。

```rust
#[derive(Clone)]
pub struct ToolHandle { /* Arc<私有擦除 trait> */ }
impl ToolHandle {
    /// 生成并缓存 `ToolSpec`（§四.1）。
    pub fn new(tool: impl Tool) -> Self;
    pub fn name(&self) -> &str;
    pub fn spec(&self) -> &ToolSpec;
    pub fn prompt_hint(&self) -> Option<&str>;
    /// 模型输出边界：`args` 解析失败 → `ToolError::input`，不调用 `execute`。
    /// 丢弃 future = 取消，语义同 `Tool::execute`。
    pub fn invoke(
        &self,
        args: &serde_json::Value,
        ctx: &ToolContext,
    ) -> impl Future<Output = Result<Vec<mic_message::ContentPart>, ToolError>> + Send;
}
```

`invoke` 不返回 `ExecOutcome`：它只可能成功或失败，取消由调用方丢弃 future 得到，转成
`Cancelled` 是 M6 的事。

### 3.5 `mic-core`：登记（修订 mic-core-module §三）

```rust
impl Registry {
    /// 工具模块在 `install` 中登记；一个模块可登记多个工具。
    pub fn tool(&mut self, t: impl mic_tool::Tool);
}

pub enum AssembleError {
    // …既有变体
    #[error("工具 `{name}` 被模块 `{first}` 和 `{second}` 重复登记")]
    DuplicateTool { name: String, first: &'static str, second: &'static str },
}
```

工具顺序 = 装配根里的模块顺序，模块内按登记顺序。请求里的 `tools` 列表和提示拼接都按这个顺序，
保证每次请求前缀一致（利于上游缓存）。

## 四、规则

### 4.1 Schema 生成

- `ToolSpec.parameters` 由 `Args` 经 `schemars` 生成，mic-tool 内部固定一套生成设置，所有工具共用：
  子 schema 内联（无 `$ref`/`$defs`），去掉 `$schema` 与 `title`，`Option` 字段只表现为非必填、不加 `null` 类型，
  `deny_unknown_fields` 生成 `additionalProperties: false`。
- 参数说明写在 `Args` 字段的 doc comment 上，由 schemars 带进 `description`；不另写一份。
- 与 DSH 的差异：DSH 把整数参数声明为 `number` 再在运行时校验，schemars 生成 `integer`，更严格；
  roadmap §2.3 要求对齐的是名称、必填项和默认值，不受影响。

### 4.2 参数边界

- `ToolHandle::invoke` 是模型输出进入工具的唯一入口，一次 parse 成 `Args`；`execute` 内部视参数类型已合法，
  只做 schema 表达不了的值校验（如 `limit` 上限），失败报 `ToolError::input`。
- `args` 不是 JSON 对象（包括 provider-openai §四.3 中原样保留的字符串）→ `input`，message 说明参数必须是
  JSON 对象；否则 message 为 serde 错误原文（含字段名，如 `unknown field \`run_in_background\``）。

### 4.3 模型视图（修订 provider-port §三.5）

| outcome | 首行头 |
|---|---|
| `Completed` | 无头，原样给出 `output` |
| `Failed { kind, .. }` | `[failed kind=input]` / `[failed kind=business]` / `[failed kind=dependency]`，后接 message |
| `Cancelled` | `[cancelled]`，后接 message |

与 `[completion exec_id=<id>]` 的键值写法一致。

### 4.4 取消

- 取消 = 调用方丢弃 `invoke` 返回的 future。工具不接收取消令牌，也不感知取消原因；原因由 M6 写进
  `ExecOutcome::Cancelled`。
- 有外部副作用的工具必须 cancel-safe：丢弃时同步终止子进程等资源（M8 的 `bash` 用 `kill_on_drop` +
  进程组）。已写入的文件不回滚。

## 五、副作用与依赖

- `mic-tool` 本身无副作用；副作用全在各工具的 `execute`。
- `mic-tool` 新增外部依赖：`serde`、`serde_json`（provider-port 已列）、`schemars` 1.x、`thiserror`。
- `mic-message` 无新依赖。
- 不需要 `async-trait`：`Tool::execute` 用 trait 内 `impl Future`，擦除层内部手写 boxed future。

## 六、调用方

| 调用方 | 用途 | 兼容性 |
|---|---|---|
| `mic-message` | 新增 `ExecFailureKind`；`ExecOutcome::Failed` 加 `kind` | 破坏性改动，但尚无落盘数据、无生产者，直接改，不需要 parallel change |
| `mic-message` `model_view`（provider-port §三.5） | `Failed` 的头改为 §四.3 | provider-port 尚未批准，直接改其正文 |
| `mic-store` | `CompletionInput.outcome` 与 JSON 列整体透传 `ExecOutcome` | 无代码改动 |
| `mic-core` 装配 | `Registry::tool`、`AssembleError::DuplicateTool`，汇总为 `Vec<ToolHandle>` | 修订 mic-core-module B2 |
| `mic-core` 执行主路径（M6） | 取 `spec()` 放进 `ModelRequest.tools`；拼 `prompt_hint()`；`invoke` 后转 `ExecOutcome` | 新契约，M6 B2 |
| `mic-tool-fs` / `-shell` / `-web-fetch`（M8） | 实现 `Tool` | 新契约，tools-basic B2 |

## 七、已知演进

- **后台执行（v2）**：`bash.run_in_background` 与 job 运行时一起加入。届时 `execute` 的成功值需要能表达
  `Dispatched { exec_id }`，并给 `ToolContext` 加 job 提交入口；改动随 job B2 定。
- **ToolContext 扩展**：需要会话身份、person、Store 只读查询等时加字段或 port trait（依赖倒置，core 注入）；
  字段私有，加字段不改公开构造以外的签名。
- **工具级配置**：同一模块内按工具开关（如只开 `read` 不开 `write`）出现真实需求时，由该模块自己的配置段表达。

## 八、验收

M3 本身只是接口，随步 3 的 M8 一并验收：临时工程用 `ToolHandle` 调各工具，确认
- 多余字段、类型错误、非对象参数 → `input`，message 含字段名；
- `spec()` 输出符合 §四.1（无 `$ref`、`additionalProperties: false`、字段说明齐全）；
- 同名工具登记两次 → `micnext --config` 启动报 `DuplicateTool`。
