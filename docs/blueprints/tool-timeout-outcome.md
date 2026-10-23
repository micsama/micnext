# B2：工具超时统一为超时终态

状态：2026-10-22 human 批准，2026-10-23 实现完成，待页面验收。

来源：[执行审查 R2](../reviews/design-review-2026-10-22/execution.md#r2执行结束原因在不同路径里被重新翻译成文案)、[human 决定 6](../reviews/design-review-2026-10-22/README.md#human-决定2026-10-22)。

## 行为与公开契约

前台 shell 超时、后台工具超时、子 agent run 超时都以强类型超时终态交付，Web 显示“超时”。仍保留此前收集到的输出；用户停止继续是 Cancelled，非零退出码沿用现行工具结论，本次不改成 Failed。

`mic_message::ExecOutcome` 新增：

```rust
TimedOut { output: Vec<ContentPart> }
```

不另存超时时间快照；具体说明可以是 output 文本，终态判别只读变体。Completed / Failed / Cancelled 的字段不变。

`mic_tool::ToolOutput::Completed(Vec<ContentPart>)` 替换为 `Terminal(ExecOutcome)`；`Dispatched` 不变。工具参数与依赖错误仍走 `ToolError`，由 core 工具执行边界转成 Terminal Failed；成功工具显式返回 Terminal Completed，前台 shell 可以返回 Terminal TimedOut。提供 `ToolOutput::completed(output)` 构造普通成功结果；删除旧 Completed 枚举入口，同步所有工具实现与测试，不保留转发别名。

| 路径 | 结果与输出 |
|---|---|
| 前台 bash 达到 timeoutMs | 杀掉进程组、等待退出和输出收尾；Terminal TimedOut，保留 stdout/stderr 尾部与超时说明 |
| Jobs 的运行时限到期 | 丢弃并清理任务 future，`JobState::TimedOut` 与 Completion 的 TimedOut 在现有 finish_job 事务内提交；输出尾部按原预算保留 |
| BackgroundTask 自身返回 TimedOut | 由同一 finish 路径映射 JobState::TimedOut，不记成 Completed |
| 子 agent RunState::TimedOut | `jobs/agent::delivery` 返回 TimedOut，保留超时说明和已有最终回复（有则按原预算截断）；首次等待和后续交付共用它 |
| 用户或管理者停止 | 沿用 Cancelled 与现行“发起方自己停止不另回注”策略 |

工具超时不会自动重试，也不使主进程退出。与停止同时就绪时沿用各执行拥有者现有 select 优先级，最终只提交一个终态；不新增竞争仲裁器。服务关闭与进程崩溃仍沿既有 Interrupted 恢复，不改称超时。

## 消费者

- `mic-message/model_view` 统一投影 `[timed_out]` 与输出；工具协议仍用正常 ToolResult 消息承载内容，不要求 Provider 改 wire schema。
- `mic-core` 的 Jobs 收尾、agent 交付、结果转文字、测试辅助函数穷尽匹配新增变体。
- `mic-store::ToolTraceOutcome` 新增 `TimedOut`，Trace 预览保留输出；看板工具 `errors` 计 Failed 与 TimedOut，Cancelled 沿用现状不计错误。模型调用成功/费用统计不受此工具终态影响。
- Web `ExecOutcome`、`ToolTraceOutcome` 解码器、`ToolRow`、Completion 展示、Trace 状态与结果文字统一显示“超时”，保留输出展开。聊天工具、后台任务列表和 Trace 使用相同事实。
- 更新 `archive/async-jobs.md`、`archive/tools-basic.md`、`archive/run-trace.md` 的旧投影说明。

## 持久化边界

新增变体能与既有 JSON 并存；现有 `JobState`、`RunState` 已含 timed_out，无需新增数据库状态列或独立 payload 版本。

本契约从实施后的新执行生效。历史消息仍忠实显示当时写入的 Completed/Cancelled，不按 `[timed out ...]` 文案猜改旧事实。历史 Job/Run 自身已有的 timed_out 状态继续按其事实显示；因此旧消息与任务列表可能仍保留旧投影差异。若要求回溯修正历史消息，需先补充可可靠关联的记录范围及不可判定记录的处理，不能在本次实现中暗自扫描文字重写。

## 验收

- 前台 shell 功能测试：真实短超时、stdout/stderr 尾部保留、进程组清理；非零退出仍是现行结论。
- Jobs 功能测试：超时 Completion 与 JobState 一致；停止、正常退出、依赖失败不受影响；重复 finish 不产生第二条 Completion。
- agent 功能测试：TimedOut 返回超时变体，等待与交付一致，已有回复按预算保留。
- Store Trace 与 Web 解码覆盖新变体，旧三种变体仍能读取；不依赖超时文字识别结果。
- `cargo fmt`、`cargo clippy -- -D warnings`、相关 Message/Store/Core/Shell 功能测试、`web/` 下 `bun run check`；human 验收前台、后台和子 agent 的超时展示。

## 实施验证（2026-10-23）

Store/Core/Shell 共 71 项测试通过，新增真实前台 shell 超时并保留输出的验收，后台与 agent 断言强类型 TimedOut。工作区 clippy、Web check 通过。历史消息维持原事实，不扫描文字重写。
