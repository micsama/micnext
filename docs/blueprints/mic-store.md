# B2: mic-store（连带 mic-message 修订）

**状态**: 本文契约 CLOSED（2026-09-23 批准并实现于 `crates/mic-store`，含会话列举增量）；微信投递完成判据随 v0b
**来源**: [`mic-store-design.md`](../brainstorm/mic-store-design.md)（B1）、
[`product-roadmap.md`](../brainstorm/product-roadmap.md) §二、§四
**依赖不变量**: `mic-store` 只依赖 `mic-message`，不依赖 `mic-tool`/`mic-core`/任何模块。

本文只写现行契约；修订过程见 git 历史。

**校验边界**：内部调用互信，store 不对调用方做防御性检查（author 与写入函数是否
匹配、query 是否仍在 Executing、可 claim 行是否带 person_id 等由写者保证）。只在
真正的外部输入处失败：磁盘上库文件的版本、payload 反序列化、SQLite 本身。

## 一、crate 概览

`crates/mic-store`：内核事实的持久化——sessions / session_entries / queries /
persons / person_identities / model_calls 六张内核表，外加模块私有表的迁移与
事务入口。

- 具体结构体 `Store`，不 trait 化；测试用内存 SQLite。
- 内部单个 `rusqlite::Connection`，`Arc<std::sync::Mutex<_>>` +
  `tokio::task::spawn_blocking`；公开方法全部 `async fn`。锁中毒直接 `expect`。
- 外部依赖：`rusqlite`（`bundled`）、`serde`（derive，tool_scope/reason/outcome 等 JSON 列）、`serde_json`、
  `thiserror`、`tokio`（`rt`）、`mic-message`。
- 不生成时间戳：所有 `created_at`/`now` 由调用方传入（"发生时间"只有 emit 侧知道）。

## 二、mic-message 依赖

消息类型、推理内容、工具结果和一次响应的落盘形状只由
[`mic-message.md`](mic-message.md) 定义；本 crate 按该契约序列化与查询，不复制定义。

## 三、公开类型

```rust
pub struct QueryId(pub i64);

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

pub enum QueryState {
    Executing,
    Completed,
    Failed { reason: FailureReason },
    Cancelled { reason: CancelReason },
}

pub enum FailureReason {
    Timeout,
    Provider { message: String },
    Interrupted,
}

pub enum CancelReason {
    User,
    ParentCascade,
}

pub struct Query {
    pub id: QueryId,
    pub session_id: SessionId,
    pub person_id: PersonId,
    pub claimed_start_id: SessionEntryId,
    pub claimed_end_id: SessionEntryId,
    pub state: QueryState,
    pub created_at: i64,
}

pub struct ContextWindow {
    pub summary: Option<String>,
    pub messages: Vec<Message>,
}

pub struct PendingDelivery {
    pub message: Message,
    pub target: DeliveryTarget,
}

/// 一次模型调用的 token 用量。费用不落盘：展示时按当前价格配置换算。
pub struct Usage {
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: u64,
}

pub enum ModelCallPurpose {
    Query(QueryId),
    Compaction,
}

pub enum ModelCallOutcome {
    Completed,
    Failed { message: String },
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
/// 可 claim 的用户输入：Channel 入站，以及 Task/Triggered 的起始消息。
pub struct UserInput {
    pub session_id: SessionId,
    pub person_id: PersonId,
    pub content: MessageContent,
    pub created_at: i64,
}

/// `wait=false` 的终态回报（可 claim）。person 来自内存执行实例句柄。
pub struct CompletionInput {
    pub session_id: SessionId,
    pub person_id: PersonId,
    pub tool_name: String,
    pub exec_id: String,
    pub outcome: ExecOutcome,
    pub created_at: i64,
}

/// 不可 claim 的产出：模型输出、工具结果、HarnessNote、Notification。
pub struct OutputInput {
    pub session_id: SessionId,
    pub author: MessageAuthor,
    pub content: MessageContent,
    pub created_at: i64,
}

pub struct BoundaryInput {
    pub session_id: SessionId,
    pub boundary: ContextBoundary,
    pub created_at: i64,
}

pub struct NewSession {
    pub kind: SessionKind,
    pub parent_session_id: Option<SessionId>,
    pub delivery_target: Option<DeliveryTarget>,
    pub pwd: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
}

pub struct ModelCallInput {
    pub session_id: SessionId,
    pub purpose: ModelCallPurpose,
    pub model: String,
    pub usage: Usage,
    pub outcome: ModelCallOutcome,
    pub started_at: i64,
    pub finished_at: i64,
}

/// 会话列表分页游标：上一页最后一项的排序键。Gateway 在边界把查询参数解析成本类型。
pub struct SessionCursor {
    pub last_activity_at: i64,
    pub session_id: SessionId,
}

pub struct SessionSummary {
    pub session: Session,
    /// 最新 entry 的 `created_at`。
    pub last_activity_at: i64,
    /// 第一条 `User` + `Text` 的前 80 个字符；没有则 `None`（只发了附件）。
    pub preview: Option<String>,
}

pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 还有更早的会话时给出，原样传回取下一页。
    pub next: Option<SessionCursor>,
}
```

