# B2：预测用户的下一句输入

状态：已批准并实现，待人工验收（2026-10-28）。方向见 [B1](../brainstorm/input-prediction.md)，审查见 [input-prediction review](../reviews/input-prediction/README.md)。

## 行为

- **何时生成**：
  - 只在 Web 主会话里、对话 run 以 `Completed` 结束、并且最后一条回复不带工具调用时（`ask_user` 这类打断 run 等用户的工具都算作未收尾），在后台用杂活模型生成一次；
  - 指令 run、失败或停止的 run、子 agent 会话、微信会话和任务会话都不生成；
  - 杂活模型没配好时跳过，不记调用。
- **取材**：
  - 用 `messages_page` 取最近 `PREDICTION_ROUNDS` 轮，按现有口径以 `UserInput`/`Trigger` 计轮；
  - 最多 `PREDICTION_MESSAGES` 条；只保留文字输入和最终回答，去掉工具调用和思考过程，每条只留末尾 `PREDICTION_SOURCE_CHARS` 字，整理成一段「用户：/AI：」对话记录放进一条消息，请模型以用户口吻接下一句；
  - 凭据脱敏沿用 Provider 层已有的处理。
- **输出**：
  - 一行纯文本，最多 `PREDICTION_CHARS` 字；
  - 输出为空表示猜不出，调用记为成功，但不产生候选；
  - 输出超长、多行、带工具调用或没有正常结束，都记为失败调用。
- **失败与停止**：
  - **只记 warn、不提示用户、不重试**：模型错误、超时（`PREDICTION_TIMEOUT`）、输出不合格。这几种都记一条失败调用，有原文时把原文存进 `SideOutput`，方便在 Trace 里诊断。
  - **取消**：新 run 开始或会话归档时取消，记一条失败调用，原因写「已取消」，和标题一样。
  - **存储错误**：沿用标题 runtime 现有的契约，runtime 退出、服务停止。不另设恢复机制。
  - **正常停服**：直接丢弃 runtime，不等待在途调用，也不补记结果。
  - 每个会话同时只有一个预测。
- **展示**：
  - 候选只对它的来源 run 有效，会话开始新 run 后就作废，前端收到 `inputs_claimed` 时清掉灰字；
  - 用户打字时灰字消失；清空输入框后，如果候选仍属当前 run，就再显示。

## 与 B1 的差异

- **候选从调用事实推导，不放内存**：预测调用和标题调用一样，要记进调用表和 `SideOutput`，Trace 和看板才看得到。「当前候选」由「成功调用、有非空输出、来源 run 仍是最新」推出，内核不再按会话另存一份内存状态，重启后候选也还在。不新增候选表或有效性标记。
- **清空后重新显示**：B1 写的是「开始输入后不再展示」。B2 改为清空输入框后重新显示，不记录「用户已忽略」。

## 契约

- **mic-message**：`SidePurpose` 新增 `Prediction`。
- **mic-store**：
  - `ModelCallPurpose` 新增 `Prediction`。`purpose` 列是不带 CHECK 的 TEXT，不改 schema。
  - `async fn save_prediction(call: NewModelCall, output: Option<String>) -> Result<(), StoreError>`：在同一事务里写调用行和 `SideOutput`。
    - `output` 是模型原文，和标题的语义一致；
    - 调用的 `run_id` 取来源 run，`context.instruction` 取取材消息的 id。
  - `async fn current_prediction(session) -> Result<Option<Prediction { run_id, text }>, StoreError>`：从该会话最新 run 的成功预测调用中，取它的非空输出；没有就返回 `Ok(None)`。读取时不再重复校验长度。
  - Trace：
    - `run_stats` 的模型耗时、步数和请求数都不计入 `Prediction` 调用，run 的总耗时和结束时间不变；
    - 预测调用仍然列在该 run 的调用里，耗时、用量和费用照常可见；
    - 会话用量和看板照常计入。
    - `TraceCall` 新增 `side_preview: Option<String>`：旁路输出原文的开头，Trace 列表直接展示。
- **mic-core**：
  - 新增 `prediction.rs`，handle 加 runtime 的结构沿用 `title.rs`。不复制手动生成、标题错误和 `SessionChanged` 这些标题特有的逻辑，也不抽象通用的杂活框架。
  - `Registry::input_prediction(channel)`：渠道声明自己的 Root 会话要预测；mic-core 不认识具体渠道，Gateway 为 `web` 声明。
  - `run.rs` 在发出 `RunFinished` 之后提交预测任务，同时取消该会话在途的旧预测。
  - `KernelEventKind` 新增 `PredictionReady { run_id, text }`，只在调用成功、输出非空且事务提交后发出。
  - `async fn Kernel::current_prediction(session) -> Result<Option<Prediction>, KernelError>`。
  - Trace 重建预测调用的输入时，和标题一样走 `context.instruction`。
  - `limits.rs` 新增：
    - `PREDICTION_ROUNDS = 5`
    - `PREDICTION_MESSAGES = 60`
    - `PREDICTION_SOURCE_CHARS = 1500`
    - `PREDICTION_CHARS = 60`
    - `PREDICTION_TIMEOUT = 20s`
- **Gateway**：
  - 会话 SSE 新增 `prediction` 事件，内容为 `{ run_id, text }`；
  - `ready` 增加 `prediction: { run_id, text } | null`；
  - 读取失败时按现有回放失败的方式处理。
- **Web**：
  - `Composer`：
    - 输入框为空、又有候选时，显示 ✦ 加灰字，替换原来的 placeholder；
    - 满足三个条件时 Tab 或 → 填入候选：有灰字、不在输入法组合输入中、不在指令补全中；其余情况 Tab 保持原有行为；
    - 触屏设备在灰字末尾显示「填入」按钮；
    - 迟到的 `prediction` 事件，如果 `run_id` 不是当前最新的 run，直接忽略。
  - Trace 与看板：调用用途显示为「预测」，概览展示任务与模型原文；预测挂在收尾回复行上，直接显示预测内容，点开看详情。

## 验收

- **自动**：
  - 预测运行时的模块测试：
    - 正常结束时生成并发出事件；
    - 最后回复带工具调用和指令 run 不生成；
    - 新 run 开始时取消，并记一条取消调用。
  - `current_prediction`：
    - 新 run 开始后返回 `None`；
    - 超长输出在 Trace 里能看到原文，但不会成为候选。
  - Trace 统计：run 完成后追加的预测调用不计入该 run 的模型耗时和请求数。
- **人工**：
  - Web 对话结束后几秒内出现灰字，按 Tab 填入，回车发送；
  - 手机上点「填入」；
  - 刷新页面后候选还在，发出新消息后消失；
  - Trace 能看到预测调用，看板计入它的花费。
