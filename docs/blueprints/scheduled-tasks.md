# B2：推送与定时任务

**状态：已批准（human 2026-10-20），P1 已验收（human 2026-10-20）；P2 修订已批准（human 2026-10-20），已实现、本地自动验收通过，待 human 验收（见 P2.9）。**

依据：[B1](../brainstorm/scheduled-tasks.md)。一份文档分三期，各自实现、各自验收：P1 推送与通知中心（独立可用：长任务做完提醒我）；P2 定时任务内核与工具；P3 定时任务页面。

## 总则

### 一、实体与一处真相

| 实体 | 职责 | 唯一事实来源 |
|---|---|---|
| 推送 | agent 或框架发给某 person 的一条消息，可发多个 Channel | `core_pushes` |
| 推送投递 | 每个 Channel 一行：发往哪、投递状态 | `core_push_deliveries`；微信的送达取对应 `Notification` 消息的 `delivered_at` |
| 定时任务 | 归属 person：名字、规则、指令、启停、任务会话 | `core_schedules` |
| 触发 | 每次到点一行：开跑（关联触发消息，run 经消息取得）或没跑（错过 / 忙） | `core_schedule_fires` |
| 运行 | 任务会话里的一个 run | `core_runs`；结局即 run 状态，是否推送由推送表按 run 关联 |

### 二、放在 mic-core，不另起模块 crate

定时任务要建会话、追加输入、提前压缩、停 run、推送，都是内核职责；模块 `Tool` 在 install 期拿不到 `Kernel`，另起 crate 还得给模块工具开内核通道。所以表进 `mic-store`（`core_` 前缀），调度循环、`schedule`/`schedules`/`push` 三个工具与 `agent` 一样由 core 按会话生成。[mic-core-module](archive/mic-core-module.md) 里「`mic-cron` 等功能模块」一行随本 B2 删除。

### 三、person 的私聊会话

推送到某 Channel = 写进该 person 在该 Channel 的**私聊会话**：`Root { channel, chat = 该 person 在此 Channel 的 external_id }`，即该 Channel 绑定账号的会话（`ChannelSetup::status()` 里的 `session_id`，也是 Channel 实际投递的会话）。微信现状即如此（`chat = user_id`）。以后接入的 IM 须遵守这条约定；群聊不在本 B2。

当前单用户：Channel 新出现的外部身份一律绑到 owner（`resolve_identity` 不再按 Channel 新建 person），所有工具里的「person」都取 `Kernel::owner()`；多 IM 用户绑定以后再议，加登录后改为本轮输入的发送者，不改表。

## P1：推送与通知中心

### P1.1 `push` 工具

| 参数 | 说明 |
|---|---|
| `text` | 推送正文 |
| `via?` | 另发的 IM Channel 名数组，如 `["wechat"]`；省略用默认推送 Channel，`[]` 只进 Web 通知中心 |

每条推送都进 Web 通知中心并计未读；Web 不是可选去向（human 2026-10-20）。

- 所有会话都有。描述写明：只在需要主动告诉用户（用户可能不在看）时调用；普通对话里的回复本来就是对用户说话。
- `via` 里的 Channel 须可用（IM 须已连接，渠道服务未运行如 `-p` 也算不可用），否则整条报错并列出原因，不部分发送。
- 返回：`Pushed to the Web notification center and wechat.`（工具结果给模型看，用英文）。
- 来源会话是子 agent 的 Task 会话时照样记它；`push` 列入 core 工具名，子 agent 也有。

### P1.2 落库与投递

一次推送一个事务：

1. `core_pushes`：`id, person_id, source_session_id, source_run_id?, text, created_at, read_at?`。框架推送（P2 的失败通知）`source_run_id` 为空。
2. 每个 IM Channel 一行 `core_push_deliveries(push_id, channel, message_id?, failure?)`，`message_id` 与 `failure` 恰有其一：在 person 私聊会话追加 `Notification { source: <来源说明>, text }`，`message_id` 指向它，走现有 `pending_deliveries` 投递。通知中心直接读推送表。
3. 发布 `KernelEvent::PushCreated`。

**框架推送**（P2 的失败通知）不走「整条报错」：默认推送 Channel 中不可用的照样建投递行、`failure` 写原因（如「微信未连接」）。推送记录因此总能落库并在通知中心看到。已建 `Notification` 的微信投递沿用现有 `pending_deliveries`，微信恢复后补发。

`Notification` 沿用现有语义：系统发给用户，不唤醒，留在上下文供之后追问。

### P1.3 默认推送 Channel

