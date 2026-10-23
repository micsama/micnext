# B2：上下文压缩、会话指令与撤回

2026-10-22 修订：文件快照改为必读路径清单，见 [压缩只保留必读文件路径](compaction-file-paths.md)。

**状态：CLOSED（2026-10-12，human 批准后已实现；fmt/clippy、Web 检查/构建与隔离模型 12 项链路验收通过）。真实使用、浏览器交互与 OpenAI API key 实测由 human 验收通过（2026-10-19）。**

来源：[已收敛 B1](../../brainstorm/context-compaction.md)。实施目录：`.worktree/localdev`；基线：master `c4100f3`。

## 一、范围与实测

自动压缩、`/compact [文字]`、`/clear` 共用会话 worker；压缩后保留累计摘要、最近 N 个完整 step 和 read 工具读取的文件快照。Web 展示结果并可撤回当前生效的压缩，边界置为失效，不删除记录。

2026-10-12，以短合成历史、一个 read 形状工具、末尾总结指令、`tool_choice: none`、流式请求实测：

| 上游 | 模型 | 结果 |
|---|---|---|
| DeepSeek Chat Completions | deepseek-flash，low | HTTP 200，finish_reason=stop，input_tokens=87；完整中文摘要，工具调用 0 |
| ChatGPT 订阅 Responses | gpt-6-luna，low | HTTP 200，response.completed，input_tokens=124；完整中文摘要，工具调用 0 |
| OpenAI API key Responses | gpt-5-mini，low | HTTP 401，invalid_issuer；未进入参数验证，不能判定接受或拒绝 tool_choice |

复现：`python3 scripts/probe-tool-choice.py`。脚本只发送内置合成内容；凭据分别来自对应环境变量和本机 Codex auth，不输出凭据、不修改应用库。实测只证明上述请求接受参数并产出摘要，不证明长中文历史的摘要质量或缓存命中。

OpenAI API key 的独立实测待有效凭据；不因此增加 fallback 或猜测该上游已通过。中文长历史质量与实际缓存收益留在实施验收。

## 二、依赖与模块

不新增 crate、跨 crate 依赖或常驻服务。依赖维持 message ← store/tool/media ← core ← Gateway/Channel/provider；core 不依赖 mic-tool-fs。

新增私有文件：core `command.rs`（统一指令语法）、`compaction.rs`（总结与快照）、`context_usage.rs`（用量估算）；store `context.rs`（边界事务、窗口读取）；Gateway `context.rs`（结果与撤回 HTTP）；Web `ContextBoundaryView.svelte`（展示摘要和撤回）。各自 `lib.rs` 只 re-export 公共契约。现有 `request.rs` 继续拥有请求组装，`run.rs` 继续拥有唯一执行主路径，`scheduler.rs` 继续只串行认领会话工作。

## 三、消息与强类型契约（mic-message）

```rust
pub enum SessionCommand {
    Compact { text: Option<String> },
    Clear,
    RevokeCompaction { boundary_id: MessageId },
}

pub struct Compaction {
    pub summary: String,
    pub keep_from: Option<MessageId>,
    pub files: Vec<FileSnapshot>,
}

pub struct FileSnapshot {
    pub path: String,
    pub outcome: FileSnapshotOutcome,
}
pub enum FileSnapshotOutcome {
    Read { text: String, extent: SnapshotExtent },
    Unavailable { reason: String },
}
pub enum SnapshotExtent { Complete, Truncated }

pub enum ContextBoundary {
    Compaction(Compaction),
    UserClear,
}
```

`MessageBody` 新增 `Command { person: PersonId, command: SessionCommand }`。一条指令对应一次入站，指令文字只在该记录保存；不再额外制造相同文字的 UserInput。

`Command::Compact { text: Some(..) }` 的模型视图为带现有用户头的原文字，其他指令的模型视图为 None。待认领指令不进入模型窗口；总结请求明确排除本次待回答输入，因此不会把 compact 文字混入被压历史。压缩成功后，此文字进入正常请求，和普通输入走同一执行路径。

