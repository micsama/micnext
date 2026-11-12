# 实现索引

## 分层

`mic-message`（消息类型与模型视图）→ `mic-store`（业务事实持久化）/ `mic-tool`（工具接口）/ `mic-media`（图片规整）→ `mic-core`（Kernel 与执行主路径）→ 实现层：`mic-gateway`（HTTP 与 Web）、`mic-channel-wechat`、`mic-provider-openai`、`mic-tool-*`。实现层彼此不依赖，在 `Module::install` 里向 `Registry` 登记贡献，之后只经 `Kernel` 访问内核；`bin/micnext/src/main.rs` 装配。

关键约束：
- 同一会话最多一个 run，执行槽与停止令牌在 `RunControl`。
- 消息先提交再经 `Events` 的 publisher 发布，发布顺序即提交顺序，消息 id 可作回放游标；事件可丢，订阅方落后按 id 重放。
- 模块只动自己前缀的表（`core_`、`wechat_`、`web_`），经 `Kernel::with_module_tx`。
- 上次遗留的执行由 `crates/mic-core/src/recovery.rs` 在启动时统一收尾。
- 二进制只认当前 schema 版本，旧库用 `scripts/db/convert.py` 转换。

## 入口

| 能力 | 实现入口 | 职责 |
|---|---|---|
| 内核与装配 | `crates/mic-core/src/kernel.rs`、`crates/mic-core/src/module.rs`、`crates/mic-core/src/assembly.rs`、`crates/mic-core/src/event.rs` | 对外操作、模块登记、启动装配、事件 |
| 存储 | `crates/mic-store/src/store.rs`、`crates/mic-store/src/schema/core.sql` | 存储入口、core 表结构 |
| 会话执行 | `crates/mic-core/src/scheduler.rs`、`crates/mic-core/src/control.rs`、`crates/mic-core/src/run.rs`、`crates/mic-core/src/request.rs` | 调度、执行槽、执行循环（模型调用、工具、并入新输入）、请求组装 |
| 入站与指令 | `crates/mic-core/src/input.rs`、`crates/mic-core/src/command.rs`、`crates/mic-core/src/ask.rs` | 输入校验、`/compact` `/clear` `/stop`、`ask_user` |
| 消息与模型视图 | `crates/mic-message/src/lib.rs`、`crates/mic-message/src/model_view.rs` | 消息类型、历史在请求里的呈现 |
| 上下文 | `crates/mic-core/src/compaction.rs`、`crates/mic-core/src/context_usage.rs`、`crates/mic-store/src/context.rs` | 压缩与空闲整理、占用估算、上下文窗口 |
| 工具 | `crates/mic-tool/src/lib.rs`、`crates/mic-tool-fs/src/lib.rs`、`crates/mic-tool-shell/src/bash.rs`、`crates/mic-tool-web-fetch/src/fetch.rs`、`crates/mic-core/src/search.rs`、`crates/mic-core/src/send_file.rs` | 工具接口与各工具 |
| 后台任务 | `crates/mic-core/src/jobs.rs`、`crates/mic-core/src/jobs/tools.rs` | job 运行时、输出、管理工具 |
| 子 agent | `crates/mic-core/src/jobs/agent.rs` | 派出、追问、停止、结果交付 |
| 定时任务 | `crates/mic-core/src/schedule.rs`、`crates/mic-core/src/schedule/operations.rs`、`crates/mic-core/src/schedule/tools.rs`、`crates/mic-store/src/schedules.rs` | 调度循环与触发、工具与 HTTP 共用操作、工具、触发事务 |
| 推送 | `crates/mic-core/src/push.rs`、`crates/mic-gateway/src/web_push.rs` | 推送与投递记录、系统通知 |
| 微信 | `crates/mic-channel-wechat/src/service.rs`、`crates/mic-channel-wechat/src/delivery.rs`、`crates/mic-channel-wechat/src/login.rs` | 入站、出站投递、扫码登录 |
| 模型 | `crates/mic-core/src/provider.rs`、`crates/mic-provider-openai/src/lib.rs`、`crates/mic-core/src/utility.rs` | 模型接口、OpenAI 协议族实现、杂活模型 |
| Skill 与脱敏 | `crates/mic-core/src/skills.rs`、`crates/mic-core/src/redact.rs` | skill 目录与加载、出站脱敏 |
| Web 后端 | `crates/mic-gateway/src/service.rs`、`crates/mic-gateway/src/stream.rs`、`crates/mic-gateway/src/auth.rs` | 路由表、会话 SSE、登录 |
| Web 前端 | `web/src/components/ChatView.svelte`、`web/src/components/Sidebar.svelte`、`web/src/api/client.ts` | 会话页、侧栏、接口 |
| 设置与备份 | `crates/mic-store/src/settings.rs`、`crates/mic-gateway/src/backup.rs` | 用户可调项、备份编排 |
| 观测 | `crates/mic-store/src/trace/mod.rs`、`crates/mic-gateway/src/logs.rs`、`crates/mic-core/src/sql_review.rs`、`crates/mic-gateway/src/update.rs` | Trace 与看板计算、日志、SQL 写入把关、一键更新 |

测试在模块同名目录的 `tests.rs`；端到端脚本在 `scripts/`（`check-*.py`）。
