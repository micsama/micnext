# 代码索引

本文给出每项能力的源码入口，小节与 `capabilities.md` 对应。路径都相对仓库根目录，写的是入口、职责和关键符号；文件改名后，用符号搜索仍能找到。测试统一放在模块同名目录下的 `tests.rs`。

**通用验证**：`cargo test -p <crate>`、`cargo clippy --all-targets -- -D warnings`，前端在 `web/` 下运行 `bun run check`。页面观感交给 human 检查。

## 装配与内核骨架

- 装配根：`bin/micnext/src/main.rs`，负责组装模块列表、`--set-password`、`-p`、更新后 exec。
- 模块框架：
  - `crates/mic-core/src/module.rs`：`Module`、`Registry`，模块在 `install` 里登记自己的贡献。
  - `crates/mic-core/src/assembly.rs`：`Assembly`，负责数据目录、锁、建库、启动各 service。
- 内核句柄：`crates/mic-core/src/kernel.rs` 的 `Kernel`。HTTP 和 channel 能用的内核操作都在这里，新增能力先看这里有没有现成入口。
- 事件：`crates/mic-core/src/event.rs`，包括 `KernelEventKind`、`Events`、`Publisher`。
- 启动收尾：`crates/mic-core/src/recovery.rs`。
- 存储：
  - 入口：`crates/mic-store/src/lib.rs`、`crates/mic-store/src/store.rs`。
  - 表目录：`crates/mic-store/src/schema.rs` 里的 `TABLE_CATALOG`。
  - 建表 SQL：`crates/mic-store/src/schema/core.sql`。
  - 版本号：`crates/mic-store/src/schema/version`。
- 旧库转换：`scripts/db/convert.py`、`scripts/db/tests.py`，以及 `scripts/db/` 下按版本命名的 SQL。
- 构建与部署：`build.sh`；一键更新在 `crates/mic-gateway/src/update.rs`。

## 对话与会话

- **前端**
  - 页面：`web/src/components/ChatView.svelte`、`web/src/components/MessageList.svelte`、`web/src/components/MessageItem.svelte`。
  - 输入框：`web/src/components/Composer.svelte`。
  - 新会话：`web/src/components/NewChat.svelte`。
  - 状态：
    - 当前会话：`web/src/state/session.svelte.ts`。
    - 会话列表：`web/src/state/sessions.svelte.ts`、`web/src/components/Sidebar.svelte`。
    - 草稿：`web/src/state/composer-drafts.svelte.ts`。
  - 接口：`web/src/api/client.ts`、`web/src/api/stream.ts`、`web/src/api/sse.ts`。
  - 搜索：`web/src/components/SearchPalette.svelte`。
  - 问卷：`web/src/components/QuestionCard.svelte`。
- **HTTP**
  - 路由表：`crates/mic-gateway/src/service.rs` 里的 `router`。
  - 会话与发送：`crates/mic-gateway/src/api.rs`，`send_message` 负责发送。
  - 会话 SSE 与历史分页：`crates/mic-gateway/src/stream.rs`，`stream` 和 `page`。
  - 全局 SSE：`crates/mic-gateway/src/global_stream.rs`。
  - 搜索：`crates/mic-gateway/src/search.rs`。
  - 标签：`crates/mic-gateway/src/tags.rs`。
- **内核**
  - 入站：`Kernel::append_user_input`，校验在 `crates/mic-core/src/input.rs`。
  - 调度：`crates/mic-core/src/scheduler.rs`，执行槽在 `crates/mic-core/src/control.rs`。
  - 执行：
    - `crates/mic-core/src/run.rs`：`Engine::run`、`execute`、`call_model`、`run_tools`、`absorb`。
    - `crates/mic-core/src/request.rs`：请求组装和系统提示。
    - `crates/mic-core/src/prompts/system.md`：基础提示词。
  - 指令：`crates/mic-core/src/command.rs`，即 `/compact`、`/clear`、`/stop`。
  - `ask_user`：`crates/mic-core/src/ask.rs`。
  - 标题：`crates/mic-core/src/title.rs`。
  - 输入预测：`crates/mic-core/src/prediction.rs`。
