# B2：工具调用身份限定到回复中的一次发生

状态：2026-10-22 human 授权，2026-10-23 实现完成，待页面验收。采用已有 Reply 身份与块位置，不新增全局执行 id。

来源：[Gateway G2](../reviews/design-review-2026-10-22/gateway-web.md)、[执行 R4](../reviews/design-review-2026-10-22/execution.md)。

## 选择

仅在 Web 拼临时键会留下 job 唯一约束与恢复误关联；改写上游 id 会影响 Provider 协议。选择保留上游 id 原文，内部以已有回复的消息 id 加块位置唯一标识一次调用，不建立新 id 表或随机编号体系。

## 契约

新增 `mic_message::ToolCallRef { reply: MessageId, index: u32 }`，index 是 `Reply.blocks` 中的零起始块位置（包含文字与推理块），不是外部 id 或“第几个工具”的隐含计数。

- `MessageBody::ToolResult` 新增必需的 `call: ToolCallRef`。原 `tool_call_id`、工具名保留，负责协议与展示，不再承担全局身份。
- core 先提交带工具调用的 Reply，拿到其消息 id 后构造 Call；后台登记、子 agent 创建与最终 ToolResult 携带同一个 call。失败/停止后的部分 Reply 不执行工具，沿原契约保留。
- Job/NewToolJob/NewAgentJob 增加 origin_call；数据库用 `origin_reply_id`、`origin_call_index` 保存，唯一约束从 `(origin_run_id, origin_tool_call_id)` 改为这两个字段。原 run 关联保留作发起人和执行起源；删除无读者的 job wire id 副本、`core_sessions.parent_tool_call_id` 以及 `SessionKind::Task` 的同名字段。Task 由唯一 agent job 的 origin_call 还原起源，wire id 只保留在回复与 ToolResult 的协议事实中。
- `Store::job_by_origin(call)` 按内部引用查询。恢复按 Reply + index 枚举打开的工具调用，用 ToolResult.call 精确闭合，用同一引用查已登记 job；旧轮次相同 wire id 不会影响新轮次。
- TraceTool 新增 call 引用；前端 Trace 归属直接使用 call.reply，不再按最近消息位置猜归属。聊天 Map 与已并入结果集合都以 reply + index 为键；分页只有结果没有回复时仍显示独立结果，加载到回复后再并入。
- Provider 请求/响应 wire 字段不变。即便同一回复有两个相同 wire id，内部也能按块位置区分；不声称上游协议一定接受这种输入。

## 历史迁移与不确定性

core 下一 schema 版本（v23）为历史 ToolResult 关联同 run、同 wire id 的最近前置 Reply 及块位置；这符合当前执行器每批闭合后才进入下一次模型请求的顺序。job 优先使用其 Dispatched 结果关联，尚未写结果时用同 run、创建时刻前的最近匹配回复。保持所有 id、任务、消息、状态、时间和输出不变。

迁移按旧执行事实推导；找不到来源，或最近回复内同 wire id 重复而无法唯一判定时，迁移失败回滚，明确报错，不能任取一个、丢掉旧消息或另造身份。普通跨轮重复 id 可无损迁移。同一事务重建 job 表与唯一索引，执行 foreign_key_check 验收；运行时只读新格式，不保留按 wire id 回退的第二套关联。

## 调用方与验收

Message/Store/Core、Web 消息/Trace 解码与所有仓库测试构造同步。更新 archive/async-jobs、run-trace 与聊天展示契约；无 Provider 变更。

- 同一 run 两次回复复用 call_0，各自得到自己的结果并可登记不同后台 job。
- 恢复场景：第一轮已闭合，第二轮同 id 未闭合，只给第二轮补结果；已登记第二轮 job 时准确恢复为 Dispatched。
- 迁移已完成与未回填的 job、跨轮重复、空库；含不可唯一关联的旧记录时整次迁移回滚。
- Web 关联函数以真实消息 fixture 验证跨轮、同回复重复 wire id、分页结果先到；cargo fmt、cargo clippy、Store/Core 功能测试及 Web check。
- 页面观感交 human；自动关联与恢复必须本地验收完成。

## 实施验证（2026-10-23）

Store 24 项、Core 45 项测试通过，包含 v23 旧记录迁移及歧义回滚、同 run 复用 wire id 的独立 job 登记和恢复、实际调用执行结果归属。Web 3 项消息关联测试通过，覆盖跨轮、同回复重复 id 和分页结果先到；Web check、工作区 clippy 通过。请求消息重排、运行指示和 Trace 参数读取也改用同一引用，删除按 wire id 或相邻消息猜归属的旧路径。

收尾时修复删除 Session 列后列表附加字段的索引回归；附加列起点统一取 SESSION_NEXT，标题与列表功能测试通过。
