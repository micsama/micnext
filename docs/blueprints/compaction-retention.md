# B2：压缩后的保留规则

**状态：已批准（human 2026-10-19），已实现，自动验收通过，待 human 真实会话验收。**

方向见 [B1](../brainstorm/compaction-retention.md)。修订 [context-compaction](archive/context-compaction.md) 的 §3.5「保留最近 N 个 step」与设置项；触发、请求形状、`/compact`、`/clear`、撤回、文件快照不变。

## 一、问题

压缩后保留最近 N 步原文（默认 15）。步的大小不均，保留段没有总量上限，压完仍可能接近阈值并接连再压；保留下来的多是易过时的工具输出，用户原话却只剩摘要里的转述；摘要还与保留段重复。

## 二、契约

### 2.1 设置

- `core_settings` 删 `compaction_keep_steps`，加 `compaction_keep_percent INTEGER NOT NULL DEFAULT 15 CHECK(compaction_keep_percent BETWEEN 0 AND 50)`（core 迁移 v13）。已有库直接取默认 15。
- `CompactionSettings { trigger_percent: u8, keep_percent: u8 }`，替换 `keep_steps: u32`。
- HTTP 设置读写体 `compaction.keep_steps` → `compaction.keep_percent`，必填；Gateway 校验 0..=50，报错「保留原文比例须在 0 到 50 之间」。
- 设置备份同字段改名。旧备份里的 `keep_steps` 不再识别，导入报格式错误（不留兼容分支）。
- 设置页「对话」组：「保留步数」改为「保留原文」，单位 %，说明「压缩时按窗口比例保留最近的原文，至少保留最新一步」。

### 2.2 压缩边界

```rust
pub struct Compaction {
    pub summary: String,
    pub keep_from: Option<MessageId>,
    pub files: Vec<FileSnapshot>,
    /// 保留段里替换为占位的工具结果（ToolResult / Completion）。
    #[serde(default)]
    pub elided: Vec<MessageId>,
    /// 本次压掉范围内的用户输入，组装请求时带回。
    #[serde(default)]
    pub quoted: Vec<MessageId>,
}
```

`#[serde(default)]` 只为已落盘的旧压缩边界：它们没有替换也没有带回，空列表即原义。

只存 id，不存文本副本；原文始终在会话记录里。

### 2.3 读取

- `ContextWindow` 增加 `quoted: Vec<Message>`：当前有效压缩边界 `quoted` 对应的消息，按 id 升序；其他边界为空。
- `context_window` 与 `window_at` 对 `elided` 中的消息在读取时替换正文为占位，二者同一处实现，Trace 的调用输入重建与实际请求一致。
- 占位由 `mic-message` 提供（`Message::elide_output()` 或同等函数），把 `Completed` 输出换成一段文本：`[Output of {tool_name} omitted at compaction ({n} chars). Key points are in the summary.]`。`n` 为原输出字符数。

### 2.4 请求组装

system prompt 的 context 块中，摘要之后、文件快照之前加一段（`PromptPartKind::Summary`，上下文构成里计入「摘要」）：

```
User messages from the compacted conversation (verbatim, oldest first):
[1] …
[2] …
```

- 单条估算不超过 `QUOTE_FULL_TOKENS`（500）放全文。
- 超过的保留首尾：开头约 `QUOTE_HEAD_TOKENS`（300）+ `……（省略约 N token）……` + 结尾约 `QUOTE_TAIL_TOKENS`（150），按估算口径换算字符切点，不切开 UTF-8 字符。
- 图片、文件附件只写 `[图片]`、`[文件：path]`，不加载数据。

阈值放 `mic-core/src/limits.rs`，与 `ELIDE_TOOL_TOKENS`（2000）一起交验收微调。

## 三、压缩流程

在现有 `compact` 中，总结请求发出前一次算定以下三项，失败路径不变：

1. **划步**：当前有效历史按 `Reply` 起点切成步；第一个 `Reply` 之前的消息不属于任何步。
2. **替换候选**：除最新一步外的两类消息：
   - **沿用**：上一有效压缩边界已替换的结果（读取时已是占位，不再按大小判定），继续替换；
   - **新增**：`ToolResult` / `Completion` 中结果为 `Completed`、估算超过 `ELIDE_TOOL_TOKENS` 的消息。

   `Failed`、`Cancelled`、`Dispatched` 不替换。
