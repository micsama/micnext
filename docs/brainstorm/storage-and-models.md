# 存储结构重整与模型设置运行期化（2026-09-24）

**状态**：§一 存储重整 B1 已收敛（2026-09-24），下一步起 B2，排在 M9 网关前，网关直接按新结构起草。
§二 模型设置依赖 Web 设置页，拆出去排在 M10 之后单独起 B2。

**切换原则**：推翻重写，不留兼容。旧类型、旧表、旧命名、旧文档表述一律删除或改写，
不保留别名、双轨或"旧版为…"的说明；受影响的 blueprint 在同一次改动里改成新结构。
库版本仍为 core v1，切换前需手动删除已有库。

## 一、存储结构

### 1. 核心概念

- **session**：一段对话流，按全局序号排列的 message 序列。
- **message**：流里的一个不可变事实，**一次产出一条**，由唯一的产出者产生：

  | 种类 | 一条代表 | 产出者 |
  |---|---|---|
  | `UserInput` | 一次入站（可含多段文字/文件） | person |
  | `Reply` | 一次模型调用的全部输出（推理、正文、工具调用，按生成顺序） | model_call |
  | `ToolResult` | 一个工具调用的结果 | 那次工具调用（`tool_call_id`） |
  | `Completion` | 一个异步任务的终态回报 | 异步任务 |
  | `HarnessNote` | 给模型看的一条框架备注 | 框架 |
  | `Notification` | 投递给用户的一条框架通知 | 框架 |
  | `Boundary` | 一次上下文截断（压缩或 `/clear`） | 压缩调用 / person |

  事后可变的只有两项：输入被认领时回填 `run_id`，投递后写 `delivered_at`。
- **run**：一次 agent loop。认领一段输入开始，期间到达的同一 person 输入并入，结束于终态。
- **model_call**：一次模型调用尝试（含失败和重试），记模型与用量。

### 2. 已定

- 表名加模块前缀 `模块名_`（内核 `core_`），模块私有表同理，靠 review 守；不用 `ATTACH`
  （跨库无外键、WAL 下跨库事务不原子）。
- 事件是消息，执行是 run：用户输入、定时触发、异步完成都先作为输入消息进入 session；
  session 空闲则开新 run，有 run 在跑则并入。事件语义由消息种类表达。
- `run_id` 在消息表与调用表上同一规则：不在任何 run 内即为空，输入被认领时回填。
- 意图、压缩等旁路调用不进对话流，只在 `core_model_calls` 记一行；压缩再写一条 `Boundary`。
- 压缩保留最近几轮：`Boundary` 追加在末尾并记录覆盖范围 `until`，组装上下文为
  "摘要 → until 之后的消息"；`until` 落在一轮末尾。随压缩功能一起加，本次不建。
- 定时任务：一个任务一个长期 session，每次到点开新 run，不挂父 session；归属查定时任务
  模块自己的表。子 agent 仍挂父 session。
- 用量：上游给什么存什么，未报存 NULL，不编造 0；失败的调用没有用量，整组为 NULL。
  修掉 `mic-provider-openai/src/response.rs` `usage()` 对未报字段写 0 的缺陷。
- 字段非必要不加：`purpose`、`model_entry`、`result`、`until` 跟各自功能走 core v2 迁移加入。
- 没有生产者的一律删除：`ModelCallPurpose`、`FailureReason::Timeout`、`CancelReason` 与
  `Cancelled` 终态（取消功能落地时再加）。
- run 失败只留种类；模型失败详情看 `core_model_calls.error`。
- 同一 run 的输入属于同一 person，由该 run 首条输入推出，`core_runs` 不存 person。

### 3. 表

```
core_persons / core_person_identities / core_sessions   只改表名

core_runs         id, session_id, state, created_at, finished_at?
                  state ∈ executing | completed | provider_failed | max_turns | interrupted

core_messages     id(全局序号), session_id, run_id?, model_call_id?,
                  payload, created_at, delivered_at?,
                  kind       生成列  ← payload 的变体名
                  person_id  生成列  ← payload 里 UserInput/Completion 的 person

core_model_calls  id, session_id, run_id?, model, error?,
                  input_tokens?, output_tokens?, cache_read_tokens?, cache_write_tokens?,
                  reasoning_tokens?, started_at, finished_at
```

- `payload` 是 `MessageBody` 的 serde，唯一真相；`kind`、`person_id` 只是为索引和认领派生的
  生成列，写入方不填。例外只有 `Reply.model`：它是那次调用的事实，只存
  `core_model_calls.model`，读出时按 `model_call_id` 主键 JOIN 还原。
