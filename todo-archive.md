# todo 归档

已收尾（实现完成/明确搁置）的条目从 `todo.md` 挪到这里，保留一行指针 + 收尾方式，
不删除对应文档。

| 收尾方式 | 主题 | 文档 |
|---|---|---|
| B1 收尾，契约并入 B2 | mic-message 类型设计 | [`docs/brainstorm/mic-message-types.md`](docs/brainstorm/mic-message-types.md) / [`docs/blueprints/mic-message.md`](docs/blueprints/mic-message.md) |
| 实现完成（CLOSED） | mic-message 类型 | [`docs/blueprints/mic-message.md`](docs/blueprints/mic-message.md) |
| 实现完成（CLOSED） | mic-store 核心持久化 | [`docs/blueprints/mic-store.md`](docs/blueprints/mic-store.md) |
| 实现完成（CLOSED），B2 并入 mic-store.md | mic-store 会话列举（v0a 步 1） | [`docs/blueprints/mic-store.md`](docs/blueprints/mic-store.md) §4.6 |
| 实现完成（CLOSED） | mic-core 模块装配 + 二进制骨架（v0a 步 2） | [`docs/blueprints/mic-core-module.md`](docs/blueprints/mic-core-module.md) |
| 实现完成（CLOSED） | 模型 port（M5）+ OpenAI 兼容实现（M7），含 mic-core `[models]` 装配修订（v0a 步 3'） | [`provider-port.md`](docs/blueprints/provider-port.md)、[`provider-openai.md`](docs/blueprints/provider-openai.md) |
| 实现完成（CLOSED） | 工具 port（M3）+ 基础工具（M8：fs、shell、web_fetch），含 mic-core `Module::activation` 修订（v0a 步 3） | [`mic-tool.md`](docs/blueprints/mic-tool.md)、[`tools-basic.md`](docs/blueprints/tools-basic.md) |
| 实现完成（CLOSED） | 执行主路径（M6：调度、Agent 循环、落盘、崩溃收尾、实时事件）+ `-p`，含 mic-store `extend_claim` 等修订（v0a 步 4） | [`run-execution.md`](docs/blueprints/run-execution.md)（原 `query-execution.md`） |
| 实现完成（CLOSED），契约并入各文档 | 存储结构重整：session / run / message / model_call，一次调用一条 `Reply`，用量不编造 | [`storage-restructure.md`](docs/blueprints/storage-restructure.md) |
| 实现完成（CLOSED） | 网关（M9）：配置与 token、HTTP 接口、SSE 回放交接，含 `Kernel` 只读委托与 mic-store `executing_run`（v0a 步 5） | [`gateway.md`](docs/blueprints/gateway.md) |
