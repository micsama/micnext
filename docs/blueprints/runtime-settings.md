# B2: 运行期设置（人设、提示词分块、对话偏好；模型设置为阶段二）

**状态**: 阶段一 CLOSED（2026-10-08 批准并实现于 mic-store / mic-core / mic-gateway / web；§九 验收 1～7 用开发库副本、假模型服务与 Playwright 实跑通过）；阶段二（§十）待细化再批
**来源**: [`runtime-settings.md`](../brainstorm/runtime-settings.md)（B1，2026-10-08 对齐）、
[`storage-and-models.md`](../brainstorm/storage-and-models.md) §二、
[`orthogonality-review-2026-09-24.md`](../brainstorm/orthogonality-review-2026-09-24.md) A
**修订**: mic-store（schema core v2、设置与人设读写、`claim_next` 定下本轮设置）、mic-core（提示词分块、
`Kernel` 设置方法、`max_turns` 移出配置）、mic-gateway（设置接口、会话人设、`workdir` 移出配置）、
web（设置页、输入框人设选择）。不新增 crate，不新增跨 crate 依赖。

## 〇、已定方向（2026-10-08 与 human 确认）

| # | 问题 | 决定 |
|---|---|---|
| Q1 | 配置放哪 | `config.toml` 只放启动必需或不能让网页改的：`data_dir`、`owner`、`listen`、`token`。其余进 SQLite，经 Web 改，下一轮生效 |
| Q2 | system prompt 分块 | 系统 → 人设 → 通用偏好。系统是二进制常量，只读 |
| Q3 | 人设 | 「名字 + 提示词」，不绑模型、目录、工具。3 个内置人设，只读不可删，只能复制后编辑；源码文件为唯一真相，每次启动按固定 id 同步覆盖，不做恢复默认 |
| Q4 | 选择放哪 | 会话保存下一轮选择；run 保存当轮事实。新会话取默认；改默认不影响已有会话 |
| Q5 | 删除 | 内置不可删；默认人设先换默认才能删；会话选着的人设已删除（含内置下线）时提示「原人设已删除」，下一轮自动改用默认人设并写回会话 |
| Q6 | `owner` | 是消息归属身份，不是昵称，留在配置；想改称呼写进通用偏好 |
| Q7 | 微信 | 不在本文范围；以后单会话，它的选择放 Web 设置的微信区域 |

## 一、用户视角的效果

- 侧栏底部有「设置」。设置页两块（阶段二加「模型」）：
  - **对话偏好**：默认人设、通用偏好（对所有人设生效）、新会话默认工作目录（不影响已有会话）、
    单轮调用上限；系统提示可展开只读查看。
  - **人设**：列表 + 编辑。内置人设只读，按钮「复制并编辑」；自建人设可改名、改提示词、删除。
- 输入框发送按钮左边是人设选择。新会话显示默认人设，可以在发第一条消息前换；已有会话显示它记住的选择。
  正在回复时切换，旁边提示「下一轮生效」，这一轮（含期间插话）仍用原人设。
- 会话选着的人设被删了：选择处直接显示默认人设并提示「原人设已删除，下一轮改用默认人设」，发送不受影响。
- 只读会话（CLI 等其它渠道）不显示选择器。
- 改设置不需要重启；正在生成的回答不受影响。
- 首次启动不用改配置：库里已有 3 个内置人设和默认偏好。

## 二、范围

**阶段一（本次批准即实现）**：人设、通用偏好、`max_turns` 与 Web 默认工作目录入库；提示词分块；
会话人设选择；run 开始时定下设置并留快照；设置页与输入框选择器。

**阶段二（草案见 §十，阶段一后细化再批）**：模型条目入库与 key 加密、Provider 工厂（正交审查 A2）、
会话模型选择、删 `[models]`。

**不做**：界面展示「本轮设置」详情（快照先落库，展示随用量显示一起做）；微信区域；子 agent / Triggered
会话的人设继承（随其生产者定）；`/models` 自动拉取（阶段二之后）。

