# B2：调用统计统一口径

状态：CLOSED（2026-10-23 human 页面与生产验收通过；2026-10-22 human 批准）。

来源：[设计审查 2026-10-22](../../reviews/design-review-2026-10-22/README.md) S1、S2、存储 R1，human 决定 2：「已知费用与用量一律计入，不论成败；存在未知时显示为下限，run/会话/Web 统一。」

## 现状问题

同一批调用事实，有四处各自定义口径：

| 位置 | 失败/停止的调用 | 缺用量 | 缓存「完整」 | 费用 |
|---|---|---|---|---|
| `run_stats`（Trace 每个 run） | 不计费用，也不影响完整性 | 只看成功的调用 | 要求成功的调用都有用量 | 成功的调用任一无价就是 null |
| `session_usage`（会话、会话树） | 计入 | 计次数 | 不看缺用量的调用（S2） | 合计已知部分 |
| `CallStat`（看板、定时任务） | 计入；看板直接丢掉被停止的调用 | 计次数 | 不给 | 合计已知部分 |
| Web `usageOf`（步骤、单次尝试） | 计入 | 计次数 | 同 `session_usage` | 合计已知部分 |

结果是同一个 run 可能在 Trace 列表里显示「$0、用量完整」，展开后单次调用却显示 $3 和未知用量（S1）；含未知调用的会话，缓存读取仍显示为确定值（S2）。

## 口径

**每次调用独立贡献**：
- 用量已知就累加 token，费用能算就累加费用。
- 缺用量或缺单价，就在那一项记一次「未知」。
- 成败、停止只决定这次调用算成功、失败还是停止，与用量、费用无关。

**调用结果分三类**：
- 有 error 算失败。
- `stop = Stopped` 算停止。停止是用户主动操作，既不算成功，也不算失败。
- 其余算成功。
- 平均耗时只取成功的调用，停止的调用耗时被截断，不参与。

**展示**：
- 合计里只要有未知，就显示为下限 `≥ 数值`。
- 悬停说明有几次未知。
- 全部未知时显示「未知」；没有调用时显示「—」。
- run、会话、会话树、步骤、看板、定时任务都按这一条。

**缓存命中率只在单次回复上显示**：
- 位置是聊天回复的悬停统计，一条回复对应一次调用，数字总是准的。
- 合计上不再显示命中率：缺用量时分子、分母都不完整，比例可能偏高也可能偏低，连下限都算不上。
- 例子：已知调用输入 100、缓存 50，未知调用实际输入 900、缓存 0。按已知部分算是 50%，实际只有 5%。

缓存读取、缓存写入、推理 token 只是参考，不单独追踪「未知」：哪次调用没上报，就不参与这一项的合计，也不加 `≥`。费用公式 `call_cost` 仍要求缓存读取已上报，否则这次费用记为未知。

## 契约

### Store（mic-store `trace`）

新增一个合计类型 `UsageTotal`，由 `add(&TraceCall)` 累加，是唯一的 Rust 口径实现：

```rust
pub struct UsageTotal {
    pub calls: u32,
    pub input_tokens: u64,
    pub output_tokens: u64,
    pub cache_read_tokens: u64,
    pub cache_write_tokens: u64,
    pub reasoning_tokens: Option<u64>,  // 全部未上报为 None
    pub unknown_usage: u32,             // 无用量的调用
    pub cost: f64,                      // 已知部分
    pub unknown_cost: u32,
}
```

- **`SessionUsage` 删除**：`SessionUsageView.own`、`TreeUsage.usage` 改为 `UsageTotal`。去掉 `known_usage`、`usage_complete`、`cache_complete`，`cost` 不再可空。
- **`CallStat` 改为 `{ #[serde(flatten)] usage: UsageTotal, failed_calls, stopped_calls, avg_ms }`**，JSON 仍是平铺字段：
  - 新增 `reasoning_tokens`、`stopped_calls`。
  - `avg_ms` 只取成功的调用。
  - 看板不再丢掉被停止的调用：用量、费用、调用次数照算。
