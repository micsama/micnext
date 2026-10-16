# B2：旁路调用记录输入与产出（先落标题）

**状态：CLOSED（2026-10-15，真实环境人工验收通过）。**

依据：trace 页标题旁路概览没有输入和产出可看（2026-10-15 human 反馈）。方向（human 已定）：旁路挂在会话下；输入像压缩一样指向原消息；产出单独存，做法同压缩的 Boundary，是一条不进上下文、不进聊天的「透明」消息。只做标题；以后有意图分类等新旁路时，沿用同一结构加用途。

## 一、现状

标题调用在 `core_model_calls` 记一行（purpose `title`，`run_id` 为空，无 context）。产出只用来覆盖 `core_sessions.title`，模型原文不留；校验失败（多行、超 40 字）时原文也丢，只剩 `title_error`。输入不记录，trace 无法重建。

## 二、用户看到什么

- trace 选中标题调用，概览依次显示：耗时/用量卡 → 「输入」卡（实际发给模型的那段文字，即拼好的用户文字与 AI 回复）→ 「产出」卡（模型原文；校验失败时原文旁标出失败原因）。
- 标题调用仍只有概览，不加详情/输入页签。
- 本 B2 之前的标题调用显示「旧调用，未记录输入与产出」。
- 聊天时间线、会话预览、模型上下文都看不到旁路消息。

## 三、公开契约变更

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| `mic-message` | `MessageBody` 新增 `SideOutput { purpose: SidePurpose, text: String }`；`SidePurpose { Title }`。`ModelView` 对它返回 `None` | 上下文组装天然跳过；Web 时间线按 kind 跳过 |
| `mic-store` 标题输入查询 | `recent_text_inputs` 改为 `title_sources(session, inputs: NonZeroU32, replies: u32) -> Vec<Message>`：最近的文字 UserInput 与含正文的 Reply，按 id 升序 | 仅手动生成调用 |
| `mic-store` 标题落库 | `save_title_result` 的 `call` 参数改为 `Option<TitleCall { call: NewModelCall, output: Option<String> }>`：取材 id 放 `call.context.instruction`，同事务写调用行、`core_model_call_inputs`（role `instruction`）、有原文时写一条 `SideOutput` 消息（`run_id` 空、`model_call_id` 指向本调用）。收尾回复判定公开为 `mic_store::reply_answer(blocks)`，取材与拼接共用 | 仅 core 标题 worker 调用 |
| `core_model_call_inputs.role` | `instruction` 语义扩为「拼进用途提示的原消息」：压缩为附加文字来源，标题为取用的文字输入与回复 | 不改表结构、不加枚举值 |
| `GET /trace/sessions/{id}` call | 增加 `side_id: number \| null`（`SideOutput` 消息 id），与 `reply_id`、`boundary_id` 并列 | Web 用它取产出 |
| `GET /trace/calls/{id}/input` | 标题调用不再 404（「该调用没有会话输入」）：返回按 instruction 关联原消息重建的那条拼接输入，与压缩的附加指令同一做法 | Web 概览复用现有接口 |
| `GET /sessions/{id}/messages` 等聊天读取 | 过滤 `SideOutput` | 聊天页不需要改渲染 |

`core_sessions.title` 仍是当前标题的唯一来源；`SideOutput` 只是这次调用的产出事实，不参与标题显示。

## 四、实现要点

- 输入：自动仍只取首条文字（此时还没有回复）。手动取最近 8 条文字输入，**另加最近 2 条 AI 回复**（human 定，2026-10-15），与用户输入按时间混排，分别标「用户」「助手」。用户输入合计仍 2,000 字、保留最近部分；每条回复保留开头 500 字。条数与字数进 core `limits.rs`。
- 「AI 回复」= 一轮的收尾回复：不含工具调用的 `Reply`，取其中 `phase` 不是 `Commentary` 的 Text（Chat Completions 一律 `phase: None`，照常取；推理不取）。先按此筛，再取最近 2 条，过程说明和工具调用轮不占名额；筛完无正文的不算。
- 一处真相：`title.rs` 提供唯一的拼接函数（选中消息 → 发给模型的输入消息），生成与 `Kernel::call_input` 都调用它；trace 不另写展示拼接。关联只记 message id，不存拼好的文本——与压缩输入（`extra_text` + `instruction_note`）同一结构。
- 重建是「按当前规则」：以后改 `limits.rs` 的条数/字数或拼接格式，旧调用按新规则重建，可能与当时不同（不为此建版本）。trace 输入页沿用现有提示里「按当时记录还原」的口径，不承诺逐字复现。
- 有 `Finished` 即写原文（含校验失败）；上游失败没有原文，只有调用行的 error。
- 无表结构变更：`kind` 是 payload 生成列，新增变体直接可用；schema 版本不动。不回填旧调用。

## 五、验收

- 自动/手动生成标题后，trace 概览可见输入卡与产出卡；校验失败时可见原文与原因。
- core 测试：同一规则下，生成时发给模型的输入文本与 `call_input` 重建结果逐字相同；含工具调用的回复、`Commentary` 正文不入选，`phase: None` 正文入选。
- 聊天页、会话预览、下一轮模型请求里都没有旁路消息（store 层测试覆盖上下文读取与消息分页过滤）。
- `cargo fmt`、`cargo clippy -- -D warnings`、相关测试，`web/` 下 `bun run check`。
