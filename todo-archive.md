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
| 实现完成（CLOSED，2026-10-12） | 开发者日志留痕：INFO+ 进独立 SQLite，超限删旧，失败降级纯文本 | [B1](docs/brainstorm/developer-log-history.md) · [B2](docs/blueprints/developer-log-history.md) |
| 已修复（CLOSED，2026-10-15 Safari 人工验收） | Web 更新后消息尾部不可见：Chrome 分批渲染贴底竞争（3b98736）；Safari 为 WebKit fetch 读流滞留尾段，空注释缓解无效，改用原生 EventSource + 一次性票据（b8267f7），补注释已删 | [B2](docs/blueprints/web-sse-eventsource.md)；[排查记录](docs/brainstorm/web-message-replay.md) |
| 实现完成（CLOSED，2026-10-13 人工验收） | Web 会话分段加载：首屏最近 20 轮/至多 300 条，ready 告知有无更早；倒序滚动，离顶三屏自动取上一页，插入不写 scrollTop；浮动胶囊与交界分隔线提示 | [B2](docs/blueprints/web-message-paging.md)；[B1](docs/brainstorm/web-message-paging.md) |
| 实现完成（CLOSED，2026-10-13 人工验收） | 思考强度按会话选择，模型 GET 返回节点，切模型重置 null；本轮快照固定；模型与人设入口统一，emoji 与浮层比例调整完成 | [B2](docs/blueprints/reasoning-selection.md) |
| 已修复（CLOSED，2026-10-13 服务器人工验收） | 聊天与标题模型选择保存时不再闪烁；保留保存提示，标题选择立即更新、失败回退 | [记录](docs/blueprints/session-title.md#模型选择闪烁修复closed2026-10-13) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 错误保留现有类型至处置边界记一次，级别与字段名固定；正文/标题共用 Provider 构建，正文/压缩共用重试日志；补压缩、工具依赖、服务致命退出与 panic 日志，Provider 上游失败降 DEBUG；为 Langfuse 留接缝，不接入 | [B2](docs/blueprints/error-logging.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 思考档位按供应商保守默认 + 已知模型覆盖；DeepSeek/Sol 默认 low、Luna 默认 medium，未知 Codex 默认三档、low；11 项测试与 11 组隔离验收通过 | [B2](docs/blueprints/reasoning-model-capabilities.md)；[B1](docs/brainstorm/reasoning-model-capabilities.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 会话归档：后端仅标记，记录保留；新消息仍隐藏，手工恢复；前端成功后整页刷新，4 组隔离整体链路通过 | [B2](docs/blueprints/session-archive.md)；[B1](docs/brainstorm/session-archive.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 模型调用改回手写，移除 async-openai（体积 15.0 → 21.6 MiB 主因）；reqwest 统一 0.13 + ring；功能不变，未知流事件改为忽略并告警 | [B2](docs/blueprints/provider-handwritten.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 首问自动标题与一键重新生成；用途模型/档位独立，生成中仅内存；11 组隔离整体链路通过；侧栏刷新边界与只 trim 已定，复用会话详情 GET | [B2](docs/blueprints/session-title.md)；[B1](docs/brainstorm/session-title.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | Web 聊天页可切换微信会话模型，与 Web 会话同一入口；`/model` 后续另定 | [B2](docs/blueprints/wechat-session-model.md) |
| 实现完成（CLOSED，2026-10-15 人工验收） | SQLite 整理 + 轻量 trace：core/wechat 新基线、删 run 配置副本与格式版本字段、调用行补事实、trace 页与看板 | [B2](docs/blueprints/run-trace.md)；[存储 B1](docs/brainstorm/sqlite-organization.md)；[trace B1](docs/brainstorm/run-trace.md) |