## 三、实体、写者与不变量

| 实体 | 写者 | 说明 |
|---|---|---|
| 人设 `core_personas` | Kernel（Web 设置页） | 内置行由迁移写入；删除为软删除，行永不物理删除 |
| 对话偏好 `core_settings` | Kernel（Web 设置页） | 恰好一行 |
| 会话选择 `core_sessions.persona_id` | 建会话时 store 填默认或指定值；之后 Kernel（Web 选择器） | |
| run 快照 `core_runs.persona_*`、`general_prompt`、`max_turns` | store 在 `claim_next` 同一事务写 | 写后不改 |

人设状态：`内置`（只读；随源码同步，源码移除后软删除）；`自建` →（删除）→ `已删除`（终态，列表不显示，按 id 仍可读）。

内置同步（`mic-store/src/personas.rs`，每次打开库、与迁移分开、单事务）：源码 `id + name + include_str!` 为唯一真相，
按 id upsert 覆盖名字与提示词（并恢复未删除）；同名的未删除自建人设加后缀「（自建）」让位；源码里已没有的旧内置人设软删除；
默认人设若已失效则回到 id 1（id 1 永不下线）。用户新建/改名为内置同名仍由 `NameTaken` 拒绝。

不变量：

1. 默认人设始终是未删除的人设；正被设为默认的人设不能删除。
2. 名字在未删除人设中唯一。
3. 每个会话恰好选着一个人设（可能已删除）；已删除不可被新选中；`claim_next` 遇已删除则改用默认人设并写回会话。
4. 每个 run 的设置在认领时一次读定，同一事务写快照；执行期间、并入的插话都按快照，不再读设置。
5. 迁移前的历史 run 快照列为 NULL；新 run 必有完整快照。

关键场景：

- **执行中切换**：run 7 用 A 执行中 → Web `PUT persona=B` 改会话行 → 插话并入 run 7 仍用 A
  → run 7 结束 → 下一次 `claim_next` 读到 B，run 8 快照 B。
- **删除竞态**：会话选着 C 且有已写入未认领的输入 → 用户删除 C（软删除）→ 认领照常按 C 的行写快照并执行
  （选择发生在发送前，按发送时的意图执行）；之后 Web 发送被拒，提示重选。

## 四、存储（mic-store，schema core v2）

```sql
CREATE TABLE core_personas (
  id          INTEGER PRIMARY KEY,
  name        TEXT NOT NULL,
  prompt      TEXT NOT NULL,
  builtin     INTEGER NOT NULL,
  created_at  INTEGER NOT NULL,
  updated_at  INTEGER NOT NULL,
  deleted_at  INTEGER
);
CREATE UNIQUE INDEX idx_personas_name ON core_personas(name) WHERE deleted_at IS NULL;
-- 仅占位 id 1 以满足 core_settings 外键；内置内容由启动同步写入（见上）。

CREATE TABLE core_settings (
  id                  INTEGER PRIMARY KEY CHECK (id = 1),
  default_persona_id  INTEGER NOT NULL REFERENCES core_personas(id),
  general_prompt      TEXT NOT NULL,
  default_workdir     TEXT NOT NULL,
  max_turns           INTEGER NOT NULL CHECK (max_turns >= 1)
);
INSERT INTO core_settings VALUES (1, 1, '', '~/workspace/mic', 50);

-- 不加外键：SQLite 在外键开启时不允许 ADD COLUMN 带 REFERENCES 且默认值非 NULL；引用由写入方法校验
ALTER TABLE core_sessions ADD COLUMN persona_id INTEGER NOT NULL DEFAULT 1;
ALTER TABLE core_runs ADD COLUMN persona_id     INTEGER;   -- 不加外键：快照，不是引用
ALTER TABLE core_runs ADD COLUMN persona_name   TEXT;
ALTER TABLE core_runs ADD COLUMN persona_prompt TEXT;
ALTER TABLE core_runs ADD COLUMN general_prompt TEXT;
ALTER TABLE core_runs ADD COLUMN max_turns      INTEGER;
```