## 四、关键规则

### 4.1 claim

可 claim 判别式（SQL 层，靠 `content_kind`/`author_kind` 判别列）：

```sql
entry_kind = 'message' AND (
     (author_kind = 'user' AND content_kind IN ('text', 'attachment'))
  OR content_kind = 'completion'
)
```

`claim_next` 单事务：已有 `Executing` Query → `None`；取下界 = 本 session 最新
Query 的 `claimed_end_id`（无则 0）；在下界之后的可 claim entry 序列上取
**最长同 `person_id` 连续前缀**；插入 `Executing` Query。区间
`[claimed_start_id, claimed_end_id]` 的含义是"本 session、区间内、满足判别式的行"，
不是区间内所有行。

分组键用 `person_id` 而非 author：两条不同 person 发起的 completion author 都是
`Tool{同名}`，按 author 会被合并，破坏"一个 Query 一个 person"。同一 person 相邻
的用户消息与 completion 会进同一个 Query。

### 4.2 person 列

`session_entries.person_id` 只对可 claim entry 非 NULL，由 `append_user_input`/
`append_completion` 写入；其余写入函数写 NULL。

### 4.3 Person 与身份

- 配置声明的 person：启动时 `ensure_person(name)` + `bind_identity`。
- 未声明的发送者：`resolve_identity` 自动注册，名为
  `{channel}:{external_id}`。配置名禁止含 `:`（由 mic-core 校验配置时 Fail Fast），
  两类名字不会冲突。
- `bind_identity` 可改绑：自动注册的身份后来在配置里认领时，新消息归新 person，
  历史 entry 保留原 person_id。返回旧 person 供调用方记日志。首次绑定时
  `display_name` 为空串，该身份下次入站经 `resolve_identity` 刷新。
- 权限上限不进 store，由 mic-core 从配置计算。

### 4.4 投递

`pending_deliveries(channel)`：`delivered_at IS NULL`，所属 session 的
`delivery_channel = channel`，`author_kind IN ('assistant','notification')`，
`content_kind IN ('text','attachment')`。`Reasoning`/`ToolCall`/`ToolResult`/
`Completion` 与入站消息不投递。Root session 的 `channel`（入站来源）与
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
- **活跃时间算出来、不落列**：一处真相是 entry 本身，取按 id 最新一条 entry 的
  `created_at`，走 `idx_entries_session`。个人规模全量排序可接受；出现性能摩擦再加索引或冗余列。
