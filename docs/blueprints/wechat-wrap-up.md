# B2：微信收尾（投递终态、不重试、日志与入站现场）

状态：2026-10-22 human 批准（含切点前消息标失败、删除旧中断批次），实现完成，待验收。

来源：[设计审查 2026-10-22](../reviews/design-review-2026-10-22/README.md) I3、I4、I5、R3、R5，以及 human 决定 3、4、5。微信进入维护期，这次只删减，不加新能力。

## 目标

1. 一条消息的投递结果只有一个权威：core 的 `delivered_at` / `delivery_failure`。微信模块不再另记终态，去掉两套状态之间的崩溃窗口（I3）。
2. 发送不重试。网络失败时结果未知，按「未知副作用不重放」直接记为失败；主要通知走 Web 推送。
3. 协议日志只记排查需要的信息，不再输出原文（I4）。
4. 入站中断现场只留批次身份、时间和进度，删掉完整快照，以及恢复时对快照的重新解析（I5、R3）。

## 现状问题

- **投递**：微信先写 core 送达，再写本模块 `outcome='sent'`，两次独立提交。中间崩溃后，恢复流程对已送达的消息再写失败，撞上 core 的 CHECK 约束，微信服务启动失败。失败路径和恢复流程自身也有同样的两步窗口。
- **重复判断**：`stored()` 为了判断「这条处理过没有」，解码并复验整份发送计划；四种 outcome 的处理完全一样（R5）。
- **日志**：解码失败时，DEBUG 级输出遮蔽过的响应原文，WARN 级带上 serde 错误文本（可能含原值）。遮蔽名单漏了媒体密钥、二维码等字段。
- **入站快照**：`wechat_inbound_batch.snapshot` 存完整 `getupdates` 响应，里面有 `context_token` 和媒体密钥，诊断 SQL 能读到。恢复时把快照完整再解析一遍，结果没人用。

## 契约

### 投递（delivery.rs）

**终态归属**：送达只写 `kernel.mark_delivered`，放弃只写 `kernel.mark_delivery_failed`，各自一次提交。微信表只保留引用需要的事实：发送计划（每段原文），以及每段的发送记录。

**发送**：每段只发一次。
- 成功：记下外部消息 ID，继续发下一段。全部成功后 `mark_delivered`。
- 失败：`mark_delivery_failed`，原因与现在一致（`只送达前 n 段：…` 或错误原文）。会话过期仍然转入重新登录。

`SEND_ATTEMPTS`、`SEND_RETRY` 删掉。

**恢复**：放在投递任务启动时，每次连接都跑，不只在进程启动时跑。这时本账号的投递循环还没开始，没有并发。对 core 里仍待投递、且已有微信发送计划的消息，`mark_delivery_failed("投递中途中断，可能未送达")`。这一步是幂等的：中途退出，下次再跑一遍就补齐。已送达的消息不在待投递里，不会再撞 CHECK。

运行期的 `stored()` 去掉：能进投递循环的消息，要么没有计划，要么已在启动时收尾。

连接建立时，切点（`subscribe_from_now`）之前就在待投递、且没有发送计划的消息，现在永远停在「待投递」。在同一步一起标记失败（「连接恢复前的消息不补发」），Web 推送的状态因此准确。

### 存储（wechat schema v5）

```sql
-- 终态改由 core 记录
wechat_delivery(message_id PK, user_id, plan)                -- 删掉 outcome、finished_at
-- 每段一次发送，不再有 attempt_no
wechat_delivery_chunk(message_id, chunk_index, client_id, started_at,
                      finished_at, external_message_id, failure,
                      PRIMARY KEY(message_id, chunk_index))  -- 取代 wechat_delivery_attempt
-- 中断现场
wechat_inbound_batch(id PK, user_id, total, imported, phase, created_at, finished_at)  -- 删掉 snapshot
```

迁移（已随[数据库转换移出二进制](database-conversion.md)删除；v5 现为当前 schema，新库直接建表，以下仅记录当时的旧数据处理）：
- `wechat_delivery_attempt` 每段保留结果那一行：有外部消息 ID 的优先，否则取最后一次尝试。
- `wechat_delivery` 删列前，把 `outcome='sending'` 且 core 仍待投递的消息补记为失败（同 v4 的做法），不留给运行时。
- `wechat_inbound_batch` 旧行：`total` 从快照里的消息条数算出；`imported` 无从得知。旧行直接删除（human 同意）：它们只是诊断现场，没有业务消费者。

### 入站批次（account.rs、service.rs）

- 收到非空批次时，写 `total`（消息条数）、`imported = 0`、`phase = 'importing'`，和游标在同一事务。
- 每条消息处理完（落盘、命令或跳过），`imported += 1`。
- 整批完成删除该行，同现在。
- 启动时把 `importing` 的批次标 `interrupted`，不读也不解析任何内容。
- `Updates.snapshot` 字段和 `client::parse_updates` 的公开用途一并去掉。

### 协议日志（client.rs）

解码失败时只记一条 WARN：`endpoint`、字段路径、错误类别、行列、响应长度。`serde_path_to_error` 的路径已包含未知字段名（如 `msgs[0].secret_new`），不另记。删掉 DEBUG 原文、`redacted()` 和 `SECRET_KEYS`，也不记 serde 错误文本（可能带原值）。

## 调用方影响

| 位置 | 改动 |
|---|---|
| mic-channel-wechat delivery.rs | 删除 `Outcome`、`stored`、`finish`、`disk_plan` 的复验；单次发送；启动时恢复 |
| mic-channel-wechat account.rs / service.rs | 批次改为进度计数；恢复不再解析；`Coordinator::serve` 不再在进程启动时调 `delivery::recover` |
| mic-channel-wechat client.rs | `decode` 日志收敛；`Updates` 去掉 `snapshot` |
| mic-channel-wechat schema.rs / limits.rs | v5 迁移；删除 `SEND_ATTEMPTS`、`SEND_RETRY` |
| docs | `archive/wechat-channel.md` 关于两次发送和原文调试的条目加修订说明 |

core、Store、Gateway、Web 的公开接口不变。

## 验收

- 自动测试：
  - ~~v5 迁移在含旧数据的库上执行通过~~：迁移代码已随数据库转换移出二进制删除，该测试不再存在；`schema/tests.rs` 现只校验当前 v5 schema 能与 core 一起建库。
  - 恢复只遍历 core 待投递消息：已送达的不在其中，不会撞 CHECK；已标失败的也不在其中，跑两遍结果一样。构造真实 kernel 成本高，靠代码结构保证，不单独写测试。
- 真实环境（human）：微信收发、引用 bot 的分段回复、断网时发送失败在 Web 显示为失败。
- 收尾：`cargo fmt`、`cargo clippy -- -D warnings`、`cargo test --workspace`。
