# B2: mic-store（连带 mic-message 修订）

**状态**: 本文契约 CLOSED（2026-09-23 批准并实现于 `crates/mic-store`，含会话列举增量）；2026-09-24 按 [`storage-restructure.md`](storage-restructure.md) 重写为 session / run / message / model_call；微信投递完成判据随 v0b
**来源**: [`mic-store-design.md`](../brainstorm/mic-store-design.md)（B1）、
[`product-roadmap.md`](../brainstorm/product-roadmap.md) §二、§四
**依赖不变量**: `mic-store` 只依赖 `mic-message`，不依赖 `mic-tool`/`mic-core`/任何模块。

本文只写现行契约；修订过程见 git 历史。

**校验边界**：内部调用互信，store 不对调用方做防御性检查（run 是否仍在 `executing` 等由写者保证）；
写入口的两条 `assert`（`append` 不收 `Reply`、输入不带 run）是调用方 bug 的断言，不是校验。只在
真正的外部输入处失败：磁盘上库文件的版本、payload 反序列化、SQLite 本身。

## 一、crate 概览

`crates/mic-store`：内核事实的持久化——`core_sessions` / `core_messages` / `core_runs` /
`core_model_calls` / `core_persons` / `core_person_identities` 六张内核表，外加模块私有表的迁移与
事务入口。

- 具体结构体 `Store`，不 trait 化；测试用内存 SQLite。
- 内部单个 `rusqlite::Connection`，`Arc<std::sync::Mutex<_>>` +
  `tokio::task::spawn_blocking`；公开方法全部 `async fn`。锁中毒直接 `expect`。
- 外部依赖：`rusqlite`（`bundled`）、`serde`（derive，tool_scope 与 payload JSON 列）、`serde_json`、
  `thiserror`、`tokio`（`rt`）、`mic-message`。
- 不生成时间戳：所有 `created_at`/`now` 由调用方传入（"发生时间"只有 emit 侧知道）；
  单位统一为 Unix 毫秒（`i64`）。

## 二、mic-message 依赖

`MessageBody`（消息种类闭集）、`ReplyBlock`、`ToolOutcome` 等落盘形状只由
[`mic-message.md`](mic-message.md) 定义；本 crate 按该契约序列化与查询，不复制定义。

## 三、公开类型

```rust
pub struct RunId(pub i64);
pub struct ModelCallId(pub i64);

/// 外部身份：某 Channel 上的某个发送者。
pub struct Identity {
    pub channel: String,
    pub external_id: String,
}

pub struct Person {
    pub id: PersonId,
    /// 配置里声明的 person 用配置名；自动注册的用 `{channel}:{external_id}`。
    pub name: String,
    pub created_at: i64,
}

/// 发送目标：`channel` 决定由哪个 Channel 投递；`payload` 是该 Channel 自己
/// 编解码的版本化不透明数据。
pub struct DeliveryTarget {
    pub channel: String,
    pub version: u32,
    pub payload: String,
}

pub enum ToolScope {
    All,
    Only(BTreeSet<String>),
}

pub enum SessionKind {
    /// Channel 入站创建；`(channel, chat)` 全局唯一。
    Root { channel: String, chat: String },
    /// 子 agent；父 session 在 `Session.parent_session_id`。
    Task { parent_tool_call_id: String },
    /// 模块触发（cron、webhook…）。`ref_id` 由该模块解释，内核只当来源元数据。
    Triggered { module: String, ref_id: String },
}

pub struct Session {
    pub id: SessionId,
    pub kind: SessionKind,
    pub parent_session_id: Option<SessionId>,
    pub delivery_target: Option<DeliveryTarget>,
    pub pwd: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
}

/// 一次 agent loop 的状态，列值为 snake_case。失败详情看调用行的 `error`。
pub enum RunState {
    Executing,
    Completed,
    /// 模型调用不可重试或重试用尽。
    ProviderFailed,
    /// 轮次用尽：模型已做过不带工具的总结，但任务未必完成（run-execution §4.3）。
    MaxTurns,
    /// 进程停止时仍在执行，启动时收尾。
    Interrupted,
}

pub struct Run {
    pub id: RunId,
    pub session_id: SessionId,
    pub state: RunState,
    pub created_at: i64,
    pub finished_at: Option<i64>,
}

pub struct ContextWindow {
    /// 最近一次 `Compaction` 的摘要。
    pub summary: Option<String>,
    /// 最近一次 `Boundary` 之后、排除未认领输入的消息，按 id。
    pub messages: Vec<Message>,
}

pub struct PendingDelivery {
    pub message: Message,
    pub target: DeliveryTarget,
}

/// 一次模型调用的 token 用量；上游没报的项为 `None`。输入含缓存命中，输出含推理。
/// 费用不落盘：展示时按当前价格配置换算。
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: Option<u64>,
    pub cache_write_tokens: Option<u64>,
    pub reasoning_tokens: Option<u64>,
}

pub enum ModelCallOutcome {
    /// `usage` 为 `None` = 上游没报。`blocks` 非空时写成一条 `Reply`。
    Replied { usage: Option<Usage>, blocks: Vec<ReplyBlock> },
    Failed { error: String },
}

/// 模块迁移。`module` 不得为 `"core"`（内核保留）；同一模块 `version` 从 1 连续递增。
pub struct Migration {
    pub module: &'static str,
    pub version: u32,
    pub sql: &'static str,
}
```

