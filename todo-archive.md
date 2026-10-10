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
| 实现完成（CLOSED） | Web 前端（M10）：Svelte 5 + Tailwind、消息呈现（Markdown/公式/高亮/复制）、流与重连、gateway 静态资源嵌入（v0a 步 6） | [`web-ui.md`](docs/blueprints/web-ui.md) |
| 实现完成（CLOSED） | 运行期设置阶段一：人设、通用偏好、按会话选择、`max_turns`/`workdir` 入库、内置人设启动同步 | [`runtime-settings.md`](docs/blueprints/runtime-settings.md) |
| 实现完成（CLOSED，2026-10-10 服务器验收） | 渠道提示：仅 Root，简短描述界面与表达适配；告知复用 Reply 投递 | [B2](docs/blueprints/channel-prompt.md)；[审查依据](docs/brainstorm/channel-prompt-review.md) |
| 实现完成（CLOSED，2026-10-10 服务器验收；Phase 1–6） | 微信 Channel：启动待命 → Kernel 能力 → Web 登录 → 入站 → 回复投递 → typing/整体验收；Bun 构建已本地验证并已推送 | [`docs/blueprints/wechat-channel.md`](docs/blueprints/wechat-channel.md) §十一；协议事实 [`wechat-protocol.md`](docs/brainstorm/wechat-protocol.md) |
| 实现完成（CLOSED，2026-10-10 人工验收） | Web 开发者诊断：内存日志（项目 DEBUG/依赖 INFO，1 万条/16 MiB，筛选/正则/暂停/复制）+ 只读 SQL（mic-store 只读连接 + authorizer，凭据列由建表方声明读作 NULL，2s/200 行/1 MiB） | [B2](docs/blueprints/developer-diagnostics.md)；B1：[日志](docs/brainstorm/developer-logs.md)、[SQL](docs/brainstorm/developer-sql.md)；原日志路径/级别约定定稿后补进 `CLAUDE.md` |
| 实现完成（CLOSED，2026-10-10 服务器验收） | Web 更新：原地 pull --ff-only（5 次，超时 20s→60s 递增，立即重试）+ 缓存构建 + exec；代理继承启动环境、记录 from 提交供人工回退、构建无超时；Linux 原地构建已实测 | [B2](docs/blueprints/self-update.md)；[B1](docs/brainstorm/self-update.md) |
