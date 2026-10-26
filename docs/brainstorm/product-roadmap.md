# micnext：产品方向与后续工作

**状态：现状索引（2026-10-26 更新）。已实现能力与验收记录以对应 B2 为准；本页记录剩余方向，不授权实现。**

## 一、项目定位

单人自用、单进程的 Rust agent：常驻 Gateway（Web）与微信 Channel，`-p` 一次性调试。支持在服务器上调用文件、shell 与网络工具。定时任务与推送已实现，可用于巡检、日报与提醒，仍在验收；长期记忆属于后续能力。

按独立变化轴建模，边界解析成强类型，失败可见；不做分布式、多租户 SaaS、通用工作流编排。配置、存储与分层遵循仓库 AGENTS.md。

## 二、已落地能力与契约入口

| 能力 | 现行文档 |
|---|---|
| Web 多会话、分页、实时流与断线回放 | [Web](../blueprints/archive/web-ui.md)、[分页](../blueprints/archive/web-message-paging.md)、[EventSource](../blueprints/archive/web-sse-eventsource.md) |
| 微信绑定、文本收发、引用、入站图片/文件/视频 | [微信](../blueprints/archive/wechat-channel.md)、[V2 能力索引](wechat-v2.md) |
| 模型与人设设置、图片能力、思考档位 | [模型设置](../blueprints/archive/model-settings.md)、[图片能力](../blueprints/archive/model-image-support.md)、[思考档位](../blueprints/archive/reasoning-model-capabilities.md) |
| 文件、shell、网络工具与唯一执行主路径 | [基础工具](../blueprints/archive/tools-basic.md)、[执行](../blueprints/archive/run-execution.md) |
| 后台 shell、可继续对话的子 agent、停止与归档 | [后台任务](../blueprints/archive/async-jobs.md)、[停止](../blueprints/archive/run-stop.md)、[归档](../blueprints/archive/session-archive.md) |
| 上下文压缩、保留规则与占用展示 | [压缩](../blueprints/archive/context-compaction.md)、[保留](../blueprints/archive/compaction-retention.md)、[构成](../blueprints/archive/context-breakdown.md) |
| Trace、整树用量、逐回复统计 | [Trace](../blueprints/archive/run-trace.md)、[会话树](../blueprints/archive/trace-session-tree.md)、[回复栏](../blueprints/archive/chat-reply-footer.md) |
| 开发者日志、只读 SQL、更新重启与设置备份 | [诊断](../blueprints/archive/developer-diagnostics.md)、[日志](../blueprints/archive/developer-log-history.md)、[更新](../blueprints/archive/self-update.md)、[备份](../blueprints/archive/settings-backup.md) |

公开契约与实现记录见 B2；旧 B1 中的参数名、配置方案和实现顺序不覆盖后续 B2。用户可调项存 SQLite 经 Web 修改，`config.toml` 只放启动项。

## 三、当前收尾

[todo](../../todo.md) 是未关闭主题的状态索引；当前已实现项主要剩页面与真实环境验收：

- [定时任务与推送](../blueprints/scheduled-tasks.md)：P1 已验收；P2 内核及调度修订、P3 页面已实现，待完成停机合并补记、恢复触发、页面操作及真实投递终态等剩余验收。
- 微信与外部交互：[错误链日志](../blueprints/external-error-observability.md)待真实环境验收；[统一文件消息](../blueprints/web-file-upload.md)的 Web 已验收、微信文件待验收；[微信收尾](../blueprints/wechat-wrap-up.md)已实现，待验收。
- Web 与对话交互：[模型重试提示](../blueprints/model-retry-indicator.md)、[图片规整](../blueprints/image-normalize.md)、[侧栏状态与置顶](../blueprints/session-sidebar.md)、[ask_user](../blueprints/ask-user-question.md)均已实现，待验收。
- [数据库转换移出二进制](../blueprints/database-conversion.md)：已实现，待真实网页更新验收。

继续延期或暂缓：[分时计价](../blueprints/offpeak-pricing.md)由 human 明确搁置，继续按高峰价计；[推送降级](push-fallback.md)暂缓。

已关闭项见 [归档索引](../../todo-archive.md)。归档 B2 中明确延期的个别验收场景仍保留原记录，不因本次整理补记通过。

## 四、后续方向与待收敛问题

### 定时任务