### 写入参数

```rust
pub struct NewSession {
    pub kind: SessionKind,
    pub parent_session_id: Option<SessionId>,
    pub delivery_target: Option<DeliveryTarget>,
    pub pwd: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
}

/// 一次模型调用尝试（含失败与重试）。
pub struct NewModelCall {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    /// 请求模型名（`Provider::model()`）。
    pub model: String,
    pub started_at: i64,
    pub finished_at: i64,
    pub outcome: ModelCallOutcome,
}

/// 会话列表分页游标：上一页最后一项的排序键。Gateway 在边界把查询参数解析成本类型。
pub struct SessionCursor {
    pub last_activity_at: i64,
    pub session_id: SessionId,
}

pub struct SessionSummary {
    pub session: Session,
    /// 最新消息的 `created_at`。
    pub last_activity_at: i64,
    /// 首条 `UserInput` 首个文本片段的前 80 个字符；没有则 `None`（只发了附件）。
    pub preview: Option<String>,
}

pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 还有更早的会话时给出，原样传回取下一页。
    pub next: Option<SessionCursor>,
}
```

## 四、关键规则

### 4.1 认领与并入

**未认领输入**（一个谓词，四处复用：认领、并入、启动补跑扫描、上下文排除）：

```sql
run_id IS NULL AND kind IN ('UserInput', 'Completion')
```

**段**：本会话未认领输入按 id 排序后，开头连续同 `person_id` 的一段。

- `claim_next` 单事务：会话已有 `executing` run → `None`；没有段 → `None`；否则插入 `executing` run，
  回填这段的 `run_id`。
- `absorb(run)` 单事务：取同样的段，其 person 等于该 run 首条输入的 person 才回填；有回填返回 true。

分组键是 person 而非种类：同一 person 相邻的用户消息与 completion 进同一个 run；不同 person 发起的
completion 不会合并，保证"一个 run 一个 person"。

### 4.2 写入口

按产出者分三个：`append` 写除 `Reply` 外的所有种类（`UserInput`/`Completion` 入站时未认领，`run`
必为 `None`）；`record_model_call` 同一事务写调用行，`Replied` 且 `blocks` 非空时再写 `Reply`
（`run_id`、`session_id` 同调用行，`model_call_id` 指向它）；认领与并入只回填输入的 `run_id`。

### 4.3 Person 与身份

- 配置声明的 person：启动时 `ensure_person(name)` + `bind_identity`。
- 未声明的发送者：`resolve_identity` 自动注册，名为
  `{channel}:{external_id}`。配置名禁止含 `:`（由 mic-core 校验配置时 Fail Fast），
  两类名字不会冲突。
- `bind_identity` 可改绑：自动注册的身份后来在配置里认领时，新消息归新 person，
  历史消息保留原 person。返回旧 person 供调用方记日志。首次绑定时
  `display_name` 为空串，该身份下次入站经 `resolve_identity` 刷新。
- 权限上限不进 store，由 mic-core 从配置计算。

