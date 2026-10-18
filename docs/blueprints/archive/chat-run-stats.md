# B2：聊天统计行与上下文占用

**现行扩展（2026-10-16，P3）**：会话范围费用汇总由 [async-jobs](../async-jobs.md) P3 的 `SessionUsage` 单独查询；“包含子 session”不改变本文的聊天 run-stats、run 概览或当前会话上下文占用口径。

**状态：CLOSED（2026-10-15，human 页面验收通过；fmt/clippy、store 测试、Web 检查与隔离实例 curl 验收通过）。**

依据：[B1](../../brainstorm/chat-run-stats.md)。只用 [run-trace](run-trace.md) 已落盘的事实，不改 schema。修订 [gateway](gateway.md) 接口表与 [context-compaction](context-compaction.md) 的上下文接口。

## 一、每轮统计

### 1.1 口径（`mic-store` 一处计算）

`RunStats` 由一个 run 的调用行与工具结果算出，聊天接口与 trace 运行概览共用；trace 前端删去自算的总耗时、模型/工具拆分与用量合计，改读它。

| 字段 | 定义 |
|---|---|
| `run_id`、`state` | run 本身 |
| `anchor_id` | 该 run 最后一条消息的 id，前端把统计行挂在它下面 |
| `total_ms` | run 结束 − 创建；未结束为 null（不返回执行中的 run） |
| `model_ms` | 该 run 全部调用（含失败尝试、压缩）耗时之和 |
| `tool_ms` | 各步工具墙钟之和：每步取「最后一个已知工具结束 − 该步模型结束」，与 trace 同法；并行不累加 |
| `steps`、`requests`、`tools` | 步数（重试合并为一步）、请求次数、工具调用数 |
| `first_output_ms` | 第一个有输出的对话调用：首次输出 − 其开始；都无输出为 null |
| `output_tps` | 输出 token ÷ Σ(结束 − 首次输出)，只取两者都有的调用；无样本为 null |
| `input_tokens`、`output_tokens`、`cache_read_tokens` | 各调用之和；失败与被停止的调用无用量，按零计；正常结束的调用缺用量时对应 `*_complete = false`，前端标「≥」 |
| `cost` | 按当前单价折算正常结束调用之和；其中任一无价则 null |

缓存命中率由前端算 `cache_read / input`，不另存；仅在两者都完整且 `input > 0` 时显示，否则显示未知。

### 1.2 接口

| 方法与路径 | 成功 |
|---|---|
| `GET /api/sessions/{id}/run-stats?from=<message_id>[&to=<message_id>]` | 200 `{"runs": [RunStats]}`，取 `anchor_id` 落在闭区间内的已结束 run，按 `anchor_id` 升序 |

前端用法（不约束）：`ready` 后以首屏最早消息为 `from` 取一次；向上翻页时取新加载区间；`run_finished` 后以上次最大 `anchor_id + 1` 为 `from` 补取。区间按已加载消息有界，查询走 `run_id` 索引。

## 二、上下文占用

`GET /api/sessions/{id}/context` 的响应增加：

```json
"occupancy": { "used_tokens": 51234, "window": 128000, "trigger_percent": 80 }
```

- 口径即自动压缩的判定：当前有效窗口下最近一次成功对话调用，上游返回的 `input_tokens + output_tokens`，不估算。边界后尚无调用（`/clear`、压缩后）或会话未选模型时 `occupancy` 为 null。
- `window` 取会话当前模型的窗口，与压缩判定一致。
- 前端每收到一条回复消息即刷新（即每次 LLM 返回），另在 `ready`、`run_finished`、压缩/清空消息后刷新，不需要新事件。

`Kernel::context_view` 返回 core 侧包装（`ContextView` + `occupancy`），`mic-store` 的 `ContextView` 不变。

## 三、调用方影响

- `mic-store`：新增 `RunStats` 与按会话、消息区间的查询；trace 的 `TraceRun` 带 `stats`。
- `mic-core`：`Kernel::run_stats`、`context_view` 返回值扩展；压缩判定改读上游用量（[context-compaction](context-compaction.md) 触发口径修订）。
- `mic-gateway`：新路由；context 响应加字段。
- Web：统计行与占用展示；trace 运行概览改读 `stats`。

## 四、验收

1. store 测试：重试合并、并行工具不累加、缺用量标不完整、无价 → `cost` null、执行中的 run 不返回。
2. 同一 run 的聊天统计与 trace 运行概览数字一致（同一结构）。
3. 隔离实例 curl：区间过滤；`occupancy.used_tokens` 等于最近一次对话调用的 input + output，`/clear` 后为 null。
4. 页面观感交 human；`cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。