- `core_runs.state` 一列闭集，不再有 JSON `reason`。
- `core_model_calls`：`error IS NULL` ⇔ 成功；用量各列上游没报即 NULL，成功调用也可能为空。
- 索引：`core_messages(session_id, id)`；未认领输入 `(session_id) WHERE run_id IS NULL AND
  kind IN ('UserInput','Completion')`；待投递 `(session_id) WHERE delivered_at IS NULL AND kind IN
  ('Reply','Notification')`；`core_runs(session_id) WHERE state = 'executing'`。

### 4. 消息协议（mic-message）

```rust
pub struct MessageId(pub i64);

pub struct Message {
    pub id: MessageId,
    pub session_id: SessionId,
    pub body: MessageBody,
    pub created_at: i64,
    pub delivered_at: Option<i64>,
}

pub enum MessageBody {
    UserInput { person: PersonId, parts: Vec<ContentPart> },
    Reply { model: String, blocks: Vec<ReplyBlock> },
    ToolResult { tool_name: String, tool_call_id: String, outcome: ToolResultOutcome },
    Completion { person: PersonId, tool_name: String, exec_id: String, outcome: ExecOutcome },
    HarnessNote { text: String },
    Notification { source: String, text: String },
    Boundary(ContextBoundary),                 // Compaction { summary } | UserClear
}

pub enum ReplyBlock {
    Reasoning(Reasoning),
    Text { text: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
}
```

删除：`MessageAuthor`、`MessageContent`、`SessionEntry`、`BoundaryEntry`、`SessionEntryId`。
原来"作者 × 内容"两个枚举不正交（`User + ToolCall` 可表示但不存在），可认领判别、
`model_view` 都要按组合匹配；合成一个闭集后非法组合不可表示，判别变成单一 `kind`。
`run_id` / `model_call_id` 只是库内列，不进 `Message`；网关要按 run 分组时在 M9 B2 再加。

`ModelView` 随之简化：`Reply` 直接是一个 assistant turn；Provider 删掉 `AssistantTurn`
累积与"连续 assistant 合并"；`Boundary` 不进上下文。

### 5. 写入与事件

- store 写入口收成三个，按产出者分：
  - `append(session, run: Option<RunId>, body)`：除 `Reply` 外的所有种类。
  - `record_model_call(call) -> (ModelCallId, Option<Message>)`：`call.outcome` 为
    `Replied { usage, blocks }` 时同一事务内写调用行和 `Reply`；`Failed { error }` 只写调用行。
    "回复必有调用"由事务保证。
  - `claim_next(session)` / `absorb(run)`：回填输入的 `run_id`。
- 内核事件：`RunStarted`、`TextDelta`、`ReasoningDelta`、`DraftDiscarded`、`MessageAppended`、
  `RunFinished`。一次调用一条消息后，成功时 `Reply` 的 `MessageAppended` 就是草稿的终点，
  只有失败（含重试前）需要 `DraftDiscarded`；删去 `DraftEnded`。
- 投递：`Reply` 由 Channel 取其中 `Text` 块发送；没有正文的回复（只有工具调用）Channel 直接标已投递。

### 6. 实体与写者

| 实体 | 写者 | 写入时机 |
|---|---|---|
| core_sessions | Kernel | 入站建/取 Root、建子会话、改 pwd |
| core_runs | 调度 worker | 认领时插入（直接 `executing`） |
| | run 引擎 | 结束时写终态 |
| | 启动恢复 | 遗留 `executing` → `interrupted` |
| core_messages | Kernel 入站 | `UserInput`、`Completion`、`/clear` 的 `Boundary`（run_id 空） |
| | 调度 worker / run 引擎 | 认领、并入时回填输入的 `run_id` |
| | run 引擎 | `Reply`（经 `record_model_call`）、`ToolResult`、`HarnessNote`、`Notification` |
| | 启动恢复 | 补 `ToolResult`、中断通知（run_id = 被中断的 run） |
| | Channel 投递 | `delivered_at` |
| core_model_calls | run 引擎 | 每次尝试一行（含失败重试） |

### 7. run 状态与时序

```
认领（事务：会话无 executing 且有未认领输入）
   ▼
executing ──并入同 person 新输入──▶ executing
   ├─ 模型说完且无新输入 ─▶ completed
   ├─ 模型失败（不可重试或重试用尽）─▶ provider_failed
   ├─ 轮次用尽（已做无工具总结）─▶ max_turns
   └─ 进程重启时仍在 executing ─▶ interrupted（启动恢复写）
终态不再变化。
```

插队：