- 预览在 SQL 侧用 `json_extract(payload, '$.Text.content')` 截取，依赖 `MessageContent`
  的 serde 外部标签形状；序列化由本 crate 独占，形状变化时与写入一起改。预览只作展示，不作标识。
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

    // ---- 写 entry ----
    pub async fn append_user_input(&self, input: UserInput) -> Result<SessionEntryId, StoreError>;
    pub async fn append_completion(&self, input: CompletionInput) -> Result<SessionEntryId, StoreError>;
    /// NOTE: author 不为 `User`——用户输入走 `append_user_input`。
    pub async fn append_output(&self, input: OutputInput) -> Result<SessionEntryId, StoreError>;
    pub async fn append_boundary(&self, input: BoundaryInput) -> Result<SessionEntryId, StoreError>;

    // ---- 调度 ----
    pub async fn claim_next(&self, session_id: SessionId, now: i64)
        -> Result<Option<Query>, StoreError>;
    pub async fn finish_query(&self, id: QueryId, state: QueryState, now: i64)
        -> Result<(), StoreError>;
    /// 启动时把遗留 `Executing` 收尾为 `Failed{Interrupted}`，返回被收尾的 Query。
    pub async fn interrupt_stale_queries(&self, now: i64) -> Result<Vec<Query>, StoreError>;

    // ---- 读 ----
    pub async fn context_window(&self, session_id: SessionId) -> Result<ContextWindow, StoreError>;
    /// 稳定 entry 回放；游标只能是 entry id，实时增量不作游标。
    pub async fn entries_after(&self, session_id: SessionId, after: Option<SessionEntryId>)
        -> Result<Vec<SessionEntry>, StoreError>;

    // ---- 投递 ----
    pub async fn pending_deliveries(&self, channel: &str) -> Result<Vec<PendingDelivery>, StoreError>;
    pub async fn mark_delivered(&self, id: SessionEntryId, at: i64) -> Result<(), StoreError>;

    // ---- 用量 ----
    pub async fn record_model_call(&self, input: ModelCallInput) -> Result<(), StoreError>;
    pub async fn session_usage(&self, session_id: SessionId) -> Result<Usage, StoreError>;

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

```sql
PRAGMA journal_mode = WAL;
PRAGMA foreign_keys = ON;

CREATE TABLE schema_migrations (
  module   TEXT PRIMARY KEY,
  version  INTEGER NOT NULL
);

CREATE TABLE persons (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL UNIQUE,
  created_at  INTEGER NOT NULL
);

CREATE TABLE person_identities (
  channel       TEXT NOT NULL,
  external_id   TEXT NOT NULL,
  person_id     INTEGER NOT NULL REFERENCES persons(id),
  display_name  TEXT NOT NULL,
  created_at    INTEGER NOT NULL,
  PRIMARY KEY (channel, external_id)
);

CREATE TABLE sessions (
  id                  INTEGER PRIMARY KEY,
  kind                TEXT NOT NULL,        -- 'root' | 'task' | 'triggered'
  channel             TEXT,                 -- root
  chat                TEXT,                 -- root
  parent_tool_call_id TEXT,                 -- task
  trigger_module      TEXT,                 -- triggered
  trigger_ref         TEXT,                 -- triggered
  parent_session_id   INTEGER REFERENCES sessions(id),
  delivery_channel    TEXT,                 -- 三列一组，同为 NULL 表示无发送目标
  delivery_version    INTEGER,
  delivery_payload    TEXT,
  pwd                 TEXT NOT NULL,
  tool_scope          TEXT NOT NULL,        -- ToolScope JSON
  created_at          INTEGER NOT NULL
);
CREATE UNIQUE INDEX idx_root_chat ON sessions(channel, chat) WHERE kind = 'root';
CREATE INDEX idx_sessions_delivery ON sessions(delivery_channel)
  WHERE delivery_channel IS NOT NULL;

CREATE TABLE session_entries (
  id            INTEGER PRIMARY KEY,        -- SessionEntryId
  session_id    INTEGER NOT NULL REFERENCES sessions(id),
  entry_kind    TEXT NOT NULL,              -- 'message' | 'boundary'
  author_kind   TEXT,                       -- boundary 行 NULL
  author_ident  TEXT,                       -- User.id / Assistant.model / Tool.name /
                                            -- Notification.source；HarnessNote 与 boundary NULL
  content_kind  TEXT,                       -- text|reasoning|tool_call|tool_result|
                                            -- completion|attachment；boundary 行 NULL
  person_id     INTEGER REFERENCES persons(id),  -- 仅可 claim 行非 NULL
  payload       TEXT NOT NULL,              -- MessageContent / ContextBoundary JSON
  created_at    INTEGER NOT NULL,
  delivered_at  INTEGER
);
CREATE INDEX idx_entries_session ON session_entries(session_id, id);
CREATE INDEX idx_entries_undelivered ON session_entries(session_id)
  WHERE delivered_at IS NULL AND author_kind IN ('assistant', 'notification');

CREATE TABLE queries (
  id               INTEGER PRIMARY KEY,
  session_id       INTEGER NOT NULL REFERENCES sessions(id),
  person_id        INTEGER NOT NULL REFERENCES persons(id),
  claimed_start_id INTEGER NOT NULL,
  claimed_end_id   INTEGER NOT NULL,
  state            TEXT NOT NULL,           -- executing|completed|failed|cancelled
  reason           TEXT,                    -- FailureReason / CancelReason JSON
  created_at       INTEGER NOT NULL,
  finished_at      INTEGER
);
CREATE INDEX idx_queries_session ON queries(session_id, id);

CREATE TABLE model_calls (
  id                 INTEGER PRIMARY KEY,
  session_id         INTEGER NOT NULL REFERENCES sessions(id),
  query_id           INTEGER REFERENCES queries(id),  -- NULL 表示压缩调用
  model              TEXT NOT NULL,
  input_tokens       INTEGER NOT NULL,
  output_tokens      INTEGER NOT NULL,
  cache_read_tokens  INTEGER NOT NULL,
  cache_write_tokens INTEGER NOT NULL,
  reasoning_tokens   INTEGER NOT NULL,
  outcome            TEXT NOT NULL,         -- ModelCallOutcome JSON
  started_at         INTEGER NOT NULL,
  finished_at        INTEGER NOT NULL
);
CREATE INDEX idx_model_calls_session ON model_calls(session_id);
```