`keep_from=None` 表示不保留旧 step 原文；非空切点必须是当前有效窗口中的 Reply 起点。若历史不足 N 个 step，全部保留已有 step；N=0 可用。摘要描述较早历史，保留原文覆盖近期细节。失效状态不塞进摘要，也不改写模型调用成败。

当前 schema 基线为 core v10；旧库拒绝打开并提示重建，见 run-trace.md。Compaction 不存附加文字副本，调用输入由 `ModelCallContext.instruction` 指向原消息。

## 四、入站与队列（mic-core / mic-store）

core 新增公共契约：

```rust
pub struct CommandSpec {
    pub name: &'static str,
    pub usage: &'static str,
    pub description: &'static str,
}
pub enum CommandParseError { UnexpectedArguments { command: String } }
pub fn session_commands() -> &'static [CommandSpec];
pub fn parse_session_command(text: &str)
    -> Result<Option<SessionCommand>, CommandParseError>;

impl Kernel {
    pub async fn append_session_command(
        &self, session: SessionId, person: PersonId, command: SessionCommand,
    ) -> Result<MessageId, KernelError>;
}
```

语法：去掉首尾空白后只识别完整的 `/compact`、`/compact <文字>`、`/clear`、`/stop`（[run-stop](run-stop.md)，不入队）；`/clear`、`/stop` 带参数报错；未知 slash 文本仍是普通输入。不提供撤回 slash 指令，不以 action 字符串分发执行。

Channel/Gateway 只对原始纯文本入站调用统一 parser，引用内容、附件说明不参与命令识别；带图片或引用的消息维持普通消息语义。微信在拼接引用之前识别，成功则调用 `append_session_command`，否则保持现有入站路径。Web 创建会话和发送消息两处都调用同一 parser。`append_recorded_input` 的 held 消息保持仅记录；一次性 `-p` 保持普通文字，不增加 CLI 命令接口。

store 新增：

```rust
pub enum RunInput {
    Conversation { message_ids: Vec<MessageId> },
    Command { message_id: MessageId, command: SessionCommand },
}
// ClaimedRun 新增 pub input: RunInput；run/settings/model 保持。
impl Store {
    pub async fn append_command(
        &self, session: SessionId, person: PersonId,
        command: SessionCommand, now: i64,
    ) -> Result<Message, StoreError>;

    // 原 bool 返回值迁移为实际并入的输入 id，供 Exec 维护待回答输入。
    pub async fn absorb(&self, run: RunId) -> Result<Vec<MessageId>, StoreError>;
}
```

`claim_next` 签名保持；待认领集合加入 Command。开头若是指令，只认领该条；若是普通输入，只合并开头同 person 的连续段，遇到任何指令即停止。`absorb` 遇到开头指令返回空，不越过它吸收后面的输入。指令也使用现有 run 行和设置/模型快照；clear/revoke 执行不需要可用 Provider，不因模型缺失而失败。

`Engine::run` 统一发布 RunStarted / RunFinished、写既有终态和取消收尾。普通输入直接进入现有 Exec；compact 命令先总结，有文字再进入同一个 Exec；clear/revoke 执行存储事务后结束。纯指令不新建 agent runner，不新增 RunState。压缩失败收尾 ProviderFailed；正常处理的 clear/revoke 收尾 Completed。

`Exec` 私有地持有本次待回答的输入 id：来自认领结果与 absorb；普通模型调用成功后清空，工具结果归历史。每次正常请求前，先从当前有效窗口取掉待回答输入，对其余历史判断压缩；自动压缩时待回答文字合成 extra，但保留原始输入消息供压缩后正常请求使用。待回答图片不作为文件清单或摘要原文自动读取，压缩后仍按已有图片路径发送。