- **存储**
  - `crates/mic-store/src/messages.rs`、`crates/mic-store/src/sessions.rs`。
  - 搜索：`crates/mic-store/src/search.rs`。
  - 标签：`crates/mic-store/src/tags.rs`。
  - 图片：`crates/mic-store/src/images.rs`。
- **消息类型与模型视图**：`crates/mic-message/src/lib.rs`、`crates/mic-message/src/model_view.rs`。
- **测试**
  - `crates/mic-core/src/run/tests.rs`
  - `crates/mic-core/src/input/tests.rs`
  - `crates/mic-core/src/title/tests.rs`
  - `crates/mic-gateway/src/api/tests.rs`
  - `crates/mic-store/src/messages/tests.rs`
  - `crates/mic-store/src/sessions/tests.rs`
- **端到端脚本**：`scripts/check-session-archive.py`、`scripts/check-session-title.py`。它们启动 debug 二进制，配临时库和 mock 模型，再驱动 HTTP；部分内容已过时，使用前先核对。

## 上下文管理

- **内核**
  - `crates/mic-core/src/compaction.rs`：`compact_if_full`、`tidy_if_idle`、`trim`、`compact`，以及摘要提示词。
  - `crates/mic-core/src/context_usage.rs`：占用估算和构成拆分。
- **存储**：`crates/mic-store/src/context.rs`，负责上下文窗口和压缩边界。
- **HTTP**：`crates/mic-gateway/src/context.rs`，提供上下文、构成、撤回压缩、指令和 skill 列表。
- **前端**：`web/src/components/ContextMeter.svelte`、`web/src/components/ContextBreakdown.svelte`、`web/src/components/ContextBoundaryView.svelte`。
- **测试**：`crates/mic-core/src/compaction/tests.rs`。

## 工具与后台任务

- **工具接口**：`crates/mic-tool/src/lib.rs`，定义 `Tool`、`ToolOutput`、`ToolError`。
- **文件工具**：`crates/mic-tool-fs/src/lib.rs`，各工具各占一个文件。
- **`bash`**：`crates/mic-tool-shell/src/bash.rs`；`notify_pattern` 在 `crates/mic-tool-shell/src/bash/watch.rs`。
- **`web_fetch`**：`crates/mic-tool-web-fetch/src/fetch.rs`。
- **`web_search`**：`crates/mic-core/src/search.rs`；DeepSeek 搜索在 `crates/mic-provider-openai/src/search.rs`。
- **后台任务运行时**：
  - `crates/mic-core/src/jobs.rs`。
  - 输出段文件：`crates/mic-core/src/jobs/output.rs`。
  - 管理工具：`crates/mic-core/src/jobs/tools.rs`。
  - 中途通知：`crates/mic-core/src/jobs/sink.rs`。
- **`send_file`**：`crates/mic-core/src/send_file.rs`。
- **环境变量**：
  - 内核：`crates/mic-core/src/env_vars.rs`。
  - 存储：`crates/mic-store/src/env_vars.rs`。
  - HTTP：`crates/mic-gateway/src/env_vars.rs`。
- **Python venv**：`crates/mic-core/src/python_env.rs`。
- **前端**：
  - 任务条：`web/src/components/JobsDock.svelte`。
  - 工具行：`web/src/components/ToolRow.svelte`、`web/src/components/tool-results.ts`。
- **HTTP**：`crates/mic-gateway/src/jobs.rs`。
- **测试**：
  - `crates/mic-core/src/jobs/tests.rs`
  - `crates/mic-core/src/jobs/output/tests.rs`
  - `crates/mic-core/src/send_file/tests.rs`
  - `crates/mic-tool-shell/src/bash/tests.rs`
  - `crates/mic-tool-fs/src/write/tests.rs`

## 子 agent

- **内核**：`crates/mic-core/src/jobs/agent.rs`。
  - `spawn_agent`、`follow_up`、`stop_agent`。
  - `delivery`：子 run 交付给父会话的内容。
- **测试**：`crates/mic-core/src/jobs/agent/tests.rs`。

## 定时任务