- 判别列（`author_kind`/`author_ident`/`content_kind`）是写入时从 payload 投影的
  查询列，只在插入函数一处生成，不单独更新。
- `model_calls` 记录每次模型调用（含失败与重试），`purpose` 由 `query_id` 是否为
  NULL 表达。

## 七、副作用

- 文件系统：`open` 创建/打开 SQLite 文件及 WAL 附属文件。
- 事务：`open` 的每个迁移版本、`resolve_identity`、`bind_identity`、
  `resolve_root_session`、`claim_next`、`finish_query`、`interrupt_stale_queries`、
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
| `mic-core` 启动 | 汇总各模块 `Migration` 后 `open`；`ensure_person`/`bind_identity`（配置）；`interrupt_stale_queries` | 新契约 |
| `mic-core` 调度与执行 | `claim_next`/`finish_query`/`context_window`/`append_output`/`append_boundary`/`append_completion`/`record_model_call` | 新契约 |
| `mic-gateway`（v0a Web） | 写经 `Kernel`（`resolve_root_session`/`append_user_input`，见 mic-core-module）；从 Store 只读 `entries_after`（稳定回放）/`list_root_sessions`（会话列表，查看微信会话时 `channel = "wechat"`）/`session_usage` | 新契约；Web 会话无投递目标 |
| 微信适配器 | 经 Gateway 入站和发送；编解码自己的 `DeliveryTarget.payload` | 新契约；是否需要显式 adapter ack 待接入方式核实 |
| 模块（如 `mic-cron`） | `Migration`、`with_module_tx`、`create_session`（`Triggered`） | 新契约 |
| `mic-tool` | 不使用（依赖不变量禁止） | 无影响 |
| `mic-message` 现有实现 | 按独立 [`mic-message.md`](mic-message.md) 修订类型 | 无落盘数据、无下游代码，直接改，不需 parallel change |

## 十、未纳入

- 入站去重（已知缺口，明确推迟；补法是 core v2 迁移加两列一索引）。
- 人工审批的请求/决定内容变体（roadmap §四-6，方向收敛后再起 B2）。
- 按时间段统计用量；会话改名/标题生成、删除归档、列表项"执行中"标记。
- `FileRef` 文件的写入、清理与尺寸上限（roadmap §四-7）。