启动遗留处置扩大到指令：未认领指令与普通输入一样 held，不自动重放副作用；已认领但未完成的指令 run 按现有规则 Interrupted，不自动重跑。held clear/revoke/纯 compact 不进入模型；held compact 文字按其用户视图可见，但不会触发压缩副作用。停止仍靠丢弃 worker/Provider stream/工具 future；不新增带 ack 的内部命令通道。

## 五、存储与有效窗口

新增公共类型与方法（store）：

```rust
pub enum CompactionState { Applied, Revoked { at: i64 } }
pub struct CompactionView {
    pub boundary_id: MessageId,
    pub result: Compaction,
    pub state: CompactionState,
}
pub struct ContextView {
    pub active_boundary: Option<MessageId>,
    pub compactions: Vec<CompactionView>,
}
pub struct ModelCallContext {
    pub boundary_id: Option<MessageId>,
    pub through: Option<MessageId>,
}
pub struct ContextUsage {
    pub input_tokens: u64,
    pub output_tokens: u64,
}
// NewModelCall 新增 pub context: ModelCallContext。
// ContextWindow 新增 boundary_id: Option<MessageId>、files: Vec<FileSnapshot>；summary/messages 保持。
// Store::context_usage(session) -> Option<ContextUsage>（修订 2026-10-15，见下文触发口径）。

pub enum ContextCommandError {
    BoundaryNotFound,
    NotCurrentCompaction,
    Store(StoreError),
}

impl Store {
    pub async fn context_view(&self, session: SessionId)
        -> Result<ContextView, StoreError>;
    pub async fn commit_compaction(
        &self, session: SessionId, run: RunId, result: Compaction,
        unanswered: Vec<MessageId>, call: NewModelCall, now: i64,
    ) -> Result<Vec<Message>, StoreError>;
    pub async fn clear_context(
        &self, session: SessionId, run: RunId, command_id: MessageId, now: i64,
    ) -> Result<Vec<Message>, StoreError>;
    pub async fn revoke_compaction(
        &self, session: SessionId, run: RunId, boundary_id: MessageId, now: i64,
    ) -> Result<Vec<Message>, ContextCommandError>;
}
```

schema core v6：

- `core_models.context_window` 可空正整数，空按 100000。
- `core_settings` 和 run 设置快照新增 `compaction_trigger_percent`、`compaction_keep_steps`。
- `core_messages.compaction_revoked_at` 可空；只用于 Compaction 的失效事实，UserClear 不可失效。
- `core_boundary_inputs(boundary_id, message_id)`，复合主键及消息外键：保存跨过边界落盘位置、仍需正常处理的输入 id。
- `core_model_calls.context_boundary_id`、`context_through_id`，可空消息外键：记录该请求基于哪个有效窗口、当时读到的历史位置。压缩调用 run_id 仍为空；正常调用 model_id 从关联 run 快照读取。
- 待认领索引纳入 Command；旧 payload 做本节三规定的完整迁移；原 Boundary 全部有效，不丢历史。

窗口读取永远取最新有效 Boundary（UserClear 或未撤回 Compaction）。Compaction 的普通历史范围为 `id >= keep_from`，无 keep_from 时只读边界之后；UserClear 只读边界之后。再合入该边界 `core_boundary_inputs` 指定的输入，去重、按 id 排序，仍排除尚待调度的输入，保留 held 输入的既有语义。

边界例外输入由 store 在边界提交事务内收集，不让 Channel 猜顺序：

- clear：在指令之后已入队且尚未认领的输入/指令。此前 run 的最终输出已在 clear 执行前落盘，不能因输入在队列里的位置较早而被恢复。
- compact：本次 unanswered，加上已经入队尚未认领的后续输入/指令。重复 compact 从当前有效窗口选择历史，已回答且不在保留范围的旧例外输入不继续携带。

这张关联表只承载“哪些输入跨过物理边界仍可见”这一事实，消费者只有 `context_window`。不移动消息 id，不复制输入内容，不给 Boundary 建 epoch 或生命周期管理器。保留 step 的 Reply 起点不会回到已清空历史；原输入只由该关联显式保留。

