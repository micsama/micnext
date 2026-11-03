# B2：流式草稿随重连恢复并按调用时刻定位

状态：已批准，实现完成，待验收。

## 问题

- 重连时前端会清空草稿，后端只回放已经落库的消息，所以正在生成的那段回答会丢掉前半截。
- 前端在收到第一段增量时取 `lastId` 作为草稿位置。如果此时已有排队输入（B），无论是重连回放进来的，还是在首个 token 之前到达的，草稿都会挂到 B 下面。等 Reply 落库后，它按 `created_at = context_at` 排序又回到 B 上面，于是出现跳位。
- 网关目前的做法是先订阅、再做无上界的回放，然后用 `last` 和 `draft_open` 去重。回放期间如果有 Reply 落库，会先出现、再作废，交接点并不明确。

## 草稿的定义

一次调用尝试对应一份草稿。草稿表是事件流的折叠，在 `Events::emit` 中与发送同锁更新，与有没有订阅者无关。

| 事件 | 草稿表 |
|---|---|
| `DraftStarted { context_at }`（每次尝试开始，包括重试） | 新建 `{context_at, reasoning: "", text: ""}`，取代旧草稿 |
| `TextDelta` / `ReasoningDelta` | 追加 |
| `MessageAppended(Reply)`（成功，或停止时有正文） | 移除 |
| `DraftDiscarded`（失败、重试前、停止时无正文） | 移除 |

- `context_at` 就是 `call_model` 开头取的那个值，同一次调用的所有重试共用它，也就是最终 Reply 的 `created_at`。
- 结束草稿的入口仍然只有 `record`。落库失败或 worker panic 会让调度器返回错误，内核随之停止，草稿表也一起消失，因此不需要额外的清理机制。

## 历史、快照与订阅的切点

把 `Kernel::subscribe_from_now` 扩展为一次原子交接：在发布锁内依次订阅、取当前草稿快照、读最新消息 id 作为 `cut`。

- Reply 的落库和移除草稿也在发布锁内完成，因此只会有两种结果：
  - Reply 落库早于切点：它在历史里，快照中没有这份草稿。
  - Reply 落库晚于切点：快照里有这份草稿，Reply 从订阅里到达。
- 网关回放的历史只取到 `cut` 为止（`messages_after` / `messages_page` 加上界）。`cut` 之后的所有消息只从订阅里到达。
- 删除网关的 `last` 跳过逻辑和 `draft_open` 去重。这样已经回放过的 Reply 不会再变成草稿，旧的终结事件也不会清掉下一份草稿。
- 微信侧已经在使用 `subscribe_from_now`，只需忽略新增的返回值；core 新增公开类型 `ReplyDraft`。

## HTTP 契约（`/sessions/:id/stream`）

- `ready` 新增 `draft: { context_at, reasoning, text } | null`。
- 新增事件 `draft_started { context_at }`。
- `text_delta`、`reasoning_delta`、`draft_discarded` 保持不变。

## 前端

- `Draft` 改为 `{ at, reasoning, text }`，删除 `afterId`：
  - `ready` 用快照建立草稿。
  - `draft_started` 新建草稿。
  - 增量只追加到已有草稿，不再兜底创建。
  - 重连前仍然清空草稿，由 `ready` 恢复。
- 定位规则与落库后的排序一致：草稿放在所有 `created_at <= at` 的消息之后。由于 Reply 在插入时 id 最大，同一毫秒的消息排在它前面。
- 草稿在有内容后才渲染，`draft_started` 本身不显示空气泡。

## 不改

- 数据库 schema 不变。
- 草稿不持久化，进程重启后丢失（run 本身也随之中断）。
- `-p` 调试输出忽略 `DraftStarted`。

## 验收

- 首个 token 之前插入 B：草稿在 B 上方，落库后不跳位。
- 生成中切页再回来：前半截完整，位置在 B 上方。
- 回放期间 Reply 落库：不出现重复，也不会闪一下草稿。
- 重试：作废的旧草稿不残留，新草稿位置相同。
- 只有思考内容时重连：思考内容恢复。
- 停止后重连：有正文的显示为 Reply，没有正文的不留草稿。
- core 测试（已通过）：
  - 生成中取切点，快照包含思考内容和正文，对应的 Reply 在切点之后到达，且 `created_at == context_at`。
  - 停止后、全部失败后，草稿表都为空。
- 收尾跑 `cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。
