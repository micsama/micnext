# todo 蓝图索引

仅列未关闭的 B2，每个主题一行；契约、细节与验收记录见蓝图。
已关闭项见 [归档](todo-archive.md)，蓝图存于 `docs/blueprints/archive/`。
产品现状与后续方向见[产品路线](docs/brainstorm/product-roadmap.md)；待定与未形成蓝图的事项见[产品路线](docs/brainstorm/product-roadmap.md#五从-todo-移入的待定与延期事项2026-10-16)。

| 状态 | 主题 | 蓝图 |
|---|---|---|
| 暂缓 | 推送降级（微信被拒） | [B1](docs/brainstorm/push-fallback.md) |
| P1 已验收，P2 待验收（调度已修订），P3 已实现、待验收 | 推送与定时任务（P1 推送 / P2 内核 / P3 页面） | [B2](docs/blueprints/scheduled-tasks.md) |
| 延期 | 服务商分时段计价 | [B2](docs/blueprints/offpeak-pricing.md) |
| 实现完成，待真实环境验收 | 外部交互错误链日志 | [B2](docs/blueprints/external-error-observability.md) |
| Web 已验收，微信文件待验收 | Web 上传文件，统一文件消息 | [B2](docs/blueprints/web-file-upload.md) |
| 实现完成，待验收 | 微信收尾：投递终态归 core、不重试、日志收敛、入站现场精简 | [B2](docs/blueprints/wechat-wrap-up.md) |
| 实现完成，待验收 | 运行指示显示模型重试次数与等待 | [B2](docs/blueprints/model-retry-indicator.md) |
| 实现完成，待验收 | 图片入库时统一规整（压缩、动图转静图） | [B2](docs/blueprints/image-normalize.md) |
| 实现完成，待验收 | 侧栏状态点、后台更新提醒与置顶 | [B2](docs/blueprints/session-sidebar.md)；[B1](docs/brainstorm/session-sidebar.md) |
| 实现完成，待验收 | 结构化询问用户工具 ask_user | [B2](docs/blueprints/ask-user-question.md)；[B1](docs/brainstorm/ask-user-question.md) |
| 实现完成，待真实网页更新验收 | 数据库转换移出 Rust 二进制 | [B2](docs/blueprints/database-conversion.md) |
| 实现完成，待验收 | 联网搜索工具 web_search（DeepSeek Search） | [B2](docs/blueprints/web-search.md) |
| 实现完成，待验收 | 模型交付文件：send_file 与 push 附件（Web + 微信出站） | [B2](docs/blueprints/send-file.md) |
| 实现完成，待验收 | 模型收藏（选择器只列收藏） | [B2](docs/blueprints/model-favorites.md) |