`commit_compaction` 单事务写无 Reply 的调用记录、Boundary、例外输入和成功通知；返回新增消息按 id 排列，供原 MessageAppended 发布。通知使用实际保留步数。失败不创建 Boundary：沿现有调用记录及通知路径记录原因，原窗口不变。总结生成与文件读取在事务外，事务内不等待网络或工具。

`clear_context` 只写空 UserClear、例外输入及清空通知，不执行文件读取或模型调用。

撤回事务确认目标属于该会话且为当前生效 Compaction，否则返回对应错误。成功将 revoked_at 写为当前时间，同时落撤回通知；调用记录和摘要快照不改。切回上一有效 Boundary 的窗口；撤回边界关联的例外输入合入上一有效边界的关联，使已经排队、后来已认领的输入不会因为切回旧物理切点而丢失；没有上一边界时全部历史按既有资格读取。后续消息仍保留，clear 始终阻断更早上下文。

重复撤回同一个失效目标是 NotCurrentCompaction，不算第二次成功。入队时可以拒绝已知失效目标；执行时仍按当前边界做这一领域判定。排队期间目标被新 Boundary 替代时，落“未撤回，目标已非当前压缩”的通知并 Completed，窗口不变；存储错误仍 Err 并按现有二进制收口。

## 六、用量、总结与 read

`ModelRequest` 新增 `pub tool_choice: ToolChoice`；公共 enum `ToolChoice { Auto, None }`。正常请求用 Auto（wire 可省略默认值）；总结请求保留原 tools，显式发 `"none"`。不以工具数组是否为空承载选择策略。此次不顺手改变 max_turns 末次请求的既有行为，审计 D2 另行处理。

ModelCallContext 由请求组装侧生成：读到的有效 Boundary 和历史最大消息 id；失败重试复用相同请求事实。`Store::context_usage` 只取当前有效 Boundary 下最近一次成功普通调用的上游 input_tokens + output_tokens；压缩调用用量测量的是旧历史，不算。

**触发口径（修订 2026-10-15）**：每次对话调用返回后立即判定，这次返回的 input + output 达到窗口 × 触发比例即压缩，只压到这次回复为止；之后的工具结果或新输入压完再接上发出，调用前不再判定。回复带工具调用时它原样保留（`keep_from` 不晚于它），保证结果接上时调用仍在窗口内；不估算新增内容，余量由触发比例（默认 80%）留出。边界后尚无调用（`/clear`、压缩后）不触发。Web 占用显示同一个数（[chat-run-stats](chat-run-stats.md) §二）。按字数的估算只留给下文的裁剪：ASCII 文本每 3 字节向上取整，非 ASCII 每字符 2 token，图片每张 4096 token，参数在 `core/limits.rs`。

默认窗口 100000，触发 80%，保留 15 step；设置页触发比例接受 1..=80，保留数 0..=100。触发后总结当前有效历史；自动压缩发生在返回之后，没有附加文字，只有 `/compact <文字>` 带 extra。历史超过窗口 80% 时从最早完整 step 裁剪到估算满足预算；不按历史调用的 token 差值估算 step 大小，也不引入大文件专项算法。保留 N 是原文保留策略，不以假想的大文件情况增加动态 N 机制；裁剪不改变真相层原始历史。

总结复用 request::build 的 system/tools/images/cache_key，末尾追加临时 HarnessNote。该 note 和原始模型输出都不存为消息，增量不向 Channel/Web 发布。正常请求事件、落盘、取消路径不变；总结消费相同 Provider stream，但只消费 Finished/Err，不让摘要成为用户回复草稿。请求失败沿用既有 Provider 错误/重试分类与次数。

正文非空且 StopReason 为 EndTurn 或 ToolUse 时提取 Text；ToolCall 丢弃、warn，不执行；MaxTokens、ContentFilter、空正文或 Provider Err 压缩失败。不检测正文是否“像拒答”或摘要是否充分；用户通过结果展示与撤回处理质量问题。

