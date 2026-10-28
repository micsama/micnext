# input-prediction 蓝图审查

日期：2026-10-28。范围：[B2](../../blueprints/input-prediction.md)、[B1](../../brainstorm/input-prediction.md)、todo 索引及现有调用存储、标题 runtime、Trace、会话 SSE / Composer。只审查，不修改蓝图或实现。

## 结论

方向和规模基本合适。复用调用表与 `SideOutput`，从来源 run 推导当前候选，可以省掉内核里的候选副本；不需要新增候选表、过期字段、独立版本号或通用杂活任务框架。todo 和 B1 到 B2 的链接正确。

批准前建议补齐以下三处。前两处会直接影响显示结果，第三处是失败契约与复用对象不一致。

## R1 · P2：关联来源 run 会把后台预测计入已结束的对话耗时

蓝图位置：`docs/blueprints/input-prediction.md:22`、`:33`。

当前 `load_run` 按 `c.run_id` 取全部调用，`run_stats` 把全部调用时长相加，并计入步数与请求数（`crates/mic-store/src/trace/mod.rs:422–476`）。Web 把它显示为该轮总耗时的组成部分（`web/src/components/developer/TraceTab.svelte:463`）。标题没有这个问题，因为其调用的 `run_id` 为 `None`。

例如对话 2 秒结束，随后预测花 8 秒：按蓝图直接实现，会出现「本轮总耗时 2 秒，模型耗时约 10 秒」。这不是用途名称改成“预测”就能解决的。

最小修改：保留来源 `run_id` 关联，明确预测调用不计入对话执行耗时、执行步数与请求数；预测自身的耗时、用量和费用仍可查看，仍计入会话与看板总账。同步说明 Trace 的展示口径，不延长已完成 run 的结束时间，也不另造一个 run。补一个“run 完成后追加预测调用”的统计功能测试即可。

## R2 · P2：未区分诊断原文和可用候选，失败输出也可能被恢复

蓝图位置：`docs/blueprints/input-prediction.md:22–26`。

标题路径先收集模型原文，再校验；校验失败时仍将原文存为 `SideOutput`，调用行记录失败（`crates/mic-core/src/title.rs:313–357`、`crates/mic-store/src/title.rs:48–59`）。因此“和标题一样，有输出就写 SideOutput”不意味着这条输出可供用户接受。

例如预测返回 80 字，按行为契约属于失败，但有非空原文，且来源 run 仍是最新。当前 `current_prediction` 的文字契约只检查来源 run，会把它恢复成候选；`PredictionReady` 的“只在输出非空时发出”也缺成功条件。

最小修改：明确 `text` 保存原文还是通过校验的候选。若继续沿用标题的原文语义，则查询只从成功调用的非空输出推导候选，事件也只在校验成功且事务提交后发出。利用调用行已有的成败事实，不增加 `valid` / `accepted` 字段，不在读取时重复长度校验。补一个“超长输出可在 Trace 诊断，但刷新后没有候选”的功能测试。

## R3 · P2：失败仅 warn 与照搬标题 runtime 的失败范围不一致

蓝图位置：`docs/blueprints/input-prediction.md:10`、`:22–27`。

标题 runtime 遇到 `StoreError` 会退出（`crates/mic-core/src/title.rs:172–179`），装配层随后停止整个服务（`crates/mic-core/src/assembly.rs:274–278`）。正常停服则直接 drop runtime，不等待在途调用补记结果（同文件 `:294–297`）。因此“结构和 title.rs 一样”不能替代本功能的失败与停止契约。

最小修改：写清“只记 warn”覆盖哪些失败，以及存储失败沿用进程失败契约还是只终止本次预测；明确运行中取消是否落调用结果、正常停服是否等待落盘。不增加恢复或自动重放机制。Store / Kernel 的公开签名补上 `async`、`Result<…, StoreError/KernelError>`，不要让 `None` 同时表达“没有候选”和“读取失败”。这些选择应在 B2 确认，不能由实现时自行扩大失败范围。

## 非阻塞的简化与实施备注

- B2 的“清空输入后再显示”比 B1 的“开始输入后不再展示”更明确，也更省状态；把它列入“与 B1 的差异”，批准后统一文字即可，不建议增加用户已忽略候选的持久化标记。
- “最近 3 轮”需要明确计数口径。现有 `title_sources` 分别取最近 N 条输入和 M 条回答，并非按 run 配对；一个 run 也可吸收多条输入。选定需要的语义即可，不为此建立通用上下文选择器。
- 生命周期可沿用现有 handle/runtime 模式，但不必复制标题特有的手动生成、标题错误展示或 `SessionChanged` 通知，也不必为了两个用途先抽象通用执行框架。
- 迟到结果与 SSE 重连是已有事件机制下的具体场景：实现时验证旧预测不会在 `inputs_claimed` 清空后再次覆盖新状态；使用已有 run 身份和取消顺序，不另加序号体系。

验证：静态对照蓝图与当前实现；`git diff --check` 通过。本次只新增审查记录，未运行 Rust / 前端构建测试。
