# todo 归档

已收尾（实现完成/明确搁置）的条目从 `todo.md` 挪到这里，保留一行指针 + 收尾方式，
蓝图移入 `docs/blueprints/archive/`，保留契约与验收记录。

| 收尾方式 | 主题 | 文档 |
|---|---|---|
| B1 收尾，契约并入 B2 | mic-message 类型设计 | [`docs/brainstorm/mic-message-types.md`](docs/brainstorm/mic-message-types.md) / [`docs/blueprints/archive/mic-message.md`](docs/blueprints/archive/mic-message.md) |
| 实现完成（CLOSED，2026-10-20） | mic-message 类型 | [B2](docs/blueprints/archive/mic-message.md) |
| 实现完成（CLOSED，2026-10-20） | mic-store 核心持久化 | [B2](docs/blueprints/archive/mic-store.md) |
| 实现完成（CLOSED），B2 并入 mic-store.md | mic-store 会话列举（v0a 步 1） | [`docs/blueprints/archive/mic-store.md`](docs/blueprints/archive/mic-store.md) §4.6 |
| 实现完成（CLOSED，2026-10-20） | mic-core 模块装配 + 二进制骨架（v0a 步 2） | [B2](docs/blueprints/archive/mic-core-module.md) |
| 实现完成（CLOSED） | 模型 port（M5）+ OpenAI 兼容实现（M7），含 mic-core `[models]` 装配修订（v0a 步 3'） | [`provider-port.md`](docs/blueprints/archive/provider-port.md)、[`provider-openai.md`](docs/blueprints/archive/provider-openai.md) |
| 实现完成（CLOSED） | 工具 port（M3）+ 基础工具（M8：fs、shell、web_fetch），含 mic-core `Module::activation` 修订（v0a 步 3） | [`mic-tool.md`](docs/blueprints/archive/mic-tool.md)、[`tools-basic.md`](docs/blueprints/archive/tools-basic.md) |
| 实现完成（CLOSED） | 执行主路径（M6：调度、Agent 循环、落盘、崩溃收尾、实时事件）+ `-p`，含 mic-store `extend_claim` 等修订（v0a 步 4） | [`run-execution.md`](docs/blueprints/archive/run-execution.md)（原 `query-execution.md`） |
| 实现完成（CLOSED），契约并入各文档 | 存储结构重整：session / run / message / model_call，一次调用一条 `Reply`，用量不编造 | [`storage-restructure.md`](docs/blueprints/archive/storage-restructure.md) |
| 实现完成（CLOSED） | 网关（M9）：配置与 token、HTTP 接口、SSE 回放交接，含 `Kernel` 只读委托与 mic-store `executing_run`（v0a 步 5） | [`gateway.md`](docs/blueprints/archive/gateway.md) |
| 实现完成（CLOSED） | Web 前端（M10）：Svelte 5 + Tailwind、消息呈现（Markdown/公式/高亮/复制）、流与重连、gateway 静态资源嵌入（v0a 步 6） | [`web-ui.md`](docs/blueprints/archive/web-ui.md) |
| 实现完成（CLOSED） | 运行期设置阶段一：人设、通用偏好、按会话选择、`max_turns`/`workdir` 入库、内置人设启动同步 | [`runtime-settings.md`](docs/blueprints/archive/runtime-settings.md) |
| 实现完成（CLOSED，2026-10-10 服务器验收） | 渠道提示：仅 Root，简短描述界面与表达适配；告知复用 Reply 投递 | [B2](docs/blueprints/archive/channel-prompt.md)；[审查依据](docs/brainstorm/channel-prompt-review.md) |
| 实现完成（CLOSED，2026-10-10 服务器验收；Phase 1–6） | 微信 Channel：启动待命 → Kernel 能力 → Web 登录 → 入站 → 回复投递 → typing/整体验收；Bun 构建已本地验证并已推送 | [`docs/blueprints/archive/wechat-channel.md`](docs/blueprints/archive/wechat-channel.md) §十一；协议事实 [`wechat-protocol.md`](docs/brainstorm/wechat-protocol.md) |
| 实现完成（CLOSED，2026-10-10 人工验收） | Web 开发者诊断：内存日志（项目 DEBUG/依赖 INFO，1 万条/16 MiB，筛选/正则/暂停/复制）+ 只读 SQL（mic-store 只读连接 + authorizer，凭据列由建表方声明读作 NULL，2s/200 行/1 MiB） | [B2](docs/blueprints/archive/developer-diagnostics.md)；B1：[日志](docs/brainstorm/developer-logs.md)、[SQL](docs/brainstorm/developer-sql.md)；原日志路径/级别约定定稿后补进 `CLAUDE.md` |
| 实现完成（CLOSED，2026-10-10 服务器验收） | Web 更新：原地 pull --ff-only（5 次，超时 20s→60s 递增，立即重试）+ 缓存构建 + exec；代理继承启动环境、记录 from 提交供人工回退、构建无超时；Linux 原地构建已实测 | [B2](docs/blueprints/archive/self-update.md)；[B1](docs/brainstorm/self-update.md) |
| 实现完成（CLOSED，2026-10-12） | 开发者日志留痕：INFO+ 进独立 SQLite，超限删旧，失败降级纯文本 | [B1](docs/brainstorm/developer-log-history.md) · [B2](docs/blueprints/archive/developer-log-history.md) |
| 已修复（CLOSED，2026-10-15 Safari 人工验收） | Web 更新后消息尾部不可见：Chrome 分批渲染贴底竞争（3b98736）；Safari 为 WebKit fetch 读流滞留尾段，空注释缓解无效，改用原生 EventSource + 一次性票据（b8267f7），补注释已删 | [B2](docs/blueprints/archive/web-sse-eventsource.md)；[排查记录](docs/brainstorm/web-message-replay.md) |
| 实现完成（CLOSED，2026-10-13 人工验收） | Web 会话分段加载：首屏最近 20 轮/至多 300 条，ready 告知有无更早；倒序滚动，离顶三屏自动取上一页，插入不写 scrollTop；浮动胶囊与交界分隔线提示 | [B2](docs/blueprints/archive/web-message-paging.md)；[B1](docs/brainstorm/web-message-paging.md) |
| 实现完成（CLOSED，2026-10-13 人工验收） | 思考强度按会话选择，模型 GET 返回节点，切模型重置 null；本轮快照固定；模型与人设入口统一，emoji 与浮层比例调整完成 | [B2](docs/blueprints/archive/reasoning-selection.md) |
| 已修复（CLOSED，2026-10-13 服务器人工验收） | 聊天与标题模型选择保存时不再闪烁；保留保存提示，标题选择立即更新、失败回退 | [记录](docs/blueprints/archive/session-title.md#模型选择闪烁修复closed2026-10-13) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 错误保留现有类型至处置边界记一次，级别与字段名固定；正文/标题共用 Provider 构建，正文/压缩共用重试日志；补压缩、工具依赖、服务致命退出与 panic 日志，Provider 上游失败降 DEBUG；为 Langfuse 留接缝，不接入 | [B2](docs/blueprints/archive/error-logging.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 思考档位按供应商保守默认 + 已知模型覆盖；DeepSeek/Sol 默认 low、Luna 默认 medium，未知 Codex 默认三档、low；11 项测试与 11 组隔离验收通过 | [B2](docs/blueprints/archive/reasoning-model-capabilities.md)；[B1](docs/brainstorm/reasoning-model-capabilities.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 会话归档：后端仅标记，记录保留；新消息仍隐藏，手工恢复；前端成功后整页刷新，4 组隔离整体链路通过 | [B2](docs/blueprints/archive/session-archive.md)；[B1](docs/brainstorm/session-archive.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 模型调用改回手写，移除 async-openai（体积 15.0 → 21.6 MiB 主因）；reqwest 统一 0.13 + ring；功能不变，未知流事件改为忽略并告警 | [B2](docs/blueprints/archive/provider-handwritten.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | 首问自动标题与一键重新生成；用途模型/档位独立，生成中仅内存；11 组隔离整体链路通过；侧栏刷新边界与只 trim 已定，复用会话详情 GET | [B2](docs/blueprints/archive/session-title.md)；[B1](docs/brainstorm/session-title.md) |
| 实现完成（CLOSED，2026-10-14 人工验收） | Web 聊天页可切换微信会话模型，与 Web 会话同一入口；`/model` 后续另定 | [B2](docs/blueprints/archive/wechat-session-model.md) |
| 实现完成（CLOSED，2026-10-15 人工验收） | SQLite 整理 + 轻量 trace：core/wechat 新基线、删 run 配置副本与格式版本字段、调用行补事实、trace 页与看板 | [B2](docs/blueprints/archive/run-trace.md)；[存储 B1](docs/brainstorm/sqlite-organization.md)；[trace B1](docs/brainstorm/run-trace.md) |
| 实现完成（CLOSED，2026-10-15 人工验收） | 聊天统计行（每轮一行：步数、模型/工具耗时、首字、速率、token、缓存、费用）与输入框上下文占用；后端一处计算，trace 运行概览共用 | [B2](docs/blueprints/archive/chat-run-stats.md)；[B1](docs/brainstorm/chat-run-stats.md) |
| 实现完成（CLOSED，2026-10-15 人工验收） | 旁路调用记录输入与产出：手动标题另带最近 2 条 AI 回复（截断）；标题调用的输入指向原消息，产出存为不进上下文的 `SideOutput` 消息；以后新旁路沿用 | [B2](docs/blueprints/archive/side-call-output.md) |
| 实现完成（CLOSED，2026-10-15 人工验收） | 流式调用长时间无数据即判挂起、直接失败不重试（起因：DeepSeek 思考模式 + `tool_choice: none` 卡死，已在 provider 规避） | [B2](docs/blueprints/archive/model-stall.md) |
| 实现完成（CLOSED，2026-10-15 人工验收） | 运行中停止：Web 停止键与 Web/微信 `/stop`；半截回复保留并通知「已停止」，工具记已取消，未并入的消息照常开新一轮；run 新增「用户停止」终态 | [B2](docs/blueprints/archive/run-stop.md)；[B1](docs/brainstorm/run-stop.md) |
| 实现完成（CLOSED）；遗留两项 2026-10-15 转入 [model-image-support](docs/blueprints/model-image-support.md) 与会话预览修复 | 运行期设置阶段二：模型设置/工厂/凭据、图片输入、本轮快照与模型呈现 | [`docs/blueprints/archive/model-settings.md`](docs/blueprints/archive/model-settings.md)；取舍见 [B1](docs/brainstorm/model-settings.md) §九 |
| 实现完成（CLOSED，2026-10-16 人工验收） | 上下文构成浮层与系统提示结构化：系统提示一处组装，trace 按块展示；构成按上次调用记录查询时重算，不改 schema | [B2](docs/blueprints/archive/context-breakdown.md)；[B1](docs/brainstorm/context-breakdown.md) |
| B2 CLOSED（2026-10-09） | 构建脚本支持 debug 与 run：默认 release，可构建后 exec 启动，运行参数原样透传 | [build-script-run](docs/blueprints/archive/build-script-run.md) |
| B2 CLOSED（2026-10-10） | 对话记录与用户所见大致一致：长期记录与系统解释、临时微信媒体打扁；短占位与合并通知省 token；含占位入站只记录 | [B2](docs/blueprints/archive/conversation-parity.md)；[B1](docs/brainstorm/conversation-parity.md) |
| B2 CLOSED（2026-10-09） | 统一消息头解释与 system prompt 布局；沿用现有模型呈现所有权 | [`docs/blueprints/archive/system-prompt-layout.md`](docs/blueprints/archive/system-prompt-layout.md) |
| B2 CLOSED（2026-10-12）；fmt/clippy、Web 检查/构建与 12 项隔离链路验收通过；human 接手真实使用验收，OpenAI key 401 待补 | 上下文压缩与 `/compact`、`/clear`：每次模型返回后按其上游用量判定，默认 80% 触发，压完再接上工具结果或新输入；只读最新有效 Boundary，clear 后排队输入正常执行；快照走 read；Web 展示结果并可将当前压缩边界置为失效撤回，保留审计记录 | [B1](docs/brainstorm/context-compaction.md)；[B2](docs/blueprints/archive/context-compaction.md) |
| 实现完成（CLOSED，2026-10-10 服务器验收） | 微信引用与入站图片 | [图片 B2](docs/blueprints/archive/wechat-inbound-images.md)；[引用 B2](docs/blueprints/archive/wechat-quotes.md)；[V2 B1](docs/brainstorm/wechat-v2.md) §二 |
| 实现完成（CLOSED，2026-10-16 整理归档） | v0a 模块地图与 Web 闭环 | [B2](docs/blueprints/archive/v0a-module-map.md) |
| 实现完成（CLOSED） | Trace 会话树与整树用量 | [`trace-session-tree.md`](docs/blueprints/archive/trace-session-tree.md) |
| 实现完成（CLOSED，2026-10-19） | Trace AI 调用概览并入完整产出，去掉详情页签 | [B1](docs/brainstorm/trace-ai-overview.md) |
| 实现完成（CLOSED，2026-10-19） | 压缩后的保留规则：按当前历史比例保留原文、替换旧工具输出、带回用户原话 | [B1](docs/brainstorm/compaction-retention.md)；[B2](docs/blueprints/archive/compaction-retention.md) |
| 实现完成（CLOSED，2026-10-19） | 后台任务与子 agent（P1–P3），含聊天页任务条 | [B2](docs/blueprints/archive/async-jobs.md)；[B1 任务条](docs/brainstorm/chat-jobs-card.md) |
| 实现完成（CLOSED，2026-10-19） | 模型支持图片开关与会话预览 | [B2](docs/blueprints/archive/model-image-support.md) |
| 实现完成（CLOSED，2026-10-19） | 微信入站文件与视频 | [B2](docs/blueprints/archive/wechat-inbound-files.md) |
| 实现完成（CLOSED，2026-10-19） | 会话缓存键 | [B2](docs/blueprints/archive/provider-cache-key.md) |
| 实现完成（CLOSED，2026-10-19） | 设置备份与库版本不符时自动重建 | [B2](docs/blueprints/archive/settings-backup.md) |
| 实现完成（CLOSED，2026-10-19），SDK 实现已由手写替代 | Responses 接入 | [B2](docs/blueprints/archive/provider-sdk-responses.md) |
| 实现完成（CLOSED，2026-10-19） | 任务浮窗打磨：按状态分区、常驻任务、回复子 agent、单行列对齐；重启中断汇成一条通知 | [B1](docs/brainstorm/chat-jobs-card.md) |
| 实现完成（CLOSED，2026-10-20） | 前端 Markdown CJK 强调修正 | [B2](docs/blueprints/archive/frontend-markdown.md) |
| 实现完成（CLOSED，2026-10-20） | 连续聊天流的回复悬停栏 | [B2](docs/blueprints/archive/chat-reply-footer.md) |
| 实现完成（CLOSED，2026-10-20） | 会话消息按发生时刻排序 | [B2](docs/blueprints/archive/chat-timeline-order.md) |
| 实现完成（CLOSED，2026-10-20） | 后台 bash 正则命中提前通知 | [B2](docs/blueprints/archive/background-output-notify.md) |
| 实现完成（CLOSED，2026-10-20） | Trace 展示工具清单，删除工具提示 | [B2](docs/blueprints/archive/trace-tools.md) |
| 实现完成（CLOSED，2026-10-20） | Web 推送站内提醒 | [B2](docs/blueprints/archive/web-native-notification.md) |