固定总结节：用户诉求、关键结论与决定、涉及文件、错误与修正、未完成事项、当前进度；末尾可选 `<required_files>` 块，每行一个路径，闭合后不带其它清单内容。边界一次解析为路径列表；无块为零文件，有起始标记却未闭合报总结格式错误。其余正文保留为 summary，不把路径清单再作为第二份独立事实展示。

core 从当前会话开放的 ToolHandle 中选择 read，按已有 `file_path` 参数调用，ToolContext 使用会话 pwd，工具侧负责参数和路径权限。core 不调用 std::fs 自动读取。没有 read、路径不可读或工具报错，保存 FileSnapshotOutcome::Unavailable 原因，压缩仍可成功；read 成功文本存快照。最多 8 个文件、总文本 64 KiB，超限按 UTF-8 边界截断并保存 Truncated，未读取项记录超限原因。快照通过 request 的 context 块注入，明确为压缩时的文件内容，后续每次请求稳定复用。

`NewModelCall` 继续使用现有 ModelCallOutcome；总结成功以 `Replied { blocks: [] }` 存无 Reply 调用行，usage 独立记录，结果只在 Boundary。撤回绝不修改这行的成败与用量。调用目的使用 `ModelCallPurpose::Compaction`。解析失败仍记上游成功，失败由通知与 run 状态表达。

## 七、设置、HTTP 与 Web

模型 `ModelDraft`（core）、`ModelWrite/ModelView`（store）、`ClaimedModel::Selected` 新增 `context_window: Option<NonZeroU32>`。模型设置 HTTP 创建/更新/读取新增 `context_window: number|null`；缺省/null 表示默认 100000，页面展示默认，不存入 config_json，也不作为 Provider 参数。

Settings/RunSettings 新增 `compaction: CompactionSettings`，公开结构包含 `trigger_percent: u8`、`keep_steps: u32`；settings HTTP 在已有偏好字段旁新增 `compaction: {trigger_percent, keep_steps}`。Gateway 在 HTTP 边界校验上述范围，数据库存同一事实，认领写 run 快照。非法值 HTTP 400；非法模型窗口 HTTP 422；配置不进入 config.toml。

core 新增 `KernelError::CommandParse(CommandParseError)`、`KernelError::ContextCommand(ContextCommandError)`，及：

```rust
impl Kernel {
    pub async fn context_view(&self, session: SessionId)
        -> Result<ContextOverview, KernelError>; // ContextView + occupancy，见 chat-run-stats §二
}
```

Kernel::append_session_command 的 RevokeCompaction 在入队边界确认目标存在且当前可撤回，成功只代表入队，不等待 worker。没有新增 ack 等待路径。

HTTP（保持现有 token 与会话 writable 边界）：

| 接口 | 请求 / 响应 | 错误 |
|---|---|---|
| GET `/commands` | `{items:[{name,usage,description}]}`，compact/clear/stop | 现有鉴权错误 |
| GET `/sessions/{id}/context` | `{active_boundary:number|null, compactions:[{boundary_id,result,state}], occupancy}`（occupancy 见 [chat-run-stats](chat-run-stats.md) §二） | 会话不存在 404 |
| POST `/sessions/{id}/compactions/{boundary_id}/revoke` | 无 body；202 无响应体 | 非 Web 会话 403；目标不存在/不属于会话 404；目标不是当前生效压缩 409 |

CompactionState JSON 用内部 tag：`{kind:"applied"}` / `{kind:"revoked",at:number}`。其他新增消息类型和 boundary 按 mic-message 既有 serde 方式完整接收；Gateway 不只挑几个摘要字段省略其余。

不用新增 SSE event：边界新增及成功/失败/撤回通知均走 MessageAppended；run 始终走原有开始/结束事件。边界 payload 保持不可变，失效时间由 context_view 提供，避免改旧消息后增量游标漏更新。Web 在 ready、每次 message/Boundary/Notification/Command、run_finished 时按需合并刷新 context_view；重连 ready 也刷新，页面不依赖通知字符串猜状态。会话元数据仍按既有接口读取，不塞第二份 context 状态。

