# B2：意外结束的 run 原地重试 / 继续

状态：CLOSED（2026-10-26 human 验收通过）。

来源：human 2026-10-26 提出。出现「模型暂时不可用……已尝试 3 次，请稍后重新发送」这类失败时，用户现在只能把原话再发一遍，导致上下文里同一句话出现两次。意外打断或失败的场景，都应该能原地重试或继续。

## 行为

- **哪些情况可以接续**：会话最近一个 run 意外结束，会话空闲，之后也没有新输入。
  - 失败（`provider_failed`，包括压缩失败）：按钮叫「重试」。
  - 达到调用上限（`max_turns`）、服务中断（`interrupted`）、定时任务超时（`timed_out`）：按钮叫「继续」。
  - 用户主动停止（`stopped`）不算意外，不提供按钮。
  - 子 agent 会话不提供，因为结局已经回报给父会话。
  - 用户发了新消息，新 run 本来就会带上之前没答到的输入，按钮随之消失。
- **接续做什么**：把这个 run 从终态改回 `executing`，在当前上下文上接着执行，不新增任何消息。
  - 本 run 里最后一条回复之后的输入，算作还没答复的输入。
  - 压缩指令如果还没压完，就重做压缩；已经压完，就接着处理附带的文字。
  - 设置和模型按接续时会话的当前值确定，可以先换个模型再重试。调用上限和超时也重新计算。
  - 失败的调用记录和原来的通知都保留，trace 里能看到同一个 run 前后的调用。
  - 再次意外结束时，按现有规则收尾，按钮出现在新的末条消息旁。
  - 定时任务的执行记录跟着变成新的结局，之前已推出去的失败推送不撤回；再次失败时照常再推一次。
- **入口**：只在 Web 上提供，按钮在消息列表末尾的结束提示行里（如「模型调用失败，详情见服务日志 · 重试」），刷新后仍在。微信这次不做，用户照旧重新发送。

## 契约

- **store**：
  - `resumable_run(session) -> Option<Resumable { run_id, state }>`：按上面的条件判断。这是唯一的判断来源。
  - `resume_run(session, run) -> Option<ClaimedRun>`：在同一事务里重新做同样的判断，把 run 置回 `executing`、清空 `finished_at`。设置和模型的确定方式与 `claim_next` 共用同一段代码。输入的还原规则：
    - 压缩指令还没留下边界时，还原为原 `Command`。
    - 其他情况还原为 `Conversation`，`message_ids` 是本 run 最后一条 `Reply` 之后的输入。
- **core**：
  - Kernel 新增 `resume_run(session, run)`：先用 `resumable_run` 校验，不满足时返回 `KernelError::NotResumable`；满足时在 `RunControl` 登记请求，再唤醒会话。
  - worker 认领时先处理登记的请求，`resume_run` 返回 `None` 时回到 `claim_next`。之后的执行路径与新 run 相同，照常发出 `inputs_claimed` 和 `run_finished`。
  - `KernelEventKind::RunFinished` 增加 `resume: Option<Resumable>`，在 run 收尾后读取。
- **Gateway**：
  - 新增 `POST /sessions/{id}/runs/{run_id}/resume`。成功返回 202；不可接续返回 409「这次执行已无法继续」；会话只读时按现有规则拒绝。
  - SSE 的 `ready` 和 `run_finished` 增加 `resume: { run_id, state } | null`。
- **Web**：有 `resume` 时，在结束提示行末显示按钮，提示文案按 `state` 给出。`state` 为 `provider_failed` 时叫「重试」，其余叫「继续」。点击后调用接口；收到 `inputs_claimed`，或本会话出现新的用户输入或指令时，隐藏按钮。等待计时从认领时刻算起，因为接续不新增消息。

无 schema 变更。

## 验收

- **本地**：
  - store 测试：可接续的判断，包括最近一个 run、各种终态、有新输入、子会话这几种情况；以及输入还原：未答复的输入，压缩指令是否已压完。
  - core 功能测试：失败的 run 接续后在同一个 run 里完成，且没有新增消息。
  - 收尾检查：`cargo fmt`、`clippy`、`cargo test --workspace`、`bun run check`。
- **human**：
  - 断网或填错 key 制造失败后点「重试」，确认就地恢复回答；换模型后再重试能生效。
  - 服务中断、达到上限后点「继续」，能接着做完。