3. **保留段**：预算 = 窗口 × `keep_percent`。最新一步始终保留；往前逐步累加（按替换后的估算大小），整步放得下才保留，放不下即停。`keep_from` = 最早保留步的 `Reply` id；历史中没有 `Reply` 时为 `None`。
4. `elided` = 保留段内的替换候选（沿用 + 新增）；只有新增部分要求摘要提炼，沿用部分的要点已在上一份累积摘要里。
5. `quoted` = 有效历史中 id < `keep_from`（`None` 时为全部）的 `UserInput`，排除本次待回答的输入（它们已按原规则作为新输入执行）。取自裁剪前的历史，总结请求因超长被裁掉的早期输入也会带回。

### 调用记录与 Trace 重建

总结指令由同一个函数生成，执行与 Trace 共用：`instruction_note(session, extra, kept_replies: usize, elide: &[Message])`。生成所需的事实随压缩调用记下，成功与失败的调用都记：

- `ModelCallContext` 增加 `keep_from: Option<MessageId>` 与 `elide: Vec<MessageId>`（本次新增替换）。
- 存储：`core_model_calls` 加 `context_keep_from INTEGER REFERENCES core_messages(id)`；`core_model_call_inputs.role` 增加 `'elide'`（CHECK 变更需重建该表，同在 v13）。
- 重建：`Kernel::input_messages` 对压缩调用按 `window_at` 取窗口、去掉 `skip`，`kept_replies` = 窗口中 id ≥ `keep_from` 的 `Reply` 数，`elide` 按 id 读回，再调同一函数得到指令。窗口里上一边界已替换的结果照样是占位，与当时请求一致。

原先「刚返回、工具结果未到的回复原样保留」的特例并入「最新一步始终保留」，删除 `open` 分支。

### 总结指令

`SUMMARY_PROMPT` 调整：

- 摘要覆盖保留段之前的历史，不复述保留段；指令写明「最后 K 个助手回复及其工具结果会原样保留」。
- 列出本次**新增**替换结果对应的 `tool_name` 与 `tool_call_id`（Completion 用 `exec_id`），要求提炼其中继续工作所需的信息，因为这些结果压缩后只剩占位。
- 照抄硬信息：文件路径、错误原文、用户明确的约束、未完成任务。
- 用户原话会原文带回，摘要不必逐字复述用户输入。

### 通知与 Web

- 「已压缩」通知改为：`已压缩上下文，保留最近 {kept} 步，省略 {elided} 条工具输出，带回 {quoted} 条用户原话。`，计数为 0 的分句省略。
- Web 类型 `Compaction` 增加 `elided: number[]`、`quoted: number[]`；分隔条展开后在摘要下加一行灰字「省略 M 条工具输出 · 带回 K 条用户原话」，均为 0 时不显示。

## 四、不变量

1. 切点只落在步起点，工具调用与结果不拆开。
2. 最新一步始终原样保留，其工具结果不替换。
3. 失败、取消的工具结果不替换。
4. 新增替换的结果在总结指令中逐条列出；已替换且仍在保留段内的结果继续替换，原文不会因连续压缩重新出现；会话记录原文不改。
5. 带回与替换由边界记下的 id 决定，组装请求时确定性生成，压完后前缀稳定。
6. 撤回压缩时一并失效，恢复上一有效边界自己的 `elided` / `quoted`。
7. Trace 中压缩调用的输入（含总结指令）可由调用记录还原，成功与失败一致。

## 五、不做

- 平时（非压缩时）清理工具输出。
- 更早压缩范围里的用户原话持续带回：B1 未定项，先只带本次范围。
- 按工具类型区分「易腐」输出。

## 六、验收

- core 测试（`compaction/tests.rs` 或现有 run 测试）：
  - 最新一步超预算仍整步保留、更早步不保留；
  - 更早步整步放得下才保留；
  - 替换只落在保留段内、跳过最新一步与失败结果；
  - `quoted` 只取保留段之前且排除待回答输入；
  - 连续压缩：上一边界已替换、仍在新保留段内的结果继续在 `elided`，且不出现在新指令的提炼列表；
  - 压缩调用（含失败）的 Trace 输入重建与实际请求的指令一致。
- store 测试：`context_window` / `window_at` 对 `elided` 返回占位，`quoted` 按 id 读回；旧格式边界可读。
- `cargo fmt`、`cargo clippy -- -D warnings`、`cargo test`、`bun run check`；真实库副本迁移 v13。
- human：真实长会话压缩一次，看摘要质量、压后占用、Trace 调用输入与分隔条。