### 4.4 投递

`pending_deliveries(channel)`：`delivered_at IS NULL`，所属 session 的
`delivery_channel = channel`，`kind IN ('Reply','Notification')`，按 id。`Reply` 里哪些块对外可见由
Channel 决定（通常只发 `Text`）。Root session 的 `channel`（入站来源）与
`delivery_target.channel`（投递去向）是两个事实，通常相同但不强制。

### 4.5 迁移

`schema_migrations(module, version)` 记录每个模块已应用的最高版本。`open` 时：

1. 应用内核迁移（`module = "core"`），再按传入顺序应用各模块迁移；每个版本单独
   事务，成功后更新记录。
2. 库中版本高于二进制已知最高版本 → `SchemaAhead`（库文件来自更新的二进制，Fail Fast）。
3. 库里有、但本次未传入的模块（功能被删）→ 不动，其表保留休眠，重新启用即恢复。

模块表名必须以 `{module}_` 开头，只能经 `with_module_tx` 访问；内核表只由 `Store`
方法写。运行时不设限制，由各模块的单元测试（`tests.rs`）把关：用
`open_in_memory(&该模块迁移)` 建库，跑一遍该模块的读写操作，断言
① `sqlite_master` 里除内核表外只有 `{module}_` 前缀的表；② 内核表行数不变。

### 4.6 会话列举

- 只列已有用户输入的 Root 会话；Task/Triggered 从父会话或定时看板进入。v0 不做改名、删除、归档。
- **活跃时间算出来、不落列**：一处真相是消息本身，取按 id 最新一条消息的
  `created_at`，走 `idx_messages_session`。个人规模全量排序可接受；出现性能摩擦再加索引或冗余列。
- 预览在 SQL 侧用 `json_each(payload, '$.parts')` 取首条 `UserInput` 的首个 `Text` 片段，依赖
  `MessageBody`/`ContentPart` 的 serde 形状；序列化由本 crate 独占，形状变化时与写入一起改。预览只作展示，不作标识。
- 游标是强类型的排序位置，不存在"非法游标"，结果可能为空页；不新增错误变体。
- 翻页期间某会话有新消息会跳到最前，后续页可能缺席或重复一次；列表以刷新/实时事件为准，不做快照游标。

## 五、公开签名