`DEFAULT 1` 只为回填已有会话；新会话由 `insert_session` 显式写入。`default_workdir` 存用户写法
（绝对路径或 `~/` 开头），由使用方展开。

### 4.1 类型

```rust
pub struct PersonaId(pub i64);

pub struct Persona {
    pub id: PersonaId,
    pub name: String,
    pub prompt: String,
    pub builtin: bool,
    pub deleted: bool,
}

pub struct Settings {
    pub default_persona: PersonaId,
    pub general_prompt: String,
    /// 用户写法：绝对路径或 `~/` 开头。
    pub default_workdir: String,
    pub max_turns: u32,
}

/// 认领时定下的本轮设置，与快照列一致。
pub struct RunSettings {
    pub persona: Persona,
    pub general_prompt: String,
    pub max_turns: u32,
}

pub struct Session { /* 现有字段 */ pub persona_id: PersonaId }

pub struct NewSession { /* 现有字段 */
    /// `None` = 建会话时的默认人设（同一事务读取）。
    pub persona: Option<PersonaId>,
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("人设不存在")]
    PersonaNotFound,
    #[error("内置人设不能修改或删除，可以复制后编辑")]
    Builtin,
    #[error("这是默认人设：先在对话偏好里换一个默认，再删除它")]
    IsDefault,
    #[error("已有同名人设「{0}」")]
    NameTaken(String),
    #[error("人设已删除，请重新选择")]
    Deleted,
    #[error(transparent)]
    Store(#[from] StoreError),
}
```

### 4.2 方法

| 方法 | 规则 |
|---|---|
| `settings() -> Result<Settings, StoreError>` | |
| `update_settings(Settings) -> Result<(), SettingsError>` | 默认人设须存在且未删除 |
| `personas() -> Result<Vec<Persona>, StoreError>` | 未删除，内置在前，其余按 id |
| `persona(PersonaId) -> Result<Option<Persona>, StoreError>` | 含已删除 |
| `create_persona(name, prompt, now) -> Result<PersonaId, SettingsError>` | 同名 → `NameTaken` |
| `update_persona(id, name, prompt, now) -> Result<(), SettingsError>` | 不存在或已删除 → `PersonaNotFound`；内置 → `Builtin`；同名 → `NameTaken` |
| `delete_persona(id, now) -> Result<(), SettingsError>` | 不存在或已删除 → `PersonaNotFound`；内置 → `Builtin`；是默认 → `IsDefault` |
| `set_session_persona(SessionId, PersonaId) -> Result<(), SettingsError>` | 人设不存在 → `PersonaNotFound`；已删除 → `Deleted`。会话存在由调用方保证 |
| `claim_next(..) -> Result<Option<(Run, RunSettings)>, StoreError>`（改） | 同一事务读会话人设行与偏好，写快照列；会话人设已删除则取默认人设并写回会话 |
| `resolve_root_session` / `create_session`（改） | `persona: None` 时同一事务读默认 |

名字与提示词的长度、空白检查在边界（Gateway）做，store 不重复。

## 五、核心（mic-core）

- **请求组装**：`request::build(window, pwd, tools, &RunSettings)`。system prompt 顺序：
  1. `BASE_PROMPT`（系统；删去 "Be concise."，语气归人设）；
  2. 人设提示词；
  3. 通用偏好非空时：`User preferences:\n<文本>`；
  4. `Working directory: <pwd>`；
  5. 工具 `prompt_hint()`；
  6. 摘要。
- **执行**：`Engine.max_turns` 删除，`Engine::run(session, channel, run, settings)` 用 `settings.max_turns`。
  调度 worker 与 `run_once` 把 `claim_next` 返回的设置交给 `run`。
