# 过度设计复查（2026-10-19）

状态：审计结论，未改代码。替代 [10-12 审计](over-design-audit-2026-10-12.md)；后台任务、压缩、手写 provider 落地后重扫，并按「定时任务会用到什么」重新判断脚手架。

## 总体结论

10-12 列的大部分问题已随后续实现消失：`Completion`、`Dispatched`、`Boundary`、`Task` 会话、`ToolScope::Only`、`parent_session_id` 都有了真实生产者；手写 provider 去掉了空正文重试与 `FunctionCall` 变体；max_turns 收尾改为 `tool_choice: none`。剩下的是少量无人调用的公开函数、一个从未有生产者的消息片段，以及微信模块的重复校验。

## 已解决（10-12 → 现在）

| 原编号 | 结局 |
|---|---|
| A1 空正文重试、A3 `FinishReason::FunctionCall` | 随手写 provider 消失 |
| A2 `header_legend` 描述不存在的消息头 | 现在列出的头都会真实出现，不再是问题 |
| C 路线图脚手架 | 除 `File` 与 `Triggered` 外均已被后台任务、压缩用上 |
| D2 收尾轮发 `tools=[]` | 已改为 `ToolChoice::None`，保住前缀缓存 |

## A. 小范围整理候选

| # | 位置 | 问题 |
|---|---|---|
| A1 | `now_ms` 五份拷贝：`mic-core/run.rs`、`mic-gateway/api.rs`、`mic-gateway/logs.rs`、`mic-store/schema.rs`、`mic-channel-wechat/account.rs` | 一处真相被拆成五份；`logs.rs` 版本还把时钟异常静默成 0。若收成 `mic-store` 一个公开函数，属于新增公开签名，须先走 B2；本项不是私有修改豁免 |
| A2 | `mic-provider-openai/src/factory.rs:45` `key_env` | `.ok()?` 把配置解析错误吞成「无环境变量」。库内配置已规范化可 `expect`，但 `test_endpoint` 传入的是未保存表单，需先确认那条路径已校验过 |

## B. 公开死代码（删除走 B2，不改行为）

| # | 位置 | 说明 |
|---|---|---|
| B1 | `ContentPart::File` / `FileRef` | 2026-09-23 起无生产者（微信文件走文本路径）。波及 mic-message 类型、store trace 摘要、core 请求与用量估算、provider 三处 `unsupported_file`、`-p` 渲染、Web `MessageItem` 文件片段 |
| B2 | `Store::bind_identity`、`Store::set_pwd`、`Store::sessions_with_unclaimed_input` | 生产代码与测试都无调用 |

## C. 定时任务会用到的（建议保留，交定时任务 B1 定形）

| 项 | 现状 | 定时任务怎么用 |
|---|---|---|
| `SessionKind::Triggered { module, ref_id }` | 存取、trace、`session_channel`、交付路由都已支持，但从无生产者 | 每次触发开一个 Triggered 会话，`ref_id` 记任务 id |
| `Kernel::create_session` | 无调用方（Web/微信走 `resolve_root_session`，子 agent 由 store 建） | 定时模块建 Triggered 会话的唯一入口 |
| `DeliveryTarget` 挂在会话上 | Root 微信会话用 | Triggered 会话带上目标即可把结果推到微信 |
| `Module` / `Service` / `with_module_tx` | 微信在用 | 定时模块按同样方式装配、存自己的表、跑调度循环 |

定时任务 B1 需要复核的取舍（决定上表是否够用；早期已选 Triggered 独立会话，见 [产品方向](product-roadmap.md#定时任务)）：

1. **结果落在哪**：每次触发新开 Triggered 会话（干净上下文、可在侧栏单列），还是追加进某个已有 Root 会话（像「提醒我」，对话连续）。后者只是替代候选，尚未批准；不能据此删除 `Triggered`。
2. **侧栏是否可见**：`list_root_sessions` 只列 Root；Triggered 会话目前只在 Trace 里能看到。
3. **看板事实来源**：[cross-check](cross-check-2026-09-23.md) §四-4 的待核对项。

## D. 需 human 拍板（沿用 10-12）

- **D1 微信磁盘重复校验**：约 27 处 `AccountError::Invalid` 重新校验本模块同事务写入的行，另有逐行 `plan_version` / `snapshot_version`。推荐「磁盘边界只指 schema 版本」，可删约一半，user_id 归属检查保留。
- **D3（低置信）`ExecFailureKind` 三分类**：现在 Input/Business/Dependency 都有生产者，只作给模型看的标签，维持现状。
- **D4（低置信）`wechat` 可选 feature**：删除收益小，维持现状。

## 建议顺序

1. A1 新增公开签名先起 B2；A2 核实表单校验路径后判断修改范围。此处仅记录候选，不授权实现。
2. B1、B2 合成 `docs/blueprints/dead-code-cleanup.md`，删前用诊断 SQL 确认库内无 `File` 片段。
3. C 留到定时任务 B1 一起定；D1 等 human 选解释。
