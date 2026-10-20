# B1：SQLite 存储整理

**状态：方向已拍板（2026-10-14），与 [run-trace](run-trace.md) 合并 B2 见 [run-trace](../blueprints/archive/run-trace.md)，CLOSED（2026-10-15）。** 过程稿已收敛，本文只留结论。

## 方向

沿用 SQLite 与现有物理模型，按职责整理代码、表目录、SQL 页，服务轻量 trace 与看板。旧数据直接清理，core 与微信各建新 schema 基线，删除历史迁移链与只为旧结构服务的代码。版本只在 schema 层，不建配置/消息/payload 版本体系。logs 保持独立库，不迁；旧库报错时一并列出日志库路径、与业务库一起删除，避免复用的 session/run id 关联到旧日志。

不做：通用 KV/万能对象表、表合并凑数、改表名前缀、拆库、图片外置、连接池。表数不是指标。

## 现状（2026-10-14 本地库）

core v9、wechat v2，共 21 张表；7 会话、20 run、60 消息、28 调用；图片占库约 83%。本地无微信数据，不据此推断服务器规模。

## 职责分组

前缀表达模块所有权，分组表达业务职责；三者共用：表目录、`store` 私有模块、SQL 页折叠。

| 分组 | 表 | 性质 |
|---|---|---|
| 对话与执行 | core_sessions、core_runs、core_messages、core_model_calls | 容器、轮次、内容、调用事实 |
| 身份 | core_persons、core_person_identities | person 与外部身份映射 |
| 用户配置 | core_settings、core_personas、core_endpoints、core_models | 当前可调值 |
| 内容资源 | core_images | 图片字节 |
| 上下文 | core_boundary_inputs | 边界与保留输入关系 |
| 调度处置 | core_input_holds | 输入 held 标记 |
| 微信 | wechat_account、wechat_settings、wechat_state、wechat_inbound_batch、wechat_inbound_message、wechat_delivery、wechat_delivery_attempt | 账号配置、协议状态、入站、出站 |
| 存储管理 | schema_migrations | 模块版本 |

核心四表不合并：消息与调用非一一对应，失败调用无消息，标题调用无 run。

## 删除与调整清单

| 项 | 依据 | 处理 |
|---|---|---|
| run 的 persona_id/name/prompt、general_prompt、max_turns、压缩参数、model_name、reasoning_effort | 仅 `claim_next` 写入，执行用内存 `RunSettings` | 删除 |
| run 的 `model_id` | 仅 `context_window` 消费 | 移到调用行（可空外键），连同模型名、思考档位；`context_window` 改从调用取 |
| `Compaction.extra` | 复制待处理输入/指令文本，无后续消费者 | 删除 |
| `delivery_version`/`DeliveryTarget.version`、`snapshot_version`、`plan_version` | 各自单设格式版本 | 删除字段与检查，归 schema 管 |
| trace 所需新列 | 见 [run-trace](run-trace.md#已拍板) | 调用行：首次输出、StopReason、输入描述的起点/排除项/末尾项；失败调用保留 usage；Boundary 写 `model_call_id`；ToolResult 带 `started_at`；重试不加字段 |
| 模型单价 | 费用估算 | `core_models` 上与 `context_window` 同级的可空列，模型设置里填写；不进 Provider 的 `config_json` |

微信投递计划、入站暂存、压缩文件快照都有实际消费者，只删版本层。

## 表目录

`schema.rs` 旁放一份静态数组（表名、分组、一句说明），SQL 页按它分组展示，同时作为表目录；`lib.rs` 表清单与 `mic-store.md` 里过时的表概览删掉，改指向它。离线调试对齐即可，不加一致性断言。

## 代码整理

- `store.rs`（约 1,200 行）拆私有模块，公开契约不变，不加 trait 或 crate；只做实现所需的机械拆分，放在 B2 实施第一步。分组只是展示口径，不强制成模块结构，现有 `context`、`models`、`title` 等职责保留。
- 迁移改为每模块一份基线建表；微信建表移出 `account.rs`。

## 性能

数据量小，表数不是瓶颈。索引随实际读取路径加（如 trace 按 `run_id`）；保持单写连接、短事务，网络执行在事务外。

## 未定（不阻塞本轮）

对话与调用长期保留期、微信 interrupted 入站批次与投递记录保留期。归档仍只隐藏。
