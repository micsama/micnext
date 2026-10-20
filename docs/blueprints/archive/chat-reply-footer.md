# B2：连续聊天流的回复悬停栏

**状态：CLOSED（2026-10-20，human 页面验收通过；fmt/clippy、Core/Store/Provider 测试与 Web check/build 通过）。方向：逐回复简要统计，删除聊天专用 run 统计旧路径，trace 保留完整细节。**

## 一、方向与运行语义

Session 与一串连续上下文是持久的对话载体。Agent 执行器消费待处理输入队列：有输入时被激活并处理，AI 产出结果或执行被打断后休息；工作期间有新输入，在检查点优先接入后续处理。这里的优先是接入输入，不是保证抢占、倒序或取消已有工作。

用户看到的通常是“人的消息 → AI 的回复块”，可以据此称为业务上的一轮。这个称呼用于阅读与操作，不规定执行器生命周期，也不要求每轮恰好一条用户消息、一条 Reply 或一次 run。插嘴前的 AI 块可能只有工具调用，没有正文；这种块没有可复制内容是正常状态，不回找其他消息补上。

每个 AI 回复块底部都有自己的悬停栏，不按 run 决定哪些块有栏。

现有代码将一次激活后的执行记为 run，供调度、停止和诊断统计使用。这是实现对执行过程的记录，不是业务轮的定义。休息不表示保留一个暂停中的 Exec 或模型请求。

后台任务独立启动、独立执行；完成或其他约定时机可以投递消息，推动执行器继续工作。它不属于某个聊天轮的执行范围。现有实现仍保存 owner_session_id、来源调用等关联，用于回报路由、任务管理及子 agent 关系；“独立执行”不意味着删除这些实际需要的关联。

当前插队是循环检查点吸收输入，不是抢占正在进行的模型请求或前台工具。`Store::absorb` 吸收队首连续、同 person、非 held 的输入，Command 不在执行中吸收；停止后不再吸收。一次 run 可以包含多条用户输入和多条 AI 回复。

上下文段与 run 是独立变化轴，自动压缩可以发生在 run 内。此次仅调整聊天展示，不改变调度、插队、压缩、后台任务或停止行为。

## 二、展示契约

| 对象 | 行为 |
|---|---|
| 已落盘的每条可见 Reply | 底部保留一行，悬停回复块或键盘聚焦栏内控件时显示，平时整行隐藏但保留高度 |
| 回复栏基础信息 | 复制、消息时间、该 Reply 的模型；没有正文时不显示复制 |
| 复制 | 只复制当前这条 Reply 的全部 Text 块，以空行连接并去除首尾空白；不跨消息，不包含思考、工具参数或结果。最终回复块的按钮即复制最后一次回复正文 |
| 统计 | 该 Reply 对应的单次模型请求：模型耗时、输入 token、输出 token、缓存命中率；与复制、消息时间、模型同处一行，全部悬停显示 |
| 模型耗时范围 | 请求开始到请求结束，不含失败重试、退避、工具执行、后台等待、压缩或标题请求 |
| 用量 | 输入包含缓存 token，输出包含推理 token，沿用调用事实口径；上游未提供时整段只显示「用量未知」，不显示为零，也不另列缓存 |
| 缓存命中率 | 该请求 cache_read_tokens / input_tokens；输入为零或缓存读数未知时显示未知，已知缓存读数为零时显示 0% |
| 无正文的工具调用回复 | 同样显示消息信息与该请求统计，只不显示复制 |
| 停止后的部分回复 | 展示已记录的模型耗时；用量未取得时显示未知，不推算 |
| 流式草稿 | 沿用草稿展示，落盘后出现回复悬停栏 |

“每个回复块展示一次”指每个块都有自己的栏，正文为空时只缺复制操作。不能将现有 run 合计复制到各块，冒充块级统计。

不增加轮次编号、run 卡片、run 分组容器、问题聚合、独立分隔条或多模型汇总。消息按现有顺序连续呈现，插入用户消息或后台回报不重新组织已有回复。

## 三、契约与调用方影响

### 3.1 单一路径：调用事实随回复读出与发布

现有 `core_messages.model_call_id` 已直接关联产出回复的请求，消息查询已经 JOIN `core_model_calls` 还原模型名。扩展这条现有投影，不新增查询端点、统计副本、后台补查或独立缓存。

`mic-message` 新增公开类型 `ReplyStats { model_ms: i64, usage: Option<ReplyUsage> }`、`ReplyUsage { input_tokens: u64, output_tokens: u64, cache_read_tokens: Option<u64> }`；`MessageBody::Reply` 新增必填 `stats: ReplyStats`。这些是调用事实的读取投影，不进入模型上下文。

`model_ms = finished_at - started_at`；`usage = None` 表示未获得用量，`cache_read_tokens = None` 表示用量已获得但缓存读数未知。Reply 必然有关联调用，不为非 Reply 消息添加空统计，也不提供旧结构兼容默认值。

`mic-store` 的 `row::message` 从已有关联调用还原 `model` 与 `stats`；`record_model_call` 提交时返回的 Reply 使用同一组事实构造同形数据。磁盘 payload 不重复持久化可由调用行还原的 `model`、`stats`。不改 schema，不回填历史消息，不做消息 payload 版本化。

