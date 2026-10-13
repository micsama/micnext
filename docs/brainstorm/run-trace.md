# B1：用现有存储还原 run trace

**状态：方向讨论（2026-10-13），不实施。前置：[B2 error-logging](../blueprints/error-logging.md)。**

## 想要什么

类似 Langfuse 的排查视图：一次用户输入（run）是一条 trace，其中每次模型调用、工具调用、压缩是 span，能看耗时、用量、输入输出和失败原因，并能关联同期日志。用量小，先用现有 SQLite；数据量大了再考虑 DuckDB 读同一份数据做分析，不在本期。

## 现有数据能还原到哪

| trace / span | 来源 | 能拿到 | 缺 |
|---|---|---|---|
| trace = run | `core_runs` | 会话、状态、起止、本轮快照（模型、思考档位、人设与通用 prompt、max_turns、压缩参数） | 触发它的输入只能经 `core_messages.run_id` 反查 |
| 模型调用 span | `core_model_calls` | purpose、模型、起止、各类 token、失败原因；每次重试一行；`context_boundary_id`/`context_through_id` 圈出输入消息范围；产出经 `core_messages.model_call_id` 关联 | 首 token 时间；实际发出的完整请求（system、工具定义）只能按快照重建，非原文 |
| 工具 span | `Reply` 块里的 tool call（参数）+ `ToolResult` 消息（结果、失败分类） | 参数、结果、`input/business/dependency` 分类；结束时间≈结果的 `created_at` | 开始时间（可近似取所属模型调用的 `finished_at`）、准确耗时 |
| 压缩 span | `purpose=compaction` 的模型调用 + `Boundary` 消息 | 摘要、保留范围、文件快照、撤回时间 | 压缩调用审计 `run_id` 为空（既定契约），需按会话 + 时间归入 run，或改契约 |
| 子 agent | `core_sessions.parent_tool_call_id` / `parent_session_id` | 可挂到父 trace 的工具 span 下 | 尚未有实际消费者，先不展开 |
| 标题 | `purpose=title`，`run_id` 为空 | 归属会话，不属于任何 trace | 作为会话级事件单列即可 |
| 通知、框架备注 | `Notification` / `HarnessNote` 消息 | 作为 trace 内事件点 | — |
| 日志 | 日志库 `log`（独立 SQLite，按 `boot_id`） | B2 固定 `session`/`run` 字段后可按 run + 时间窗关联 | 两个库不同文件，关联在读取侧做 |

结论：现有表已能还原「run → 模型调用（含重试）→ 工具结果 → 压缩」的主干，足以做第一版只读视图。缺口集中在**工具开始时间/耗时**和**首 token**，与 todo 中「统计行」条目是同一批缺口，可一起补。

## 候选

- **A. 只读 SQL 视图 + 现有诊断 SQL 页**：建几个视图（trace 列表、span 列表），在开发者 SQL 页查。成本最低，适合先验证数据够不够用。
- **B. Web trace 页**：按 run 展开时间线（瀑布图），点 span 看输入输出与关联日志。体验接近 Langfuse，需要新的只读接口和页面。
- **C. 外部导出**：把同样的 trace/span 转成 OTLP 发给 Langfuse 或自建监控。数据来源与 A/B 相同，只多一个出口。

倾向 A 起步，确认字段够了再做 B；C 等真有外部监控时再接。三者共用同一份存储，不另存第二份 trace 数据。

## 缺口补法（慢慢补，各自独立）

1. 工具调用起止时间：`ToolResult` 落盘时一并记开始时间（或耗时）。
2. 首 token 时间：模型调用记录加一列。
3. 实际请求原文：是否需要存要权衡体积，先按快照重建，确有排查需要再定。

每项单独起 B2，不为 trace 一次性改表。

## 待 human 决定

- 起步选 A 还是直接 B。
- 缺口 1、2 是否与「统计行」条目合并推进。
