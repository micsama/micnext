# B2：文件写入取消与实际收尾

状态：CLOSED（2026-10-23 实现与自动验收完成；2026-10-22 human 授权审查剩余项一并实施）。

来源：[执行 E2、R3](../../reviews/design-review-2026-10-22/execution.md)。

## 取舍

只在 rename 前读取消标志，无法保证停止返回后文件不再变化；把全部文件工具改成异步 I/O，也不能取消已经提交的底层阻塞操作。选择让已有工具调用上下文拥有少量明确的写入工作：排队中的工作可中止，已经开始的工作由 core 等待实际收尾。不提供文件回滚或通用事务框架。

## 契约

- `mic_tool::ToolContext` 增加 `commit(f)`、`cancel()`、`wait_commits()`。commit 接受 Send + 'static 的同步闭包，在 Tokio blocking 池执行，返回 `Option<T>`；取消先发生则 None，否则 Some(实际结果)。此入口只用于必须收尾的文件变更，读工具仍用现有合作式取消。
- 每次工具调用独立持有取消令牌与 TaskTracker。commit 注册工作后才 await；future drop 会 abort JoinHandle，因此尚未开始的 blocking 工作不再执行。已开始的同步闭包不可强杀，由 tracker 保留真实生命周期。
- blocking 闭包进入时检查令牌；检查通过是“允许本次文件变更执行”的开始点。检查之后即便取消抢先于 syscall，也必须等待闭包结束，不能假定原子 rename 能消除这段窗口。
- core 停止工具批次时先取消各上下文，再 abort 异步任务。确认任务已 drop 后等待该上下文的 tracker 清空，再写取消 ToolResult；最后才允许 run 落停止终态。已经正常返回的结果仍按正常结果提交。
- `write`、`edit` 的同步文件操作经 commit 执行；排队时取消不会创建目录、临时文件或替换目标。已经开始的写入可能完成，取消结果继续明确“副作用可能已经发生”，不谎称回滚。
- 等待已开始的 OS I/O **不另设超时**：超时后放任写入继续会重新违反停止后不变更的承诺。进程强杀／崩溃仍无法等待，恢复按既有 Interrupted 与结果未知处理，不重放写入。
- 已 dispatch 的后台任务仍归 Jobs；本次不改注册、通知和已接受的追问竞态。

## 依赖与调用方

mic-tool 增加已在工作区使用的 tokio（rt）和 tokio-util（rt）依赖；不改变 crate 分层。ToolContext 的构造参数保留，由 new 初始化每次调用的拥有者。core 的 started 表保留该上下文，fs 的 write/edit 使用新入口；其他工具不需要增加空实现或取消标志。

## 验收

- 单 blocking 线程被占住时排队 commit，取消并 drop 调用后释放线程，目标文件保持不存在；覆盖原审查的确定性复现。
- 已开始的闭包受控暂停，取消后 `wait_commits` 必须保持等待，释放闭包后才结束；目标副作用在返回前发生，返回后没有残留工作。
- core 停止批次功能测试确认先 drain 再记 ToolResult/结束 run；已有 shell 取消和后台登记测试继续通过。
- cargo fmt、cargo clippy、Tool/FS/Core 相关测试。本项可自动验收，不要求新增人工确认。

## 实施验证

Core 43 项、FS 1 项测试通过；队列占满时取消真实 Write 不落盘，已开始工作未收尾前 core 不提交取消 ToolResult。Store 落盘失败的退出路径同样取消、drain 剩余工作再返回错误。工作区 clippy 通过。