存 `core_settings.push_channels`（JSON 数组），不进整体 `Settings`（生效值依赖渠道连接状态，单独读写，同标题模型）。Web 设置页「对话偏好 → 推送」多选。可选项只有 IM。未设（空）时：取已连接的 IM，可为空（只进 Web 通知中心）。默认值在读取时算，不写库。设置备份带上它。

### P1.4 HTTP

| 接口 | 说明 |
|---|---|
| `GET /api/pushes?before&limit` | 新的在前；每条含来源会话 id 与标题、来源任务名（P2 起）、各 Channel 投递状态、是否已读 |
| `POST /api/pushes/read` | body `{ ids: [...] }`，只把实际展示过的这些标为已读 |
| `GET` / `PUT /api/settings/push` | 读：`{ channels: 可选, chosen: 用户所选, effective: 省略 via 时实际去向 }`；写：`{ channels: [...] }`，未知 Channel 400 |
| `POST /api/pushes/stream-ticket` → `GET /api/pushes/stream` | 全局 SSE，事件 `push_created { id }`；Web 收到即重拉首页，每次（重）连先补拉 |

### P1.5 Web 通知中心

- 右上角铃铛；有未读推送时显示未读数。点开是下拉列表，打开即把当前可见的标为已读。
- 每条：来源（任务名或会话标题）、时间、正文、各 IM 投递状态。
- 【去会话里问】：跳到来源会话，输入框带引用前缀（复用 `view.reply`）。来源会话里有它自己的 `push` 调用，能接着答；不另开新会话（新会话没有前因后果）。

### P1.6 验收

- 主会话调用 `push`（省略 / 指定 `via`、多 Channel、不可用 Channel 报错）。
- 微信收到，微信里回复主 agent 能接上。
- Web 未读数、已读、回复跳转与引用前缀。

## P2：定时任务内核与工具

### P2.1 触发消息

新增 `MessageBody::Trigger { source: String, text: String }`（mic-message 公开类型）：框架以某个来源的名义发给 bot 的指令，与人工 `UserInput` 区分。

- 渲染头：`[trigger src=<source> at=<本地时刻>]`，与其他消息头同在 `model_view` 定义；消息头说明加一条：「a scheduled instruction from task src, not the user; the user is not watching.」模型据此分辨到点指令与用户直接提问。
- 认领规则同 `Command`：只能独占一个 run，不与前后输入合并。
- 只有定时任务产生；无 person。

### P2.2 表

```sql
CREATE TABLE core_schedules (
  id             INTEGER PRIMARY KEY,
  person_id      INTEGER NOT NULL REFERENCES core_persons(id),
  name           TEXT NOT NULL,
  rule           TEXT NOT NULL,          -- 工具参数 when：cron 表达式或一次性本地时刻
  prompt         TEXT NOT NULL,
  enabled        INTEGER NOT NULL,
  paused_reason  TEXT,                   -- 框架自动暂停时的原因
  active_since   INTEGER NOT NULL,       -- 建任务、改规则、重新启用时刻；补记错过从这里起算
  session_id     INTEGER NOT NULL REFERENCES core_sessions(id),
  created_at     INTEGER NOT NULL,
  updated_at     INTEGER NOT NULL
);
CREATE TABLE core_schedule_fires (
  id            INTEGER PRIMARY KEY,
  schedule_id   INTEGER NOT NULL REFERENCES core_schedules(id) ON DELETE CASCADE,
  due_at        INTEGER NOT NULL,        -- 规则上的时刻；错过合并时为最后一次
  message_id    INTEGER REFERENCES core_messages(id),  -- 开跑时的 Trigger 消息；run 取该消息的 run_id
  skipped       TEXT,                    -- 'missed' | 'busy'；开跑则为空
  missed_count  INTEGER                  -- missed 合并的次数
);
```

删除任务删这两张表的行；任务会话、run、trace 保留。

**结局**由事实推导，不另存：

| 结局 | 推导 | 系统级失败 |
|---|---|---|
| 完成·已推送 / 完成·未推送 | run `Completed`/`MaxTurns`，按推送表 `source_run_id` 区分 | 否 |
| 失败 | run `ProviderFailed` | 是 |
| 超时 | run `TimedOut` | 是 |
| 已停止 | run `Stopped`（人工停止） | 否 |
| 中断 | run `Interrupted`，或 Trigger 消息在重启时未认领而被 held | 是 |
| 错过 | `skipped = 'missed'` | 是 |
| 忙 | `skipped = 'busy'`：到点时会话有执行中的 run 或待处理输入 | 否 |

### P2.3 规则

