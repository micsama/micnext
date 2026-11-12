# todo 蓝图索引

仅列未关闭的 B2，每个主题一行；契约、细节与验收记录见蓝图。待人工验收的条目汇总在[验收清单](docs/acceptance.md)。
已关闭项见 [归档](todo-archive.md)，蓝图存于 `docs/blueprints/archive/`。
产品现状与后续方向见[产品路线](docs/brainstorm/product-roadmap.md)；待定与未形成蓝图的事项见[产品路线](docs/brainstorm/product-roadmap.md#五从-todo-移入的待定与延期事项2026-10-16)。

| 状态 | 主题 | 蓝图 |
|---|---|---|
| 实现完成，待验收 | 流式草稿随重连恢复并按调用时刻定位 | [B2](docs/blueprints/stream-draft-snapshot.md) |
| 实现完成，待验收 | 新会话发送前可关闭凭据脱敏 | [B2](docs/blueprints/new-chat-settings.md)；[B1](docs/brainstorm/new-chat-reuse.md) |
| B2 待批准；混合负载测试先行 | 请求与数据库耗时观测 | [B2](docs/blueprints/storage-request-timing.md) |
| 实现完成，待验收 | Web 改为登录制，取消 token（用户 dzmfg） | [B2](docs/blueprints/web-login.md) |
| 实现完成，待验收 | iPhone 系统通知（Web Push，镜像通知中心） | [B2](docs/blueprints/web-push.md) |
| 实现完成，待验收 | 推送加标题（系统通知、通知中心、IM） | [B2](docs/blueprints/push-title.md) |
| B1 调研完成，不急 | 让 agent 用本机浏览器（Zigzag MCP + SSH 反向隧道） | [B1](docs/brainstorm/local-browser-mcp.md) |
| 暂缓 | 推送降级（微信被拒） | [B1](docs/brainstorm/push-fallback.md) |
| P1 已验收，P2 待验收（调度已修订），P3 已实现、待验收 | 推送与定时任务（P1 推送 / P2 内核 / P3 页面） | [B2](docs/blueprints/scheduled-tasks.md) |
| 已实现，待验收 | 定时任务限频与到点前压缩 | [B2](docs/blueprints/schedule-limits.md) |
| 延期 | 服务商分时段计价 | [B2](docs/blueprints/offpeak-pricing.md) |
| Web 已验收，微信文件待验收 | Web 上传文件，统一文件消息 | [B2](docs/blueprints/web-file-upload.md) |
| 实现完成，待验收 | 微信收尾：投递终态归 core、不重试、日志收敛、入站现场精简 | [B2](docs/blueprints/wechat-wrap-up.md) |
| 实现完成，待验收 | 运行指示显示模型重试次数与等待 | [B2](docs/blueprints/model-retry-indicator.md) |
| 实现完成，待验收 | 图片入库时统一规整（压缩、动图转静图） | [B2](docs/blueprints/image-normalize.md) |
| 实现完成，待验收 | 侧栏状态点、后台更新提醒与置顶 | [B2](docs/blueprints/session-sidebar.md)；[B1](docs/brainstorm/session-sidebar.md) |
| 实现完成，待验收 | 结构化询问用户工具 ask_user | [B2](docs/blueprints/ask-user-question.md)；[B1](docs/brainstorm/ask-user-question.md) |
| 实现完成，待验收 | 模型交付文件：send_file 与 push 附件（Web + 微信出站） | [B2](docs/blueprints/send-file.md) |
| B2 待批准 | 专题：索引与 `topic` 工具（压缩时 fork 整理在 B1 讨论） | [B2](docs/blueprints/topics.md)；[B1](docs/brainstorm/topics.md) |
| 实现完成，待验收 | 内置 skill（首个 web-guide，功能地图移出 Web channel prompt） | [B2](docs/blueprints/builtin-skills.md) |
| B1 待定 | 统一「轮」的口径 | [B1](docs/brainstorm/rounds.md) |
| 实现完成，待验收 | 压缩后重新加载仍在用的 skill | [B2](docs/blueprints/skills-after-compaction.md)；[B1](docs/brainstorm/skills-after-compaction.md) |
| 实现完成，待验收 | 设置里的环境变量 | [B2](docs/blueprints/env-vars.md) |
| 实现完成，待验收 | 命令里的 Python 用固定的 uv venv | [B2](docs/blueprints/python-env.md)；[B1](docs/brainstorm/python-env.md) |
| 实现完成，待验收 | 会话草稿保留与发送前图片预览 | [B2](docs/blueprints/composer-drafts-preview.md) |