- **内核**
  - 调度循环、触发与结局：`crates/mic-core/src/schedule.rs`，`run`、`drive`、`fire`、`report`。
  - 工具和 HTTP 共用的操作：`crates/mic-core/src/schedule/operations.rs`。建任务时从调用会话复制工作目录和模型。
  - 规则解析：`crates/mic-core/src/schedule/rule.rs`。
  - 工具 `schedule`、`schedules`：`crates/mic-core/src/schedule/tools.rs`。
- **存储**：`crates/mic-store/src/schedules.rs`，`fire_schedule` 负责认领和结局。
- **HTTP**：`crates/mic-gateway/src/schedules.rs`。
- **前端**：
  - `web/src/components/SchedulesView.svelte`、`web/src/components/ScheduleRow.svelte`、`web/src/components/ScheduleDialog.svelte`。
  - 状态：`web/src/state/schedules.svelte.ts`。
- **测试**：`crates/mic-store/src/schedules/tests.rs`、`crates/mic-core/src/schedule/rule/tests.rs`。

## 推送与通知

- **内核**：`crates/mic-core/src/push.rs`。
  - `Pushes::send` 处理模型的 `push`，`notify` 处理框架通知。
  - `create` 在同一事务里写入推送和投递记录。
- **存储**：`crates/mic-store/src/pushes.rs`。
- **HTTP**：`crates/mic-gateway/src/pushes.rs`，提供通知中心和默认渠道。
- **系统通知**：
  - 后端：`crates/mic-gateway/src/web_push.rs`，包括 VAPID、订阅接口，以及 `run` / `mirror` / `deliver`。
  - 前端：`web/src/lib/webPush.ts`、`web/public/sw.js`、`web/public/manifest.webmanifest`。
- **前端**：
  - 通知铃：`web/src/components/PushBell.svelte`、`web/src/state/pushes.svelte.ts`。
  - 投递状态：`web/src/components/DeliveryChips.svelte`。
  - 设置：`web/src/components/settings/PushSection.svelte`。
- **测试**：`crates/mic-store/src/pushes/tests.rs`、`crates/mic-gateway/src/web_push/tests.rs`。

## 渠道：微信

- **入口**：`crates/mic-channel-wechat/src/lib.rs`，`WechatModule`。
- **连接与入站**：`crates/mic-channel-wechat/src/service.rs`。
  - 媒体：`crates/mic-channel-wechat/src/media.rs`。
  - 引用：`crates/mic-channel-wechat/src/quote.rs`。
- **出站投递**：`crates/mic-channel-wechat/src/delivery.rs`。
  - `run` 读取 `Kernel::pending_deliveries`。
  - 「正在输入」：`crates/mic-channel-wechat/src/typing.rs`。
- **登录与账号**：`crates/mic-channel-wechat/src/login.rs`、`crates/mic-channel-wechat/src/account.rs`。
  - 内核侧的接口：`crates/mic-core/src/channel_setup.rs`。
  - HTTP：`crates/mic-gateway/src/channels.rs`。
- **协议**：`crates/mic-channel-wechat/src/client.rs`、`crates/mic-channel-wechat/src/wire.rs`。
- **存储**：`crates/mic-channel-wechat/src/schema.sql`，这是本模块自己的 schema。
- **前端**：`web/src/components/settings/WechatSection.svelte`。
- **测试**：`crates/mic-channel-wechat/src/client/tests.rs`、`crates/mic-channel-wechat/src/schema/tests.rs`。

## 模型、人设与 skill

- **模型接口**：`crates/mic-core/src/provider.rs`，以及每次调用尝试的观测 `crates/mic-core/src/attempt.rs`。
- **OpenAI 协议族实现**：`crates/mic-provider-openai/src/lib.rs`。
  - Responses：`crates/mic-provider-openai/src/responses.rs`。
  - Chat Completions：`crates/mic-provider-openai/src/chat.rs`。
  - 流：`crates/mic-provider-openai/src/stream.rs`。
  - 错误分类：`crates/mic-provider-openai/src/error.rs`。
  - ChatGPT 订阅凭据：`crates/mic-provider-openai/src/token.rs`。