- **`RunStats` 只留耗时和计数**：`total_ms, model_ms, tool_ms, steps, requests, tools`。
  - token、缓存、费用和三个完整性字段一律删除，Web 从 `calls` 算。
  - `first_output_ms`、`output_tps` 也删除：当前没有任何读者。
  - `settled` 过滤随之消失。

### Web

- **`usageOf(calls)`**：返回与 `UsageTotal` 同形的对象。它是口径在前端的唯一实现，与 Rust 那份保持相同规则（两处各十来行，不为去重加依赖）。
- **`TraceUsage`**：直接接收 `UsageTotal`，删掉现在把 `SessionUsage` 字段名改写成另一套的映射。
- **下限显示**：加一个格式化函数 `atLeast(value, unknown, total)`，输出 `≥ x`、`x` 或「未知」。Trace 列表行、`TraceUsage`、会话树合计、看板、`ScheduleRow` 都用它。
- **删除合计上的缓存命中率**：
  - `TraceUsage` 的命中百分比和比例条。
  - 看板 KPI 的「缓存命中」。
  - 看板模型表的「缓存命中」列。
  - 聊天回复悬停里的命中环保留。
- **看板调用图**：
  - 成功 = `calls − failed_calls − stopped_calls`，图例仍是「成功 / 失败」。
  - KPI 的悬停说明里写停止次数。
  - 模型表的失败率仍是 失败 ÷ 调用。
- **Trace run 行的 token**：改用 `usageOf(r.calls)`。
- **聊天回复的悬停统计**：只有单次调用，不受影响。

## 调用方影响

| 位置 | 改动 |
|---|---|
| mic-store `trace/mod.rs` | 新增 `UsageTotal`；删除 `SessionUsage`、`session_usage()` 私有函数、`run_stats` 中的用量与费用；`CallStat` 改组合；看板不过滤 Stopped |
| mic-store `lib.rs` | 导出 `UsageTotal`，去掉 `SessionUsage` |
| mic-core kernel / schedule | 类型名跟随，无逻辑变化 |
| mic-gateway trace | 无代码逻辑变化，响应形状随类型变化 |
| HTTP | `GET` 会话用量、trace runs、看板、定时任务列表的统计字段按上面调整；仓库内唯一调用方是 Web |
| web `api/types.ts`、`api/trace.ts` | `RunStats`、`SessionUsage`、`CallStat` 类型与解码器同步 |
| web `trace-presentation.ts`、`TraceUsage`、`TraceTab`、`DashboardTab`、`ScheduleRow` | 同上 |
| docs `archive/run-trace.md`、`archive/chat-run-stats.md` | 加修订说明，指向本文 |

没有 schema 迁移：口径只是读取时的聚合方式，存储的调用事实不变。

## 验收

- **自动测试**：改写 `trace/tests.rs` 中「失败调用不算缺用量」那条断言。同一会话构造四次调用：
  - 失败，有用量且有价（$3）。
  - 失败，无用量。
  - 停止，有用量，但缓存读取未上报。
  - 成功，有完整用量。

  断言：
  - 会话 `UsageTotal`：`calls = 4`、`cost` 含第一次的 $3、`unknown_usage = 1`、`unknown_cost = 2`；缓存读取只合计已上报的两次。
  - 看板 `CallStat`：`failed_calls = 2`、`stopped_calls = 1`，`avg_ms` 只等于成功那次的耗时。
  - 空会话：`calls = 0`，各项为 0。
- **收尾**：`cargo fmt`、`cargo clippy -- -D warnings`、`cargo test --workspace`、`web/` 下 `bun run check`。
- **页面（human）**：
  - Trace 列表行、run 和会话的用量卡、会话树合计、看板、定时任务行，有未知时显示 `≥`，悬停能看到几次未知。
  - 合计上不再出现命中率，聊天回复悬停里的命中环照常显示。