Web 分隔条显示「✓ 已压缩」，可展开 summary、文件快照、保留信息；只有 active_boundary 对应的 Applied 压缩显示撤回按钮，排队后显示处理中，最终事实从 context_view 读取。旧 Applied 记录仍显示历史成功，但没有撤回按钮；失效记录显示「已撤回」。失败通知直接展示失败原因，不凭空创建一个“失败摘要” Boundary。跨 Channel 的只读历史仍可看摘要，但不提供撤回操作，沿用当前 Web 的副作用权限。

Composer 输入 `/` 后展示统一命令清单、支持选中补全；提交仍经原 messages 接口，由 Gateway 边界解析。微信按现有通知投递显示成功/失败，不新增微信按钮。

## 八、实体、写者与关键顺序

| 实体 | 唯一执行写者 | 消费者 |
|---|---|---|
| Command 入站消息 | Channel/Gateway 经 Kernel→Store | 同会话认领、Web 历史 |
| run、认领与并入 | 会话 worker 经 Store | 现有执行、恢复、Web 运行状态 |
| Compaction、快照、例外输入与通知 | 同会话 worker 的 compact 事务 | 请求组装、上下文读取、Web 展示 |
| UserClear 与例外输入 | 同会话 worker 的 clear 事务 | 上下文读取、Web 历史 |
| revoked_at 与撤回通知 | 同会话 worker 的 revoke 事务 | 最新有效窗口、Web 撤回状态 |
| 调用及请求上下文事实 | 发起调用的 Exec/compact 经 Store | 用量基线、统计与审计 |
| 模型窗口与压缩偏好 | Web 设置经 Kernel→Store | run 认领快照与请求前判断 |

```text
输入 / 指令：Pending → Claimed(既有 run) → Completed / ProviderFailed / Interrupted
                    ↘ 启动尚未认领时 Held，不执行副作用
压缩边界：提交即 Applied → 显式撤回后 Revoked
clear 边界：提交即有效，不提供撤回
```

```text
A run 执行中 → clear 指令落盘 → B 输入落盘
A 最终 Reply 落盘 → A 完成 → worker 认领 clear
clear 事务追加空 Boundary，并记录 B 为例外输入 → 通知
worker 认领 B → context_window = clear 后消息 + 已认领 B → 普通模型请求
```

```text
普通请求前判断历史超阈值 → 保留待回答输入 id
临时总结请求 → 正文规整 → 通过 read 读取快照
事务写调用 + Boundary + 例外输入 + 成功通知 → 发布既有消息事件
重读窗口 → 原输入进入普通请求 → 用户在 Web 查看摘要、点击撤回
撤回入队 → worker 轮到时确认目标 → 原子 revoked_at + 通知
读上一有效边界与后续消息 → 下一普通请求重新按该窗口判断用量
```

不变量：同会话至多一个模型请求；tools 成组闭合；失败不移动窗口；只有最新有效 Boundary 控制上下文；新输入不被物理切点漏掉；clear 不可撤回；撤回不撤销工具结果或改写调用事实；所有副作用排队，不等待同一内部队列 ack。

## 九、全部调用方与兼容迁移