```
Web ─append(UserInput)─▶ INSERT m1(run_id NULL) ─▶ wake(S)
worker ─claim_next(S)─▶ 事务: 无 executing → INSERT run R → UPDATE m1.run_id = R
引擎 R: 读上下文（store 已排除未认领输入）→ 调模型
        → record_model_call: INSERT call C1 + INSERT Reply(C1, R) → 工具 → append(ToolResult, R)
   ‖ Web 发 m2 → INSERT m2(NULL) → wake(S) → 调度标 Dirty
引擎 R: 下一轮前 absorb(R) → UPDATE m2.run_id = R（同 person 才并）→ 读上下文 → …
        → 无工具且无新输入 → state = completed
worker: claim_next → 无 → 退出；Dirty → 重起 → 无 → 结束
```

启动恢复：

```
事务: runs executing → interrupted，取出这些 run
逐个 R: 取 run_id = R 的消息 → Reply 里的 ToolCall 块 减去 已有 ToolResult
        → 每个补 ToolResult(cancelled, R) → 补 Notification(R)
之后才启动调度与订阅。
```

### 8. 不变量

1. 每个会话至多一个 `executing` run（认领事务保证）。
2. 输入的 `run_id` 只由认领/并入从空填成非空，之后不变；同一 run 的输入属于同一 person。
3. 未认领输入 = `kind ∈ {UserInput, Completion}` ∧ `run_id IS NULL`；上下文组装由 store 排除它们。
4. `model_call_id` 只出现在 `Reply` 上，指向一次成功调用，二者 `run_id`、`session_id` 相同。
5. run 进入终态时，其每个 `ToolCall` 块恰有一条同 run 的 `ToolResult`（恢复负责补齐）。
6. `run_id` 非空的消息与调用，`session_id` 等于该 run 的 `session_id`。

### 9. 模型名与入站粒度（已定）

1. **调用记录的模型名统一为请求时的上游模型名**。现状两种口径混在一列：成功时存上游
   回报的名字，失败时存 `[models]` 条目名。改为统一存调用前就已知的请求模型名，失败也有；
   Provider 判断推理能否回传也拿它比。上游回报的名字（可能带版本后缀）不存。
   请求模型名由谁提供（`Provider` 暴露，或 core 从模型条目取得）在 B2 里定；§二 落地后
   自然由模型条目给出。
2. **`UserInput` 一次入站一条**，内容为 `Vec<ContentPart>`，文字和文件同条，取代现在
   文字、附件各自一条。

### 10. 公开契约变更与调用方

| 位置 | 变更 | 调用方 |
|---|---|---|
| mic-message | §4 整体替换；`ModelView` 按 `MessageBody` 重写 | mic-store row、mic-core run/request/recovery/kernel、provider-openai request、bin `-p` |
| mic-store 类型 | `Query*` → `Run*`（`RunId`、`RunState`、`Run`）；`Usage` 除输入输出外 `Option`，失败调用无 `Usage`；删 `ModelCallPurpose`、`FailureReason`、`CancelReason`、`UserInput`/`CompletionInput`/`OutputInput`/`BoundaryInput` | mic-core 全部读写路径 |
| mic-store 方法 | §5 三个写入口；`finish_run`、`executing_runs`/`interrupt_run`、`run_messages`、`messages_after`、`context_window`（排除未认领输入与边界） | scheduler、run 引擎、recovery、assembly、bin |
| mic-core | 事件按 §5；`query.rs` → `run.rs`；`request::order` 去掉可认领判别；`append_user_input` 改收 `parts` | bin（`-p`）、将来 M9 |
| provider-openai | 请求映射改按 `Reply`；`usage()` 未报给 `None` | — |
| 文档 | 改写 mic-message、mic-store、query-execution（改名 run-execution）、provider-port、provider-openai、mic-tool（Completion 引用处）、v0a-module-map | — |

## 二、模型设置运行期化（M10 后单独 B2）

- 模型条目（Provider 地址、预设、参数、key）存库，只经 Web 增删改；`config.toml` 的
  `[models]` 在新路径落地的同一次改动里删除。
- run 开始时读取外部选择的模型；~~会话当前模型从最近一次调用反推~~（2026-10-08 作废，见
  runtime-settings：会话保存下一轮选择，run 记当轮事实）；新会话用模型列表里标为默认的条目（store 保证恰好一条）。
- key 用本机主密钥文件（0600，不在 data_dir）+ AES-GCM 加密入库，只防库单独泄露；
  主密钥丢失明确报错。Web 端 key 只写不读，日志不出现 key。
- key 来源建议显式字段 `Stored(密文) | Env(变量名)`，常见预设默认 `Env` 并预填变量名，
  不用回退顺序（待 B2 确认）。
- 配错或失效即该次 run 失败。
- 预设经 `/models` 拉取模型、上下文窗口、模态与推理档位，只留主要参数给用户填。