- `when` 接 5 段 cron 表达式或 `YYYY-MM-DD HH:MM` 一次性时刻，服务器本地时区；写入时 parse，非法报错。一次性的触发后 `enabled = 0`。
- 改规则、`enabled` 由假变真时 `active_since = now`；只改名字、指令不动。
- 新依赖：cron 解析 crate（实现时选型，只需「下一次时刻」）。

### P2.4 任务会话

- 建任务时同事务建会话：`Triggered { module: "schedule", ref_id: <任务 id> }`，`parent_session_id` 为空；`pwd`、人设、模型、思考档位拷自调用会话。之后可在任务会话页面单独改。
- 渠道说明 `channel_prompt("schedule", …)`（所有定时执行共用的执行规则）：触发消息是任务的一次运行，用户多半不在看、回复送不到；需要告知用户时用 push，内容独立可读，无事简短收尾；缺必要信息不猜，用 push 说明；会话里留有历史运行，按最新触发执行；用户直接发来的消息照常回答。
- 工具：全套基础工具 + `push`；不含 `schedule`、`schedules`、`agent`，也不含 `jobs`、`job_output`、`job_stop`。轮数上限同子 agent。
- **不允许后台执行**（human 2026-10-20）：任务会话的后台派发直接拒绝，`bash` 设 `background` 得到 business 错误 `background jobs are unavailable in scheduled tasks; run in the foreground`。一次触发即一个 run：结局、超时与「忙」都只看这个 run，不会有后台任务结束后另起的 run。
- 用户可从任务列表进入任务会话直接对话，与触发共用上下文。触发 run 执行中用户发来的输入，按现有规则在下一个模型调用边界并入该 run。

### P2.5 调度循环

core 内一个 tokio 任务：内存里维护各启用任务的下一时刻，睡到最早的一个；任务增改删时唤醒重算。到点调用 store 的一个事务 `fire_schedule`：

1. 会话有执行中的 run 或未认领的非 held 输入 → 写触发行 `skipped = 'busy'`，结束。
2. 上下文占用超过 `SCHEDULE_COMPACT_PERCENT`（`limits.rs`，暂定 50）→ 先追加 `/compact` 命令。
3. 追加 `Trigger { source: 任务名, text: 当前 prompt }`，写触发行关联该消息。
4. 提交后唤醒会话；现有 `claim_next` 依次认领 `/compact` 与 Trigger 各成一个 run。之后到达的人工输入 id 更大，排在后面。

检查与写入在同一事务里，不会有人工 run 夹进来，也不会只有消息没有触发行。

- 压缩失败：不单独计数与推送，触发照常执行，结局由触发 run 自己给出（占用仍低于普通会话阈值）。
- 超时与轮数：任务会话的每个 run（含人工追问）同子 agent 计时，墙钟超过 `SCHEDULE_RUN_TIMEOUT`（暂定 30 分钟）即停止并落 `TimedOut`。
- 重启：遗留执行中的 run 沿用现有恢复转 `Interrupted`；未认领的 Trigger 按现有规则 held。两者都是「中断」，启动时对本次收尾涉及的触发推送通知。
- 一次性任务在触发（含「忙」）或记为错过时即停用。
- 启动补记错过：对每个启用任务，在 `max(最后一行触发的 due_at, active_since)` 到现在之间按**当前规则**枚举应触发时刻；有则合并写一行 `missed`（`due_at` 取最后一次，`missed_count` 为次数）。暂停期间与旧规则时段不在区间内。不补跑。

### P2.6 系统级失败

上表标「是」的结局由框架以框架推送（来源 = 任务会话）通知「定时任务 X：结局与原因」。与所有推送一样进 Web 通知中心，并发往默认推送 Channel；不可用的 IM 照样建投递行、`failure` 写原因。同一任务最近连续 `SCHEDULE_FAIL_PAUSE`（暂定 3）次触发都是系统级失败时：`enabled = 0`，写 `paused_reason`，并推送。「忙」「已停止」既不计入也不打断连续计数；正常完成打断连续计数。agent 判断「没发现」属于正常完成。

### P2.7 `schedule` / `schedules` 工具

只给常驻模式的 Root 会话（`-p` 没有调度循环，不给）。

| 工具 | 参数 | 行为 |
|---|---|---|
| `schedule` | `id?`、`name?`、`when?`、`prompt?`、`enabled?`、`delete?` | 无 `id`：新建，`name`/`when`/`prompt` 必填（`when` 存为 `rule` 列），新任务默认启用；有 `id`：只改给出字段，`enabled: true` 同时清 `paused_reason`，一个字段都没给报错；`delete: true` 删除，不得带其他字段。返回任务 id 与下次运行时刻（带 UTC 偏移） |
| `schedules` | `id?` | 无 `id`：全部任务的名字、规则、启停（含暂停原因）、下次运行、最近一次结局；有 `id`：该任务最近 N 次触发的时刻、结局、是否推送、最终回复首段 |