- **配置**：`[core] max_turns` 删除。写了 → 启动报错「max_turns 已移到网页 设置 → 对话偏好，请从配置删掉这一行」。
  `[gateway] workdir` 同理（文案指向「新会话默认工作目录」）。提示随下一次配置调整删除。
- **`Kernel` 新增**（均委托 store，错误经 `KernelError`）：

```rust
pub fn system_prompt(&self) -> &'static str;   // BASE_PROMPT，设置页只读展示
pub async fn settings(&self) -> Result<Settings, KernelError>;
pub async fn update_settings(&self, s: Settings) -> Result<(), KernelError>;
pub async fn personas(&self) -> Result<Vec<Persona>, KernelError>;
pub async fn persona(&self, id: PersonaId) -> Result<Option<Persona>, KernelError>;
pub async fn create_persona(&self, name: String, prompt: String) -> Result<PersonaId, KernelError>;
pub async fn update_persona(&self, id: PersonaId, name: String, prompt: String) -> Result<(), KernelError>;
pub async fn delete_persona(&self, id: PersonaId) -> Result<(), KernelError>;
pub async fn set_session_persona(&self, session: SessionId, persona: PersonaId) -> Result<(), KernelError>;

pub enum KernelError {
    Store(StoreError),
    Settings(SettingsError),   // 新增
}
```

## 六、Gateway（mic-gateway）

`App.workdir` 删除。建 Web 会话时读 `settings().default_workdir`，展开 `~/`，不存在则创建（失败 → 500，日志写原因）。

| 接口 | 请求 | 响应 |
|---|---|---|
| `GET /api/settings` | | `{default_persona_id, general_prompt, default_workdir, max_turns, system_prompt}` |
| `PUT /api/settings` | `{default_persona_id, general_prompt, default_workdir, max_turns}` | 204 |
| `GET /api/personas` | | `{items: [{id, name, prompt, builtin}]}` |
| `POST /api/personas` | `{name, prompt}` | 201 `{id}` |
| `PUT /api/personas/{id}` | `{name, prompt}` | 204 |
| `DELETE /api/personas/{id}` | | 204 |
| `PUT /api/sessions/{id}/persona` | `{persona_id}` | 204；只读会话 403 |
| `POST /api/sessions`（改） | `{text, persona_id}` | 不变；人设已删除 → 409 |
| `POST /api/sessions/{id}/messages`（改） | 不变 | 不检查人设；已删除的下一轮改用默认人设 |
| `SessionItem`（改） | | 加 `persona_id` |

边界校验（`limits.rs`）：名字去首尾空白后 1～`PERSONA_NAME_MAX_CHARS`（40）字；人设提示词非空且
≤ `PROMPT_MAX_CHARS`（8000）字；通用偏好 ≤ 同上限、可空；`default_workdir` 绝对路径或 `~/` 开头；
`max_turns` 在 1～`MAX_TURNS_LIMIT`（500）。不合法 → 400，中文说明哪一项、怎么改。

错误映射：`PersonaNotFound` → 404「人设不存在」；`Builtin` → 403；`IsDefault`、`NameTaken`、`Deleted` → 409；
文案取 `SettingsError` 的 Display。`ApiError::NotFound` 改为带资源说明。

## 七、前端（web/）

- 路由加 `/settings`；侧栏底部「设置」入口。
- `state/settings.svelte.ts`：设置与人设列表，登录后加载，编辑后刷新。
- `SettingsView.svelte`：对话偏好表单（保存按钮，未保存离开提示）、人设列表与编辑器。内置人设只读 +
  「复制并编辑」（新建「<名字> 副本」并打开）；删除前确认；服务端 409 的文案原样显示。
- `PersonaPicker.svelte`：放进 `Composer` 发送按钮左侧。
  - `ChatView`：值取 `view.info.persona_id`，改动即 `PUT`，成功后更新 `info`；`executingRun` 非空时显示「下一轮生效」；
    id 不在人设列表 → 选择器显示默认人设并提示「原人设已删除，下一轮改用默认人设」，不禁用发送（`settingsStore.resolve`）。
  - `NewChat`：初值为默认人设，随首条消息提交。