所有构造、解构 Reply 的调用方同步更新；Provider 的模型视图仍只使用已有正文、思考与工具块，不发送统计给模型。

### 3.2 HTTP、实时消息与前端

历史分页、会话初始快照、重连回放及 SSE `message` 事件 中的 Reply 均新增相同的 `stats` 字段，例：

```json
{"kind":"Reply","model":"example","blocks":[],"stats":{"model_ms":1200,"usage":{"input_tokens":1000,"output_tokens":80,"cache_read_tokens":600}}}
```

沿用现有消息传输路径与 `KernelEventKind::MessageAppended(Message)`，不增加事件种类。前端 Reply 类型和严格 decoder 同步修改；显示直接从该回复读取，不等待 `run_finished`。历史与实时数据必须一致。

删除 `GET /sessions/{id}/run-stats`（完整 API 路径沿用现有 `/api` 前缀）及其客户端。Rust、Web 同步升级，不保留旧接口。

### 3.3 Trace 边界

Trace 记录与展示完整执行细节：请求尝试、失败重试、各阶段耗时、工具、后台关联及 run 合计。其采集、存储、聚合口径、端点与界面均保持原有职责。聊天只是读取同一调用事实中的少量字段，不要求 trace 跟随聊天简化。

同一请求的事实保持一致，各回复相加不要求等于 run 合计：失败重试与压缩等请求可能没有可见回复。Trace 汇总对失败或停止请求缺失用量的现有完整性处理不变，聊天单条回复缺用量则显示未知。

`RunStats` 及 trace 仍使用的聚合、费用和用量类型保留。无新增依赖、crate 或模块。

## 四、删除范围

整条删除聊天专用 run 统计路径：

- Store 的 `ChatRunStats`、`Store::run_stats`、专用导出及对应测试；保留 trace 共用的 `RunStats` 和真实调用事实。
- Core 的 `Kernel::run_stats` 转发。
- Gateway 的 run-stats 路由、`RunStatsQuery`、`RunStatsPage` 与 handler。
- Web 的 `ChatRunStats`、聊天专用 decoder、`getRunStats`、SessionView 的 `runStats` / `statsError` / `#loadStats` 及初载、分页、结束后的补查。
- `MessageList.statsAfter` 锚点匹配、非回复常显统计、统计加载错误行、`MessageItem` / `ReplyView` 的旧统计 props。
- `RunStatsLine.svelte` 及其 inline 双模式、聊天详细统计 title；简要统计直接放入已有回复悬停栏，不新建只有单一转发用途的组件。

删除把 run 等同业务轮的相关注释与文案，不增加旧接口兼容、设置开关或双路径。保留运行事件、停止控制与 trace；这些仍有独立消费者。

此前 brainstorm 中“只有 run 底部一行、取消每步栏”“移除模型”“按 run 分组”“复制 run 最后一条回复”的建议由本蓝图替代。

## 五、验收与关闭

自动检查：`cargo fmt`、`cargo clippy -- -D warnings`、在 `web/` 下运行 `bun run check` 和 `bun run build`。更新受影响模块测试，在 Store 功能测试中验证实时返回与历史读出的 Reply 统计一致、未知用量和缓存未知正确保留；验证失败重试没有计入后续 Reply 的请求统计。相关测试放现有模块测试位置，不为简单 UI 展示新增镜像实现的单元测试。

Human 验收：

1. 连续多条 AI 回复各有独立悬停栏，默认都隐藏，鼠标与键盘可操作；复制各自正文。
2. 执行中插嘴，消息流连续，不依据 run 重组聊天块。
3. 后台回报推动新回复，原有历史回复仍可悬停与复制。
4. AI 块只有工具调用时不显示复制，不从其他消息借正文。
5. 回复落盘即有简要统计，不等整个执行结束；刷新、重连、历史分页后相同回复的统计一致。
6. 缓存未知、缓存零命中、无用量、停止后部分正文均正确显示；工具运行再久也不改变该回复的模型耗时。
7. 聊天没有 run 合计、额外统计请求或旧锚点逻辑；trace 的详细记录、重试及执行聚合正常。

批准后实现；自动检查及 human 观感验收通过后标 CLOSED，将 todo 条目移至 todo-archive。

## 六、实施与自动验收记录（2026-10-20）

- 简要统计随 Reply 发布及读取，磁盘 payload 不保存统计副本；聊天专用 run-stats 接口、查询、缓存、锚点与组件已删除。
- Trace 聚合算法、调用采集与详细展示未改；原聚合测试改经 trace 查询验证，保留数值断言。
- `cargo fmt --check`、`cargo clippy -- -D warnings`、`cargo clippy --workspace --all-targets -- -D warnings` 通过。
- Core 34 项、Store 17 项、Provider 10 项测试通过。Store 新增功能测试覆盖实时返回与分页/回读一致、重试请求隔离、缓存零/未知、停止后的未知用量、payload 无统计副本。Provider 本地端口测试获准在沙箱外运行后通过。
- Web `bun run check` 与 `bun run build` 通过；窄屏悬停栏保持一行，可横向滚动。
- 2026-10-20 human 页面验收通过，CLOSED。