```rust
pub use rusqlite; // 供模块在 with_module_tx 里使用，保证版本一致

impl Store {
    pub async fn open(path: &Path, modules: &[Migration]) -> Result<Self, StoreError>;
    pub async fn open_in_memory(modules: &[Migration]) -> Result<Self, StoreError>;

    // ---- Person ----
    pub async fn ensure_person(&self, name: &str, now: i64) -> Result<PersonId, StoreError>;
    /// 返回改绑前的 person（原先未绑定则 None）。
    pub async fn bind_identity(&self, identity: Identity, person: PersonId, now: i64)
        -> Result<Option<PersonId>, StoreError>;
    /// 原子 get-or-create；已存在时刷新 display_name。
    pub async fn resolve_identity(&self, identity: Identity, display_name: &str, now: i64)
        -> Result<PersonId, StoreError>;
    pub async fn person(&self, id: PersonId) -> Result<Person, StoreError>;

    // ---- Session ----
    /// Root 专用原子 get-or-create；已存在则返回既有行，init 被忽略。
    pub async fn resolve_root_session(&self, channel: &str, chat: &str, init: NewSession)
        -> Result<Session, StoreError>;
    /// Task/Triggered 用。
    pub async fn create_session(&self, new: NewSession) -> Result<Session, StoreError>;
    /// id 可能来自外部（Gateway 路径参数），不存在返回 `None`。
    pub async fn session(&self, id: SessionId) -> Result<Option<Session>, StoreError>;
    pub async fn set_pwd(&self, id: SessionId, pwd: &str) -> Result<(), StoreError>;
    /// 列出 `channel` 下已有用户输入的 Root 会话，按 `(last_activity_at, id)` 降序。
    /// `before = None` 取第一页；`limit` 上界由调用方在边界限定。
    pub async fn list_root_sessions(&self, channel: &str, before: Option<SessionCursor>,
        limit: NonZeroU32) -> Result<SessionPage, StoreError>;

    // ---- 写消息 ----
    /// 除 `Reply` 外的所有种类；传 `Reply` 或给输入带 run 是调用方 bug，panic。
    pub async fn append(&self, session_id: SessionId, run: Option<RunId>, body: MessageBody, at: i64)
        -> Result<Message, StoreError>;
    /// 同一事务写调用行；`Replied` 且 `blocks` 非空时再写指向它的 `Reply` 并返回。
    pub async fn record_model_call(&self, call: NewModelCall)
        -> Result<(ModelCallId, Option<Message>), StoreError>;

    // ---- 调度 ----
    pub async fn claim_next(&self, session_id: SessionId, now: i64) -> Result<Option<Run>, StoreError>;
    /// 执行中并入新输入（§4.1），有并入返回 true。调用方保证 run 处于 `executing`。
    pub async fn absorb(&self, run: RunId) -> Result<bool, StoreError>;
    /// 有未认领输入的会话（启动补跑用）。
    pub async fn sessions_with_unclaimed_input(&self) -> Result<Vec<SessionId>, StoreError>;
    pub async fn finish_run(&self, id: RunId, state: RunState, now: i64) -> Result<(), StoreError>;
    /// 启动时把遗留 `executing` 收尾为 `interrupted`，返回被收尾的 run。
    pub async fn interrupt_stale_runs(&self, now: i64) -> Result<Vec<Run>, StoreError>;

    // ---- 读 ----
    /// 该 run 的全部消息，按 id（启动收尾用）。
    pub async fn run_messages(&self, run: RunId) -> Result<Vec<Message>, StoreError>;
    pub async fn context_window(&self, session_id: SessionId) -> Result<ContextWindow, StoreError>;
    /// 稳定回放（含 `Boundary`）；游标只能是消息 id，实时增量不作游标。
    pub async fn messages_after(&self, session_id: SessionId, after: Option<MessageId>)
        -> Result<Vec<Message>, StoreError>;

    // ---- 投递 ----
    pub async fn pending_deliveries(&self, channel: &str) -> Result<Vec<PendingDelivery>, StoreError>;
    pub async fn mark_delivered(&self, id: MessageId, at: i64) -> Result<(), StoreError>;

    // ---- 模块表 ----
    pub async fn with_module_tx<R, F>(&self, f: F) -> Result<R, StoreError>
    where
        R: Send + 'static,
        F: FnOnce(&rusqlite::Transaction<'_>) -> rusqlite::Result<R> + Send + 'static;
}
```

### 错误枚举

```rust
#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("sqlite: {0}")]
    Sqlite(#[from] rusqlite::Error),
    #[error("payload 序列化: {0}")]
    Payload(#[from] serde_json::Error),
    #[error("模块 {module} 库版本 v{db} 高于二进制已知 v{known}")]
    SchemaAhead { module: String, db: u32, known: u32 },
}
```

## 六、schema（core v1）

内核表统一 `core_` 前缀；`schema_migrations` 是所有模块共用的迁移登记表，不属于内核事实，保持原名。
实际 SQL 见 `crates/mic-store/src/schema.rs`，列如下：

```
core_persons            id, name UNIQUE, created_at
core_person_identities  (channel, external_id) PK, person_id, display_name, created_at
core_sessions           id, kind('root'|'task'|'triggered'), channel?, chat?, parent_tool_call_id?,
                        trigger_module?, trigger_ref?, parent_session_id?,
                        delivery_channel?, delivery_version?, delivery_payload?（三列同空同非空）,
                        pwd, tool_scope(JSON), created_at
core_runs               id, session_id, state, created_at, finished_at?
core_model_calls        id, session_id, run_id?, model, error?,
                        input_tokens?, output_tokens?, cache_read_tokens?, cache_write_tokens?,
                        reasoning_tokens?, started_at, finished_at
core_messages           id, session_id, run_id?, model_call_id?, payload, created_at, delivered_at?,
                        kind       VIRTUAL 生成列 ← json_extract(payload, '$.kind')
                        person_id  VIRTUAL 生成列 ← json_extract(payload, '$.person')
```

