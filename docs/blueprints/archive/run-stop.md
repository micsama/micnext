# B2：运行中停止

**现行扩展（2026-10-16，P3）**：普通停止只停当前 run，后台任务由 `job_stop` 管理；归档才停止整棵会话树，并将未认领输入标 held、不再唤醒。归档后的执行请求返回 409。见 [async-jobs](../async-jobs.md) P3 和 [session-archive](session-archive.md)。

**状态：CLOSED（2026-10-15，Web 人工验收通过；微信 `/stop` 走同一停止路径，human 认为无需单独真机验收）。**

依据：[B1](../../brainstorm/run-stop.md)。本文修订 [run-execution](run-execution.md) 的 run 终态与工具批执行方式、[gateway](gateway.md) 接口表、[context-compaction](context-compaction.md) 指令表。

## 一、用户可见行为

1. 生成中，Web 输入框的发送键旁出现停止键；点击后这一轮立即结束，聊天里留下已输出的正文，后面跟一条通知「已停止。」。
2. Web 或微信发 `/stop`：停当前会话正在执行的一轮；没有在执行时回一条通知「当前没有在执行的任务。」。`/stop` 不带参数，带参数报错，与 `/clear` 同规则。
3. 停止前已发出、但还没被这轮并入的消息，停止后照常开新一轮回答。
4. 被中止的工具在聊天和 trace 里显示「已取消」；trace 中被中止的模型调用标「用户停止」，run 状态为「用户停止」。

## 二、契约

### 2.1 数据

| 位置 | 变更 |
|---|---|
| `mic-store` `RunState` | 新增 `Stopped`，库值与 SSE 状态名 `stopped`。`core_runs.state` 无约束，不需要迁移 |
| `mic-store` `StopReason` | 新增 `Stopped`：调用被用户停止，`blocks` 是已收到的部分。库列 `stop = 'stopped'`，无迁移 |
| 工具结果 | 复用 `ExecOutcome::Cancelled { message }`，`message` = `"Stopped by the user before this tool call finished; its side effects are unknown."`，`started_at` 取实际开始时刻 |

### 2.2 内核

```rust
pub enum StopOutcome { Stopping, NotRunning }

impl Kernel {
    /// `run` 为 None 时停该会话当前执行的 run；给定 run 但它已不在执行 → NotRunning。
    pub async fn stop_run(&self, session: SessionId, run: Option<RunId>) -> Result<StopOutcome, KernelError>;
}
```

- `Engine` 与 Jobs 共用 `RunControl` 的会话执行槽位及取消令牌；worker 认领前占槽，run 结束释放。`stop_run` 只取消令牌并返回，不等 run 收尾；收尾结果照常经 `run_finished` 推送。
- 指令解析返回统一枚举，`/stop` 不入队、不生成 run：

```rust
pub enum ParsedCommand { Session(SessionCommand), Stop }
pub fn parse_command(text: &str) -> Result<Option<ParsedCommand>, CommandParseError>; // 取代 parse_session_command
```

`session_commands()` 增加 `/stop`（「停止当前回答」），Web 补全随之出现。Web 与微信在拿到 `Stop` 时调 `Kernel::stop_command(session)`：内部 `stop_run(session, None)`，`NotRunning` 时追加上面的通知。

### 2.3 执行中的取消点

只在等待外部的点响应取消，落盘写入不被打断。取消生效后不再认领输入、不再发起新的模型调用或工具：`absorb` 与每步开头先查令牌，已取消即进入收尾。

| 等待点 | 取消时 |
|---|---|
| 对话模型流 | 丢弃流（关闭上游连接）。已转发的正文增量拼成 `Text` 块，以 `StopReason::Stopped` 走现有 `Replied` 落盘：非空即写 `Reply`，否则作废草稿。推理增量与未完成的工具调用参数不保留（不完整、不能安全回放）。用量记未知 |
| 压缩模型流 | 调用行记 `Stopped`、无 `blocks`，不写 Boundary |
| 重试等待 | 立即结束，不再发起尝试 |
| 工具批 | 中止仍在跑的工具任务；已返回的照常落盘，其余补 `Cancelled` 结果 |

工具批改为：任务只执行工具并返回 `(调用, 开始时刻, 返回时刻, 结果)`，结果由 run 自己按返回顺序落盘并发布。这样中止任务不会截断一次落盘，也不会出现「已写入却补了取消」的重复结果。消息 id 仍表示落盘顺序，不变。

任一取消点命中后：追加通知「已停止。」，以 `RunState::Stopped` 结束 run，发 `RunFinished`。随后 worker 照常 `claim_next`，未并入的输入开新 run。

### 2.4 HTTP

| 方法与路径 | 成功 | 失败 |
|---|---|---|
| `POST /api/sessions/{id}/runs/{run_id}/stop` | 202 | 404 会话不存在；409「这一轮已经结束」（run 不在执行）；只读会话 403 同现有写接口 |

Web 停止键用 `executingRun`（已由 `ready`/`run_started` 提供）调用此接口，绑定 run 防止停错下一轮。文本 `/stop` 经现有发送接口进入，与微信同一路径。

发送接口的响应不再带 `message_id`（Web 未使用，`/stop` 生效时也没有同步产生的消息）：`POST /api/sessions/{id}/messages` 202 无响应体，`POST /api/sessions` 201 `{"session_id"}`；压缩撤回接口同样改为 202 无响应体。新建会话首条即 `/stop` 走同一路径，得到「当前没有在执行的任务。」。

## 三、调用方影响

- `mic-store`：两个枚举各加一值；看板 run 成功率与调用失败率不把 `stopped` 计为失败（分母剔除）。
- `mic-core`：`Engine` 令牌表、`Exec` 各取消点、工具批落盘方式、`parse_command`、`Kernel::stop_run`；`lib.rs` 导出改名。
- `mic-gateway`：新路由；`append` 改用 `parse_command`，不再返回消息 id；发送与新建会话响应去掉 `message_id`。
- `mic-channel-wechat`：同样改用 `parse_command`，`Stop` 分支调 `stop_run`。
- Web：停止键；`Created` 解码去掉 `message_id`；`run_finished` 的 `stopped` 不弹失败提示；trace 的调用行与 run 显示「用户停止」。
- 文档：路线图 §2.4 按 B1 修订；run-execution、gateway、context-compaction 指令表补本条。

## 四、验收

1. core 测试（脚本 Provider + 慢工具）：流中途停 → 半截 `Reply` + 通知 + `stopped`，调用行 `stop = stopped`；工具批中途停 → 已完成的结果在、其余 `Cancelled`，shell 子进程已不在；重试等待中停 → 立即结束；压缩中停 → 无 Boundary；停止后未并入的输入开新 run（含：最终回复落盘期间停止且已有排队消息 → 排队消息不并入本轮，开新 run）；重复停止、停已结束的 run → `NotRunning`。
2. 隔离实例 curl：202 / 409 / 404；`/stop` 空闲时得到通知。
3. Web 与微信真机由 human 验收：生成中点停止、发 `/stop`、停止后再发消息模型能接上。
4. `cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。
