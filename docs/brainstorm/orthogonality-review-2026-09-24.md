# B1：Web 闭环后的最小正交审查（余项）

**状态**：A 并入模型设置阶段二（[`runtime-settings.md`](../blueprints/runtime-settings.md) §四）；C、D 已落地，E 只做了详情一处；B、F 与 E 余项待定。
不替换已批准 Blueprint，不新增实现；方向确定后按主题起 B2，不把本稿当成整体重写授权。

已落地（契约在对应 blueprint，此处不再展开）：

- C：所有发 `MessageAppended` 的写入在内核互斥，发布顺序即提交顺序（run-execution §3.1）。
- D：core 算收尾消息，`Store::interrupt_run` 单事务写入并转终态（run-execution §4.6）。
- E 详情：`GET /api/sessions/{id}` 返回 `SessionItem` + `writable`（gateway §4.2）。
- 另：SSE 发送响应停止并限定优雅关闭时长；前端入站逐字段解码（web-ui §4.1）。

判断标准：最小正交不是类型、trait 或 crate 越多越好——独立变化的语义分开，共同变化的保持在一起。
每次调整必须指出现有摩擦、最终消费者、可以删除的约定，以及复杂度去了哪里。

## 一、B：历史事实、模型输入、供应商 wire 各归其主（accept，既有职责分散）

现状：`mic-message/src/model_view.rs` 格式化框架头、失败文本、本地时间并决定 role；`mic-core/src/request.rs` 拼 system、
调整工具结果与插话顺序，还为判断 user role 构造一次完整 model_view；`mic-provider-openai/src/request.rs` 随后再构造它。
「这条消息发生过什么」与「这次给模型看什么、怎样表达」是独立变化轴：改框架提示策略不应迫使 L0 历史类型拥有更多呈现规则。

- B1：仅把共享呈现函数移到 core，Provider 仍收历史 Message。移动少，但 Provider 仍需理解历史变体。
- B2：core 在已有 request 职责内完成上下文选择、排序与框架呈现，产出独立的模型输入；Provider 只做协议映射、能力检查及推理回传。
  无需新增 crate 或可插拔提示词框架。

倾向 B2：可移除 Message 上的 model_view 呈现方法与重复转换，提示文案/时区集中归 core。不能把 OpenAI 的 role/内容限制误当成通用模型输入，
也不能为假想协议设计巨大联合类型。待定：工具调用与结果相邻是统一上下文约束还是协议适配约束；模型输入需保留哪些来源元数据以支持推理兼容判断。
迁移应逐字段比较当前请求 JSON，不能仅靠编译证明行为等价。

## 二、E 余项：会话身份与来源关系（needs-investigation）

- `Store::resolve_root_session(channel, chat, NewSession)` 的查找键和 `NewSession.kind.Root` 内的键重复，调用方须自行维持相等；可用一次明确构造消掉。
- `SessionKind::Task` 的父工具调用与通用 `parent_session_id: Option` 分开，Kernel 的 expect 声称父会话必有，但 store 并未保证；目前无 Task 生产者。
- `session_channel` 给 Root 返回渠道、Task 返回祖先渠道、Triggered 返回模块名，同一个字符串有不同语义。

倾向只收紧已有构造和读取边界（Root 身份只给一次）；把来源、父子关系、出站目标全部重新建模留待 cron / 子 agent / 微信的真实生产者。
前端可写性归 Gateway 授权策略，不硬编码成内核事实。

## 三、F：工具参数的对象与非法原文不靠 JSON 类型暗示（accept，收益小）

`provider-openai/src/response.rs` 把无效 JSON 或非对象参数包装成 `Value::String(raw)`；`request.rs` 遇到 String 又当原文回传；
`mic-tool/src/handle.rs` 用 `is_object` 区分可执行参数——一条跨三层的隐式协议。候选：保留约定，或用小类型显式表达「对象参数 / 非法原文」。
会触及磁盘 payload 与 Web wire，需单列兼容判断；收益小于 B，不宜单独发动全库重写，下次相关协议调整时一起评估。

## 四、不建议做（reject）

- 把 Engine 的每个步骤拆成 trait/插件：尚无独立替换消费者。
- 为「正交」统一所有事件为 action 字符串/Value，或持久化所有流式 token。
- 把 Scheduler Running/Dirty 当成数据库 RunState 的重复而删除：前者表达唤醒是否待处理，后者表达执行事实。
- 拆更多 crate、上完整 CQRS/事件溯源/通用工作流；删掉工具类型擦除（`ToolHandle` 的消费者是异构工具集合）。
- 异步 Completion/Dispatched、投递字段的未来契约：不因当前没生产者就顺手删，随对应 B1 核实生命周期。