方向已于 2026-10-20 收敛，见 [B1](scheduled-tasks.md)；现行契约、实现与剩余验收见 [B2](../blueprints/scheduled-tasks.md)。每个任务拥有一个共享上下文的任务会话，到点追加触发输入；任务属于 person，不属于创建它的会话。模型通过 `push` 按需推送给人，框架记录并通知系统级失败；Web 任务页读取任务、触发、运行与推送事实，不调用模型。

早期的「每次运行新建子会话、干净上下文加交接笔记」及 `send_message(to?, via?, text)` 设想已被上述方案取代。一次性/周期规则、时区、停机漏触发、忙时跳过、上下文压缩与管理页面均已在 B2 定义并实现，不再列为待定方向；“未请求推送”与“请求后投递失败”仍按不同事实展示。

### 其它后续能力

- 微信出站媒体：入站能力已完成，出站仍只有文本；需先定模型侧发送文件能力，见 [微信 V2](wechat-v2.md)。
- Skill、长期记忆、Hook、Webhook、其它 Provider 与浏览器/语音能力：路线想法，尚无本页授权的实现计划；按实际需求独立立项。
- 代码整理：[10-19 审计](over-design-audit-2026-10-19.md) 保留候选；修改公开签名或删除公开类型均先走 B2。旧正交审查按现状重核，不作为新功能前置。

原 v0–v3 是早期范围划分，实际已交叉交付；不再据此判断功能是否完成或强制实施顺序。早期依据保留在 [9-23 核对记录](cross-check-2026-09-23.md) 与 [历史架构草案](next-gen-architecture.md)。

## 五、从 todo 移入的待定与延期事项（2026-10-16）

以下保留原有决定与线索，形成独立 B2 后再进入 `todo.md`；不代表实现授权。

| 状态 | 主题 | 文档 |
|---|---|---|
| 想法，以后做（2026-10-15） | trace「复制为 curl / 重新推理」：消息按记录还原，系统提示与工具清单用当前配置；工具清单视为固定，与历史工具调用对不上则拒绝重放 | [trace B1](run-trace.md#未来重放与复制为-curl) |
| human 明确延期（2026-10-13） | 自建人设支持设置 emoji；当前统一使用 🎭，以后独立定义契约 | [记录](../blueprints/archive/web-ui.md#43-设置页与人设选择) |
| 历史审查，余项须按现状重新核对；不作为实现前置 | 最小正交审查余项 | [`docs/brainstorm/orthogonality-review-2026-09-24.md`](orthogonality-review-2026-09-24.md) |
| 已进入 `todo.md`，P1 已验收，P2/P3 待验收（2026-10-24） | 定时任务、运行记录与按需推送 | [B1](scheduled-tasks.md)、[B2](../blueprints/scheduled-tasks.md) |
| 脱敏首版已完成（2026-10-24），规则待按实际漏遮、误遮优化；工具隔离仍待定 | 模型出站脱敏、工具隔离与授权：脱敏只求减少部分意外披露；工具隔离既有缺口仍待定。专用用户部署 human 进行中，服务器 bwrap 已验证可运行；访问与披露各自控制，候选未批准 | [B1](privacy-permissions.md)、[脱敏 B2](../blueprints/archive/outbound-redaction.md)；合并「工具授权」缺口（cross-check §四-8～9） |
| 已知 bug，低优先级；human 明确延期（2026-10-10） | 微信 Markdown/表格部分引用无法精确定位时提供完整被引消息并注明；显示文字的精确还原后续再处理，不阻塞本轮引用验收 | [引用 B2](../blueprints/archive/wechat-quotes.md)；[样本事实](wechat-v2.md) §三 |
| 想法，未定；现状够用（2026-10-12） | 日志页开关「落盘包含 DEBUG」：存 SQLite，跨重启有效，仅项目 crate，开启时页面提示。场景：新功能临时调试时复现与查看之间有重启；现状 DEBUG 只在内存实时可看，重启即丢 | [B2](../blueprints/archive/developer-log-history.md) |
| 想法，待起 B1（2026-10-16） | 会话回收站：侧栏下方列出 7 天内归档的会话，可恢复；超过 7 天是否真删待定 | [归档 B1](session-archive.md) |
| 想法，未定 | 人设绑定 UI 主题；「理性大脑」人设定稿 | [`docs/brainstorm/runtime-settings.md`](runtime-settings.md) §三 |
| 后续需求，按实际问题立项 | 入站去重与重复投递、数据长期保留与清理；现有限额、停止与退出契约见对应 B2，不重新列为未实现 | [微信 B2](../blueprints/archive/wechat-channel.md)、[执行 B2](../blueprints/archive/run-execution.md) |
