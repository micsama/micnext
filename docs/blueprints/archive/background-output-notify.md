# 后台 bash 输出匹配通知

状态：CLOSED（2026-10-20 Claude Code review 后修订并实施；human 2026-10-20 验收真实 agent 行为与页面观感通过）。

## 目的与范围

后台派发回执只表示已提交；现有 completion 负责执行结束后的结果回推。本次保留这条主流程，增加执行中输出命中正则时的提前通知，让 agent 不必等进程退出才能行动。

调整后台 bash 入参、输出 sink port、一种新的模型输入消息及相关工具提示。不增加结构化退出结果、查询补偿、等待接口，不修改 `jobs` / `job_output` / HTTP 接口、agent 生命周期和 `eof`。

### review 修订要点

- 原稿让通知走 `MessageBody::Notification` 并调 `Jobs::wake`，两处都不成立：`Notification` 是投递给用户的框架通知，会经 `pending_deliveries` 推到渠道；它也不是 run 输入（`is_input` 只认 `UserInput` / `Completion` / `Command`），唤醒后调度器没有可认领的输入，模型收不到。改为新增只给模型的输入消息 `JobNotice`，因此需要调整消息闭集、输入谓词索引（schema 迁移）和 Web 展示。
- 停止、超时时，在途通知可能晚于 completion 落盘。改为通知与输出日志一起封口：封口前已开始的通知先落完，之后到达的丢弃。
- 原稿每段 16 KiB × 10 次会进入模型上下文，过大。改为每行片段 512 字节，通知正文就是该行或片段。
- 达到次数上限后静默停止，模型无从知道。改为每条通知带序号（如 `notice 3/10`），最后一条注明之后不再上报；提示里要求模型在通知多于预期时用 `job_stop` 主动停掉任务。

## 模型可见契约

`bash` 增加可选字符串入参 `notify_pattern`，只允许与 `background=true` 一起使用。未传时行为保持现状；传给前台或正则无效时，在派发前返回 input 错误，不执行命令。

使用 Rust `regex` 语法，不支持回溯与反向引用；空正则拒绝。不新增正则列表、匹配组模板或通知配置对象。

stdout、stderr 各自按行匹配，不跨行、不混合两路内容；去掉行尾换行后匹配，CRLF 去掉末尾 CR。进程管道 EOF 时匹配未带换行的最后一行。读取分块和 UTF-8 字符分块不影响匹配；非 UTF-8 内容有损解码。

每路待匹配内容最多 512 字节；超长行按该大小分片匹配，通知标明是行片段。正则不跨片段；此限制写入工具说明。

首次命中立即回推。stdout、stderr 共用一个任务内的冷却和次数计数：冷却 10 秒，最多通知 10 次，放在 `mic-tool-shell/src/limits.rs`。冷却内命中直接丢弃，不排队、不补发；第 10 次通知注明此后不再上报，之后停止匹配。最终 completion 不计入上限，也不受冷却影响。两路同时命中时按实际读取顺序处理。

通知文本由 bash 给出：输出来源（stdout / stderr）、序号 `notice N/10`（片段与最后一条另加说明）以及命中的行或行片段。它只说明输出里出现了什么，不代表任务成功、服务可用或已经结束。完整输出仍走原有日志存储。

模型看到的是 user role 条目，头为 `[job-notice exec_id=N]`，说明为"运行中的后台任务发来的中途通知，任务未必结束，结果仍以 completion 送达"。

## 实现契约与依赖

- `mic-tool-shell` 增加 `regex` 依赖，在参数边界编译正则。匹配、逐流缓冲、冷却与计数属于 bash 输出语义，放在该 crate；core 不解释正则。
- `mic-tool::OutputSink` 增加 `notify(&mut self, text: &str) -> BoxFuture<'_, std::io::Result<()>>`：调用方给出通知文本，core sink 绑定任务身份。同步更新所有实现和测试替身，不保留兼容分支。`BackgroundTask` / `Background` 契约不变。
- `mic-message::MessageBody` 新增 `JobNotice { person, tool_name, exec_id, text }`：person、tool_name、exec_id 同该任务的 `Completion`。它是 run 输入（入站未认领，可被认领或在运行中并入），不投递给用户；`model_view` 新增 `job-notice` 方括号头，并同步加入头说明清单。
- `mic-store`：输入谓词与 `is_input` 加入 `JobNotice`，SQL 里的输入 kind 列表收敛为一处常量；迁移 v15 重建 `idx_messages_unclaimed`。启动收尾时遗留的 `JobNotice` 与 `Completion` 同等处置：登记为 held，不触发"有消息被搁置"的用户通知。trace 映射同步。
- core：bash 后台任务拿到的 sink 由日志 `Writer` 与通知去处组成。通知在发起会话（`owner_session_id`，不是树根）追加 `JobNotice`、发布 `MessageAppended`，再走 `Jobs::wake`，沿用其停止与归档时不唤醒的约束；运行中的会话由现有 `absorb` 并入。追加在独立任务里完成并持封口门，不随工具 future 被取消而中断；`drive` 在收尾前与日志一起封口。
- 通知落盘失败沿 sink 的错误链返回；bash 杀掉进程组，由现有终态链路报告 dependency 错误。不重试。
- bash 先按现有路径写日志，再匹配和通知；通知完成后才继续读取，不另设队列。正常结束时已发出的通知都在 completion 之前；主动停止、超时不额外排空未读输出。

## 提示调整

更新 bash 工具说明（`prompt_hint` 已由 [trace-tools](trace-tools.md) 删除）：派发只表示已提交，结束结果稍后以 completion 自动送达；可选正则会在运行中提前送来 job-notice。根据已到达的证据报告状态；非预期失败先处理，再继续依赖其成功的步骤。

去掉"后台任务必须回读或探测后才能说已启动"的机械要求，也不要求收到 completion 后再查一遍。声称服务可用仍需可用性证据，job-notice 本身不能证明服务可用。job-notice 多于预期时，模型应用 `job_stop` 主动停掉任务，不要放任它继续上报。

## 调用方影响

- 模型多了一个可选参数和一种输入条目；不带正则的后台任务和所有前台任务行为不变。
- `OutputSink` 的 core 实现与测试替身随 trait 更新。
- `MessageBody` 新增变体：Web `types.ts` 解码、聊天列表以虚线后台行展示（同 Completion 一侧）、trace 展示随之更新；微信渠道不投递。
- 数据库新增迁移 v15（只重建索引）；匹配器、冷却状态和计数不落库。进程重启后，遗留后台任务仍按现有方式收尾，不恢复执行。

## 验收

- 不配置正则：原有前后台行为、输出和 completion 不变。
- 无效正则、空正则、前台带正则：派发前报 input 错误。
- 两路分别命中，跨读取块与 UTF-8 边界，EOF 尾行，超长行片段，均符合上述契约；不会把 stdout 和 stderr 拼在一起误命中。
- 首次命中、冷却内丢弃、冷却后再次命中、达到上限，用受控时间自动验证。
- 通知在进程结束前作为 `JobNotice` 落到发起会话并唤醒；最终 completion 正常到达；正常退出与非零退出结果不变。
- 通知失败可见；停止、超时、归档后不再出现新的通知。
- 本地跑相关模块功能测试、`cargo fmt`、`cargo clippy -- -D warnings`、`web/` 下 `bun run check`；真实 agent 收到通知后的行为与页面观感由 human 验收。