工具描述的标签用户语言规则同 `agent` 的 `description`。

提示归位：怎么填参数（任务指令须自包含、写明何时报告什么、时间格式与时区、一次性/重叠/错过的调度契约）写在工具描述与参数说明；每次定时执行都适用的规则写在渠道说明；单个任务的业务目标与通知条件写在任务指令；参数组合与调度策略由代码保证。

### P2.8 验收

- 对话里建、改、停、删任务；`schedules` 两种用法。
- 到点跑、没发现不推送、有发现推送；忙时跳过；超时停止并标超时；停机错过合并记录并通知，暂停与改规则后不误记；连续失败自动暂停。
- 任务会话里人工追问与触发区分正确；触发 run 中人工输入并入。
- 任务会话里 `bash` 后台执行被拒，模型改用前台。
- 默认推送设为微信、微信断开时，失败通知仍在 Web 通知中心可见并标投递失败。
- 提前压缩在两次运行之间发生（把阈值临时调低验证）。

### P2.9 实现记录与验收

实现中相对上文的取舍（已改入正文）：去掉 `timed_out` 列，超时即 run 状态 `TimedOut`；超时与轮数覆盖任务会话所有 run；压缩失败不单独通知；连续失败中正常完成会打断计数；工具只在常驻模式；渠道说明不写任务名（触发头 `src` 已带）。推送列表 `source_schedule` 与私聊会话 `Notification.source` 带任务名（P1.4「来源任务名」）。

本地已验收：store 认领/吸收/忙/结局推导测试；临时库冒烟——每分钟任务到点触发、无模型时失败并推送「运行失败：…」、停机期间错过合并补记并推送、连续 3 次失败自动暂停。

交 human：真实模型下建改停删与 `schedules` 两种用法、没发现不推送/有发现推送、忙时跳过、超时、任务会话人工追问与并入、`bash` 后台被拒、微信断开时失败通知、调低阈值看提前压缩、Web 聊天与 Trace 里的定时指令显示。

## P3：定时任务页面

### P3.1 HTTP

| 接口 | 说明 |
|---|---|
| `GET /api/schedules` | 全部任务：名字、规则（原文 + 人话）、启停与暂停原因、下次运行、最近 10 次触发结局、最近 10 次运行累计用量 |
| `GET /api/schedules/:id/fires?before&limit` | 触发列表：时刻、结局、run id、推送去向与投递状态、最终回复首行 |
| `PATCH /api/schedules/:id` | `name`/`rule`/`prompt`/`enabled` |
| `DELETE /api/schedules/:id` | 删除 |
| SSE `schedule_changed` | 任务或触发有变化 |

### P3.2 Web

- 侧栏「定时任务」入口，一页列表。每行：名字、规则人话、下次运行、启停开关、最近触发状态点、累计用量；暂停原因用警示色写在行内。
- 展开：触发列表，结局见 P2.2 表；「未推送」与「推送了但未送达」分开。点开进任务会话（聊天视图，可直接对话）或 Trace。
- 行内编辑规则与指令，删除需确认。

### P3.3 验收

本地：接口与 SSE；页面观感交 human。

## 调用方影响

| 位置 | 变化 |
|---|---|
| mic-message | 新增 `MessageBody::Trigger` |
| mic-store | 新增 4 张表与读写；`fire_schedule` 事务；`claim_next` 认 Trigger 独占；`core_settings.push_channels`；`resolve_identity` 新身份归给定 person |
| mic-core | `push` / `schedule` / `schedules` 工具；调度循环；`KernelEvent::PushCreated`、`ScheduleChanged`；`Kernel` 增推送与任务读写接口；启用 `SessionKind::Triggered` |
| mic-gateway | P1.4、P3.1 接口 |
| Web | 通知中心、设置页默认推送 Channel、定时任务页 |
| 微信 | 无代码改动；私聊会话约定（总则三）写入其 B2 |
| 文档 | mic-core-module 删「功能模块 crate」一行；[10-19 审计](../brainstorm/over-design-audit-2026-10-19.md) C 组由本 B2 消费 |

## 收尾

每期：`cargo fmt`、`cargo clippy -- -D warnings`、改前端跑 `bun run check`；能本地验收的自己跑完，页面与真实微信交 human。