| 调用方 | 判定与迁移 |
|---|---|
| mic-message `model_view.rs` | 新 Command 需穷尽处理；仅 compact 文字映射用户视图。Boundary 保持 None |
| mic-store `store.rs/row.rs/schema.rs/models.rs/types.rs` | 改边界字段、认领集合、absorb 返回值、模型/设置/调用上下文；新增 context.rs；schema v6 原子迁移 |
| mic-core `kernel.rs/input.rs` | 原输入 API 保持；新增统一 parser 与强类型 command 入队、结果读取；两个新错误来源 |
| mic-core `scheduler.rs` | claim_next 签名保持；仍只调用唯一 Engine::run，不另建指令 worker |
| mic-core `run.rs/recovery.rs` | 消费 RunInput/并入 id；复用现有终态；Provider 前的压缩准备、command 分支；恢复仍 Interrupted，不重跑指令 |
| mic-core `request.rs` | 新窗口 fields/快照注入；填 ToolChoice 与 ModelCallContext；统一用量估算依赖 core 私有模块 |
| mic-core `assembly.rs` 一次性调用 | 适配 ClaimedRun 新字段，不解析 `-p` slash，不改 CLI 公开形状；原事件/取消保持 |
| mic-provider-openai `chat.rs/responses.rs` | 两种请求增加 tool_choice 映射，保持 tools 内容；不改变正文/拒答/用量解析与认证 |
| mic-provider-openai `factory.rs/probe.rs` | Provider stream 签名不变；消费扩展 ModelRequest；模型目录 probe 不改变 |
| mic-gateway `api.rs` | 新建/发送纯文本统一 parser；图片输入与只读边界保持；新增 CommandParse 错误映射 |
| mic-gateway `models.rs/settings.rs/error.rs/service.rs` | 扩展完整 DTO、范围验证、注册结果/撤回/命令清单路由；映射 ContextCommand 错误 |
| mic-gateway `stream.rs` | 不新增事件名；新消息按现有 message SSE 完整序列化，旧游标可重放新增 Command |
| mic-channel-wechat `service.rs` | 原始纯文本解析在 quote 拼接前；指令不丢入普通 append；同队列执行 |
| mic-channel-wechat `delivery.rs/typing.rs/quote.rs` | Command/Boundary 不投递、不进入 typing 的回复终点；Notification 保持投递；引用读取仍只处理真实 UserInput，不扩大匹配 |
| bin/micnext `main.rs` | 增量事件穷尽 match 适配新 MessageBody；命令展示为运行信息，摘要不冒充 Reply |
| web `api/types.ts/client.ts` | 完整强类型 decoder、新 boundary/files/commands/context DTO 与 API；拒绝未知状态，不静默忽略 |
| web `state/session.svelte.ts` | 用既有事件刷新 context_view，重连同步撤回；无独立 lifecycle 状态机 |
| web `components/Composer.svelte/MessageItem.svelte/MessageList.svelte` | 命令补全、Command 展示、Boundary 展开与撤回按钮；不渲染模型草稿形式的摘要 |
| web `components/settings/endpoint-form.ts/EndpointDialog.svelte/PrefsSection.svelte`、`state/settings.svelte.ts` | 模型窗口、对话压缩偏好表单完整往返，默认值显示 |
| tools（fs/shell/web-fetch）、mic-media | 契约不变；自动快照只经已开放的 read ToolHandle |
| docs `mic-store/provider-port/provider-openai/run-execution/runtime-settings/model-settings/gateway/web-ui` | 同步本项批准后的最终契约，B1/本 Blueprint/todo 一起更新 |
| 当前已有测试中的结构体构造与 match | 实施时只对受改动影响的已有测试适配并运行；不为 speculative 架构写测试 |

公开契约用 parallel change 小步推进：先增加新类型/API/列并保留原输入入口，适配各消费者；再切换边界写入与窗口读取、删除旧窗口实现。最终只保留一个 context_window 和一个 parser，不留下“旧压缩/新压缩”双路径。HTTP 字段、消息变体与 decoder 同一次构建部署；旧页面版本按已有协议不匹配处理，不制造运行期双协议。

## 十、实施顺序与验收

1. 批准后落消息/设置/模型/请求类型及 store schema v6、完整磁盘迁移，适配全部构造处保持编译。
2. 实现强类型指令认领/absorb 隔离、clear 的例外输入窗口；保持既有 run/通知/恢复路径。
3. 实现总结、快照、原子提交和普通请求前触发；适配两个 Provider tool_choice；补用量元数据。
4. 实现撤回事务、结果 API 与 Web 展示/补全/设置；适配微信。
5. 按下表手工验收，覆盖改动的已有测试才运行；`cargo fmt`、`cargo clippy -- -D warnings`、Web 项目既有检查通过后 CLOSED，更新 todo；不自行 commit。