- **服务商与模型**：
  - 存储：`crates/mic-store/src/endpoints.rs`、`crates/mic-store/src/models.rs`；API key 加密在 `crates/mic-store/src/secrets.rs`。
  - HTTP：`crates/mic-gateway/src/models.rs`。
  - 前端：`web/src/components/settings/EndpointsSection.svelte`、`web/src/components/ModelPicker.svelte`。
- **杂活模型**：`crates/mic-core/src/utility.rs`、`crates/mic-store/src/utility_model.rs`。
- **人设**：
  - 内置人设：`crates/mic-store/src/personas.rs`。
  - HTTP：`crates/mic-gateway/src/settings.rs`。
  - 前端：`web/src/components/settings/PersonasSection.svelte`、`web/src/components/PersonaPicker.svelte`。
- **Skill**：
  - `crates/mic-core/src/skills.rs`：`Skills::list`、`load`、`expand`、`catalog`。
  - 内置 skill 在模块的 `install` 里用 `Registry::skill` 登记，本指南的登记在 `crates/mic-gateway/src/lib.rs`。
- **脱敏**：`crates/mic-core/src/redact.rs`，开关组件是 `web/src/components/PlaintextToggle.svelte`。
- **测试**：
  - `crates/mic-core/src/skills/tests.rs`
  - `crates/mic-core/src/redact/tests.rs`
  - `crates/mic-provider-openai/src/factory/tests.rs`
  - `crates/mic-provider-openai/src/error/tests.rs`
  - 本索引的路径校验：`crates/mic-gateway/src/tests.rs`

## 设置、备份与账号

- **设置**：
  - 存储：`crates/mic-store/src/settings.rs`。
  - HTTP：`crates/mic-gateway/src/settings.rs`。
  - 前端：`web/src/components/settings/SettingsView.svelte`，各分类在同目录的 `*Section.svelte`；状态在 `web/src/state/settings.svelte.ts`。
- **备份**：
  - 编排：`crates/mic-gateway/src/backup.rs`。
  - core 部分：`crates/mic-store/src/backup.rs`。
  - 微信部分：`crates/mic-channel-wechat/src/backup.rs`。
  - 前端：`web/src/components/settings/BackupSection.svelte`。
- **登录**：
  - 后端：`crates/mic-gateway/src/auth.rs`，包括 cookie、限速、改密码和 `set_password`。
  - Gateway 自己的表：`crates/mic-gateway/src/schema.sql`。
  - 前端：`web/src/components/LoginView.svelte`、`web/src/state/auth.svelte.ts`、`web/src/components/settings/AccountSection.svelte`。
- **配置**：
  - Gateway 启动项：`crates/mic-gateway/src/config.rs`。
  - 构建期旋钮：各 crate 的 `limits.rs`，例如 `crates/mic-core/src/limits.rs`。
- **测试**：`crates/mic-gateway/src/auth/tests.rs`、`crates/mic-gateway/src/backup/tests.rs`、`crates/mic-store/src/backup/tests.rs`。

## 观测与诊断

- **Trace 与看板**：
  - 存储：`crates/mic-store/src/trace/mod.rs`。重试归组、耗时、费用都只在这里计算。
  - HTTP：`crates/mic-gateway/src/trace.rs`。
  - 前端：`web/src/components/developer/TraceView.svelte`、`web/src/components/developer/DashboardTab.svelte`。
- **日志**：
  - 采集：`crates/mic-gateway/src/logs.rs`。
  - 落盘：`crates/mic-gateway/src/log_store.rs`。
  - 流：`crates/mic-gateway/src/log_stream.rs`。
  - 前端：`web/src/components/developer/LogsTab.svelte`。
- **SQL**：
  - 存储：`crates/mic-store/src/diagnostic.rs`。
  - 写入把关：`crates/mic-core/src/sql_review.rs`。
  - HTTP：`crates/mic-gateway/src/sql.rs`。
  - 前端：`web/src/components/developer/SqlTab.svelte`。
- **更新**：`crates/mic-gateway/src/update.rs`、`web/src/components/developer/UpdateTab.svelte`。
- **测试**：`crates/mic-store/src/trace/tests.rs`、`crates/mic-store/src/diagnostic/tests.rs`。
