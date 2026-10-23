# B2：用户消息的排队提示

**状态：已批准（human 2026-10-23），已实现，本地检查通过，待 human 验收。**

## 目标

用户发出的消息在 agent 还没看到时显示为「排队中」：气泡变淡，加小时钟和「排队中」字样；被 agent 接手后恢复正常。

## 语义

「排队中」= 输入**未认领且非 held**（`UNCLAIMED AND NOT HELD`）。这和模型上下文用来判定「不可见」的是同一个谓词（`context.rs` 的 `read_range`），所以排队提示和「agent 看到没有」一致。

- 输入只经 `claim_next`（开新 run）和 `absorb`（并入执行中的 run）被认领，认领后不再排队。
- 停止执行后，没被认领的输入仍在排队，直到下一轮认领。
- 重启时遗留的输入转为 held，不算排队，由现有的重启通知说明；下一轮它们随新消息一起进入上下文。
- 已归档的会话不显示排队提示（归档后不再认领）。

不新增存储：认领事实就是现有的 `core_messages.run_id`。

## 契约变化

### Kernel 事件：`RunStarted` → `InputsClaimed`

```rust
/// run 认领了这些输入；某个 run_id 第一次出现即该 run 开始。
InputsClaimed { run_id: RunId, inputs: Vec<MessageId> }
```

- `claim_next` 认领后发一次，取代原来的 `RunStarted`。
- `absorb` 并入非空时发一次。
- 现有消费者：Web 设置 `executingRun`，重复赋值无影响；微信 `typing.rs` 已按 run_id 去重；`global_stream` 照旧发 `session_activity`。

### 查询

- Store：`queued_inputs(session_id) -> Result<Vec<MessageId>, StoreError>`，返回该会话未认领且非 held 的输入 id。
- Kernel：同名转发。

### 会话 SSE

- `run_started` 改名为 `inputs_claimed`，数据为 `{ run_id, inputs }`。
- `ready` 增加 `queued: number[]`，和 `executing_run` 在同一处查出。

## Web

`SessionView` 维护一个排队 id 集合，消息组件只负责按集合显示样式：

- 收到 `ready`：用 `queued` **整体替换**集合，重连时也一样。
- 实时收到用户发出的 `UserInput` / `Command`：加入集合。它们落库时一定还没被认领。
- 收到 `inputs_claimed`：从集合删掉 `inputs`，并照旧设置 `executingRun`。
- 会话已归档（`info.archived_at`）时不显示样式。

排队提示属于整条消息，纯图片、纯文件的消息也要显示。发送失败沿用现有的错误处理。

订阅在回放之前建立，回放期间发生的认领事件会排在 `ready` 之后处理；删除不怕重复，所以不会漏，也不会残留。

## 不做

不新增表、字段、迁移、轮询，也不建排队状态机或队列管理器。旧草案中的 `InputsAbsorbed` 事件和 `queued` SSE 事件取消；前端也不再按「思考 / 回复阶段」推断认领。

## 验收

自动：

- `cargo fmt`
- `cargo clippy -- -D warnings`
- `web/` 下 `bun run check`
- 在现有调度测试里补一条 Store 功能测试：`queued_inputs` 在认领和 held 前后的结果。

人工：

- 空闲时发消息：瞬间排队，开始处理即恢复。
- 长输出或长工具执行期间追加消息：保持排队，直到被并入。
- 模型重试期间追加消息：重试本身不会清除提示。
- 执行中发送命令：命令等到下一轮才恢复。
- 停止后未处理的消息仍显示排队；刷新、换设备、重连后状态一致。
- 重启后遗留的消息不显示排队，只显示重启通知。
- 归档后不显示排队提示。