| 场景 | 验收结果 |
|---|---|
| 普通短对话 | 不压缩、原消息/事件/投递正常 |
| `/compact` | 一次总结、无回复草稿；Boundary + 实际保留通知；summary 可展开 |
| `/compact 问题` | 总结带该文字，压完原文只作为下一次问题处理一次 |
| A 正在跑，依次 clear、B | A 先完成，clear 不调用 Provider，B 收到响应且模型不带清理前历史 |
| A 正在跑，依次 compact、B | absorb 不跨命令；先压缩再处理 B |
| 自动触发与连续压缩 | 发普通请求前判断当前历史；压完按新窗口计量；没有旧用量导致的空转压缩 |
| read 可用、关闭、读失败、超限 | 自动快照经 read；不可用原因可见；快照及截断稳定复用 |
| 上游失败、空正文、截断、异常工具调用 | 失败无 Boundary、通知原因；工具不执行；已有正文按既定规整使用 |
| 用户撤回 | 当前边界 Revoked；旧摘要/用量还在；恢复上一有效窗口和后续消息；刷新/重连状态一致 |
| 旧压缩之后有 clear | 旧边界不能撤回，不恢复清空前历史 |
| 撤回入队后目标被新压缩替代 | 执行拒绝、原因通知，边界不变 |
| 停止/重启 | 在途 run Interrupted，不重跑；未认领指令 held；已提交边界/撤回事实保留 |
| v5 库升级 | 旧 Compaction 读行为等价、全部历史仍可查看，旧库无新字段时完整迁移 |
| 上游与缓存 | DeepSeek/ChatGPT 回归；有效 OpenAI API key 单独验收；长中文摘要内容检查；同前缀未裁剪时看 cache_read_tokens |

审查仅修本次契约内缺陷；发现既有架构问题交回 human，不借本项修审计 D2、权限体系或其它模块方向。

## 十一、实施收尾记录（2026-10-12）

- `cargo fmt`、`cargo clippy -- -D warnings` 通过；`bun run check`（零错误/警告）、`bun run build` 通过。仓库当前没有覆盖本改动的已有 Rust 测试，未新增测试文件。
- 用独立临时 v5 数据库、本机合成 Chat 上游和真实 micnext HTTP 服务验收：旧摘要迁移、设置/模型完整往返、非法触发比例拒绝、短对话不压缩、纯 compact 无 Reply、read 快照、compact 文字在总结后只处理一次、撤回恢复上一边界、空摘要失败不移动边界、clear 后排队 B 不丢失、自动触发与新窗口用量、完整工具配对、撤回不改调用成败/用量、重启后失效状态保留。验收脚本输出 12 项 PASS，合成上游共收到 16 次请求；现有用户数据未改。
- **accept／本次改动缺陷**：压缩后的待回答输入必须在通知之后呈现；现已在普通请求组装后稳定移到末尾，避免模型最后看到压缩通知而非问题。Store 的历史排序与消息 id 不变。
- **accept／本次改动缺陷**：模型窗口 0 是字段语义错误；Gateway 在边界转为 NonZeroU32，按契约返回 HTTP 422。
- 原始摘要和临时总结指令不进会话消息；成功 Boundary/通知、撤回通知走既有 MessageAppended，状态变更从 context_view 读取。无新 runner、终态、SSE event 或跨 crate 依赖。
- 浏览器连接器不可用；native Safari 界面控制未获准，未声称已完成真实浏览器点击验收。human 已明确接手提交后的使用验证。
- 人工验证入口：在 localdev 构建并启动，模型设置可填上下文窗口；发 `/compact`、`/compact 下一步问题`，展开摘要后撤回；执行中依次发 `/clear` 和下一条消息，核对下一条正常回答。真实中文长历史质量、缓存命中、只读渠道与微信发送仍按 §十 验收。