- 新增接口与 `SessionItem.persona_id` 全部经 `decode.ts` 解码。

## 八、调用方逐项

| 调用方 | 变化 | 兼容 |
|---|---|---|
| `scheduler::worker`、`Assembly::run_once` | 取 `claim_next` 的设置传给 `Engine::run` | 编译期强制 |
| `Assembly::run_once` 的 `NewSession` | `persona: None` | 同上 |
| `gateway::create_session` 的 `NewSession` | `persona: Some(请求值)`；`pwd` 改读设置 | 同上 |
| `row::session`、`SESSION_COLS` | 读 `persona_id` | 同上 |
| `Assembly::new` / `CoreConfig`、`gateway::config` | 删 `max_turns` / `workdir`，旧写法报错 | 现有配置需删两行；`default-config.toml` 同步 |
| 前端 `SessionItem` 解码器 | 加 `persona_id` | 前后端同版本发布 |
| 文档 | mic-store（schema v2、方法、调用方表）、run-execution（§4.4 分块、§五 配置、Engine）、gateway（接口表、配置）、web-ui、mic-core-module（配置）、storage-and-models §二（「会话当前模型从调用反推」作废，改为会话保存选择） | |

## 九、副作用、依赖与验收

- 新增写入只在 core 表；无新依赖；迁移 core v1 → v2 在一个事务内。
- 验收（开发库副本 + 浏览器）：
  1. 旧库启动：迁移成功，已有会话显示默认人设；`config.toml` 留着 `max_turns` 时启动报错并指明删除。
  2. 设置页改通用偏好与默认工作目录，新建会话：新目录生效，旧会话目录不变；库里新 run 快照含通用偏好原文。
  3. 内置人设不能保存修改；复制后编辑保存；同名报错。
  4. 回复进行中切换人设：显示「下一轮生效」；该 run 快照为旧人设，下一 run 为新人设。
  5. 删除默认人设被拒；删除某会话选着的自建人设：会话提示重选、发送被拒；重选后正常。
  6. 模型 `base_url` 指向本地记录请求的假服务，核对 system prompt 顺序符合 §五。
  7. 刷新页面后选择保持；CLI 会话无选择器。

## 十、阶段二草案：模型设置

阶段一落地后细化成正式契约。已定方向沿用 storage-and-models §二，以下为结构上的决定：

- **模型条目**入库 `core_models`（名字、`kind`、该 kind 自己解析的配置 JSON、key 来源、软删除）；
  设置里的默认模型、会话的 `model_id`、run 快照（条目 id 与名字；请求模型名已在 `core_model_calls`）
  与人设完全同构。
- **Provider 工厂**：Module 登记 `kind` 对应的工厂（校验配置 + 由条目创建 Provider），运行期按 run
  的快照创建实例；删除装配期按 provider 数量反推类别的逻辑与 `[models]` 解析。core 不解析服务商字段。
- **key**：`Stored(密文) | Env(变量名)` 显式二选一；本机主密钥文件（0600，不在数据目录）+ AES-GCM；
  Web 只写不读。新依赖（AES-GCM）在阶段二契约里列出。
- **首次运行**：没有模型时服务照常启动，网页提示先添加模型；`-p` 报错指向设置页。
- 待定：工厂 trait 签名、配置 JSON 版本化、Web 表单是否由 kind 描述字段、实例是否缓存。

## 附录 A：内置人设

提示词全文只在源码文件里维护（`crates/mic-store/src/personas/`），此处不复制。

| id | 名字 | 文件 | 用途 |
|---|---|---|---|
| 1 | 默认（兜底） | `default.md` | 平衡的日常助理 |
| 2 | 大肥鱼 | `whale.md` | 角色人设 |
| 3 | 理性大脑 | `rational.md` | 理性讨论搭档（草稿，待定） |
