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
| 实现完成，待页面验收 | 调用统计统一口径（已知一律计入，未知显示下限） | [B2](docs/blueprints/usage-stats.md) |
| 实现完成，待页面验收 | 压缩只保留必读文件路径 | [B2](docs/blueprints/compaction-file-paths.md) |
| 实现完成，待页面验收 | 前台、后台与子 agent 超时统一终态 | [B2](docs/blueprints/tool-timeout-outcome.md) |