- `payload` 是 `MessageBody` 的 serde（内部标签 `kind`），唯一真相；生成列写入方不填。
  例外：`Reply` 的 `model` 不进 payload，读出时按 `model_call_id` JOIN `core_model_calls.model` 还原。
- `core_model_calls`：每次调用尝试一行（含失败与重试），`run_id` 为空表示压缩调用；
  `error IS NULL` ⇔ 成功；失败行用量全空；成功行 `input_tokens`、`output_tokens` 同空同非空。
- 外键：`run_id` → `core_runs`，`model_call_id` → `core_model_calls`，`person_id` → `core_persons`，
  `parent_session_id` → `core_sessions`。
- 索引：`idx_root_chat`（root 的 `(channel, chat)` 唯一）；`idx_sessions_delivery`；
  `core_messages(session_id, id)`；未认领 `(session_id, id) WHERE run_id IS NULL AND kind IN
  ('UserInput','Completion')`；待投递 `(session_id) WHERE delivered_at IS NULL AND kind IN
  ('Reply','Notification')`；`core_messages(run_id)`；`core_runs(session_id) WHERE state = 'executing'`；
  `core_model_calls(session_id)`。

### 不变量

1. 每个会话至多一个 `executing` run。
2. 输入的 `run_id` 只由认领/并入从空填成非空，之后不变；同一 run 的输入属于同一 person。
3. 未认领输入不进模型上下文（`context_window` 排除），回放照常返回。
4. `model_call_id` 只出现在 `Reply` 上，指向一次成功调用，二者 `run_id`、`session_id` 相同。
5. run 进入终态时，其每个 `ToolCall` 块恰有一条同 run 的 `ToolResult`（启动收尾负责补齐，run-execution §4.6）。
6. `run_id` 非空的消息与调用，`session_id` 等于该 run 的 `session_id`。

## 七、副作用

- 文件系统：`open` 创建/打开 SQLite 文件及 WAL 附属文件。
- 事务：`open` 的每个迁移版本、`resolve_identity`、`bind_identity`、
  `resolve_root_session`、`record_model_call`、`claim_next`、`absorb`、`interrupt_stale_runs`、
  `with_module_tx`；其余单语句天然原子。
- 不做：后台任务、定时清理、发送、解释 `DeliveryTarget.payload`/`trigger_ref`、
  校验 I3、计算权限、生成时间戳、管理 `FileRef` 指向的文件、换算费用。

## 八、依赖方向

```
mic-store ← mic-message
```

`lib.rs` 只 re-export 上述公开 API 与 `rusqlite`；SQL 与行映射 `pub(crate)`。

## 九、调用方枚举

| 调用方 | 用到什么 | 兼容性 |
|---|---|---|
| `mic-core` 启动 | 汇总各模块 `Migration` 后 `open`；`ensure_person`/`bind_identity`（配置）；`interrupt_stale_runs`/`run_messages`/`sessions_with_unclaimed_input` | 现行 |
| `mic-core` 调度与执行 | `claim_next`/`absorb`/`finish_run`/`context_window`/`append`/`record_model_call` | 现行 |
| `mic-gateway`（v0a Web） | 写经 `Kernel`（`resolve_root_session`/`append_user_input`，见 mic-core-module）；从 Store 只读 `messages_after`（稳定回放）/`list_root_sessions`（会话列表，查看微信会话时 `channel = "wechat"`）；用量读取随 M9 B2 定形状 | 待 M9；Web 会话无投递目标 |
| 微信适配器 | 经 Gateway 入站和发送；编解码自己的 `DeliveryTarget.payload` | 新契约；是否需要显式 adapter ack 待接入方式核实 |
| 模块（如 `mic-cron`） | `Migration`、`with_module_tx`、`create_session`（`Triggered`） | 新契约 |
| `mic-tool` | 不使用（依赖不变量禁止） | 无影响 |

## 十、未纳入

- 入站去重（已知缺口，明确推迟；补法是 core v2 迁移加两列一索引）。
- 人工审批的请求/决定内容变体（roadmap §四-6，方向收敛后再起 B2）。
- 按时间段统计用量；会话改名/标题生成、删除归档、列表项"执行中"标记。
- `FileRef` 文件的写入、清理与尺寸上限（roadmap §四-7）。
