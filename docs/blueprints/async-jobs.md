# B2：后台任务与子 agent

**状态：APPROVED（2026-10-16），按 P1 → P2 → P3 顺序实施。**

依据：[B1](../brainstorm/async-jobs.md) 及其 review 后 human 确认。一份文档分三期：总则为三期共用契约，P1/P2/P3 各自实现、各自验收，实现某期只需读总则加该期。批准前不覆盖现行 B2，不改代码。

## 总则

### 一、已确认的行为与范围

- 子 agent 本质是 `SessionKind::Task` 的 session。没有“交付后结束”的生命周期：当前 run 跑完即休眠，追问或后台结果到达时照常处理新输入。
- 后台工具任务与子 agent 共用任务管理入口（列表、读输出、终止），不共用执行状态机：前者状态来自任务执行，后者是否执行中来自现有 run。
- 普通停止键和 `/stop` 只停当前 run，不动后台任务。
- 归档停止整棵会话树的执行与后台任务，历史保留（P3）。
- 重启只留用户可见的中断事实，不自动执行任何会话。
- Web 是用户与 Root 会话的界面：子 agent 只由父 agent 派出和追问，用户在聊天页不进入、不直接对话子会话；子会话的过程只在 trace 里可查。
- 费用在 trace 用“包含子 session”开关控制汇总范围；上下文占用只算当前 session。

不做：同步子 agent、提醒正则、递归派子 agent、codemode、定时任务；不预留无消费者的接口。

### 二、实体与一处真相

| 实体 | 职责 | 唯一事实来源 |
|---|---|---|
| session | 上下文、父子关系、工具范围、模型选择 | `core_sessions` |
| run | 一次认领输入后的执行、状态、调用与用量 | 现有 run / 调用 / 消息记录 |
| tool job | 一次后台工具执行的状态与输出 | `core_jobs` + 输出文件 |
| agent job | 管理工具可引用的子 session 身份与发起归属 | `core_jobs` 关联子 session；执行状态查 run |

`JobId` 为整数 newtype，由 `mic-store` 定义；模型和消息里的 `exec_id` 是其十进制字符串。一个 agent job 对应一个子 session，追问不新建 job。父子关系只存 session 表。

`core_jobs` 字段：

- 共用：`id`、`kind`（`tool` / `agent`）、`owner_session_id`、`person_id`、`origin_run_id`、`origin_tool_call_id`、`tool_name`、`description`、`created_at`。
- `(origin_run_id, origin_tool_call_id)` 唯一，恢复时据此找回原调用对应的 job。
- tool：`timeout_ms`（NULL 不限时）、`state`、`finished_at`、`report_message_id`。状态 `running → completed / failed / timed_out / cancelled / interrupted`。命令、退出码等工具细节不进表，输出字节数由输出文件推出：命令在原 tool call 参数里，退出信息在 Completion 正文里。
- agent：`child_session_id`（唯一）。运行状态不另存，回注记在子 session 的 `core_runs.report_message_id`。

`report_message_id` 非空即已回注，不另建关联表；正文只存 Completion 消息。

### 三、回注规则

- 回注 = 写一条 `Completion` + Wake，沿用现有顺延：owner 空闲则开新 run，运行中则在边界吸收，结束边界由 Dirty 接住。
- 终态、Completion、`report_message_id` 同事务提交，事件与 Wake 在提交后发。退出/超时/终止的竞争由条件终态更新决定唯一胜者，只回注一次。
- 回注对象：tool job 回注 owner；agent job 回注父 session。子 agent 名下的后台任务先唤醒子 agent，子 run 再交付父 session。
- 终止的回注：模型经 `job_stop` 终止的，发起方已从工具返回获知，不写 Completion（owner 是另一子 session 时仍告知 owner）；用户在 Web 终止的，给 owner 写「已被用户终止」；归档整树停止只记事实，不唤醒。
- `MessageBody::Completion` / `ToolResultOutcome::Dispatched` 结构不改；一个 agent job 可产生多条 Completion，各对应一次子 run。

### 四、管理工具与接口（三期共用，按期扩展覆盖的 job 种类）

| 模型工具 | 参数 | 行为 |
|---|---|---|
| `jobs` | `include_old=false` | 列自己和后代的任务；活跃在前，其余按最近活动与 ID 倒序；不分页 |
| `job_output` | `job_id, offset?, limit=16384` | tool job：缺省读尾部，给 offset 按字节读，返回 `next_offset`、`eof`、`available_from`；agent job：返回最近一次交付 |
| `job_stop` | `job_id` | tool job 终止；agent job 停当前 run 及名下后台任务，session 保留可再追问 |

管理范围是“自己 + 后代”，不能凭 ID 操作兄弟任务。`Kernel` 对应 `jobs(session, include_old) -> Vec<JobView>`、`job_output(session, job, offset, limit) -> JobOutput`、`stop_job(session, job) -> StopJobOutcome`，session 为管理者。

HTTP（`/api` 下，沿用认证；越界 403、不存在 404、非法参数 400、归档/清理中 409）：

| 接口 | 返回 |
|---|---|
| `GET /sessions/{id}/jobs?include_old=false` | 200 `JobView[]`，与模型 `jobs` 同口径 |
| `GET /sessions/{id}/jobs/{job}/output?offset=&limit=16384` | 200 `JobOutput` |
| `POST /sessions/{id}/jobs/{job}/stop` | 200 `{outcome: stopped / already_idle / already_finished}`，收尾完成后返回；只读会话 403 |

`JobView`：ID、工具名、归属 session、描述、创建时间，加按 `kind` 区分的详情：`tool` 为状态、结束时间、时限、输出字节数与受限尾部；`agent` 为子 session、是否执行中、最近 `RunState`、最近活动时间。agent 的 `output` 返回最近一次交付整段，不接受 `offset`。

`KernelEventKind::JobsChanged`（session_id 为所属 Root）：job 状态变化、子 run 开始/结束时发布，列表收到后重查；输出增长每秒最多通知一次。沿用 SSE 重连/ready 重查，不建可靠队列。

### 五、牵连契约

| 文档/调用方 | 修订 | 期 |
|---|---|---|
| mic-message、provider-port | 类型不改；Completion 头说明随交付语义更新 | P1/P2 |
| mic-store | `core_jobs`、`core_runs.report_message_id`、`RunState::TimedOut`、子树查询、原子回注、归档 | P1–P3 |
| mic-tool、tools-basic | `ToolOutput`、`Background` port、工具与测试迁移；bash 后台参数 | P1 |
| mic-core-module、run-execution | 任务运行时、派出提交点、写者表、启动收尾、子 session 设置 | P1/P2 |
| run-stop、session-archive | 当前 run 停止与整树停止区分；归档语义 | P3 |
| system-prompt-layout、context-breakdown | 子 session 无 persona、任务为输入、工具表一处组装 | P2 |
| gateway、web-ui、run-trace、chat-run-stats | 任务接口与列表、trace 查看子会话与 usage 开关 | P1–P3 |
| bin/micnext | `-p` 后台任务退出前清理；不暴露 `agent` | P1/P2 |
| product-roadmap、cross-check、B1 | 以本文替代旧 run_in_background 方向 | P1 |

schema 随整体数据库迁移，不另设版本号。logs 仍只记框架运行与错误；任务状态、输出、回注、trace 使用业务库事实。常量放所属模块 `limits.rs`，不新增配置或设置项。

## P1：后台工具任务（bash 后台）

### P1.1 架构

后台执行是 core 的通用能力，core 不认识 shell：工具把一个自持数据的 future 交给 core，core 登记 job、spawn、计时、终止、落输出、回注。终止与超时就是丢弃该 future，复用 `Tool::execute`“丢弃 = 取消”的既有契约（bash 丢弃即杀进程组）。不新增 crate、不新增 `Registry` 登记项；`mic-tool-shell` 只需要 `mic-tool` 的 port。

### P1.2 mic-tool 契约

```rust
pub enum ToolOutput {
    Completed(Vec<ContentPart>),
    Dispatched { exec_id: String },
}

pub trait OutputSink: Send {
    fn write<'a>(&'a mut self, bytes: &'a [u8]) -> BoxFuture<'a, std::io::Result<()>>;
}

pub struct BackgroundTask {
    pub description: String,
    pub timeout: Option<Duration>,
    /// 丢弃返回的 future = 取消，须释放全部资源（如杀进程组）。
    pub run: Box<dyn FnOnce(Box<dyn OutputSink>) -> BoxFuture<'static, ExecOutcome> + Send>,
}

pub trait Background: Send + Sync {
    /// 登记即提交；返回 exec_id。
    fn dispatch(&self, task: BackgroundTask) -> BoxFuture<'_, Result<String, ToolError>>;
}
```

- `Tool::execute`、`ToolHandle::invoke` 成功值改为 `ToolOutput`；现有工具显式返回 `Completed`。
- `ToolContext::new(cwd, background: Arc<dyn Background>)`，新增 `background()`；不保留旧构造。port 实例由 core 每次调用创建，绑定 session、run、person、tool_call_id；工具不能冒用身份，`ToolContext` 不暴露 Store / Kernel。
- 后台 future 返回的 `ExecOutcome` 只描述工具自身结论（如退出码）；输出尾部与字节数由 core 拼进 Completion。

### P1.3 输出

- core 提供的 sink 写数据目录 `jobs/<id>/` 下的段文件（每段 256 KiB，以全程起始偏移命名），运行中可读。只保留最近 1 MiB（按整段丢弃），可读范围与总字节数由段文件推出，不进库；偏移是全程字节偏移，`available_from` 标出最早可读位置。不因输出多而终止任务。
- 单次读取上限 64 KiB，文本边界按 UTF-8 修整；读取或落盘失败不当 EOF。sink 写失败由工具按 `Failed(Dependency)` 结束。
- 回注带最后 4 KiB、总字节数和 `job_output` 引用。文件是输出唯一副本，数据库不存全量。完成前先刷出再提交终态。

### P1.4 bash

- 新增 `background: bool = false`。`timeoutMs` 边界解析为 `Default / Finite / Unlimited`；后台允许 0 = 不限时，前台 0 仍报 input 错误。前台默认 120 秒、上限 600 秒不变；后台默认 10 分钟，正整数不截顶。
- 前后台共用同一进程执行逻辑，只是输出收集分别为内存尾部与 sink。stdout/stderr 合并，按读到的顺序写。后台 stdin 关闭；不支持命令自行 daemonize 脱离进程组。
- 返回 Dispatched 不代表服务就绪；提示词要求 agent 查看输出或探测成功后再说“已启动”。

### P1.5 派出交接与停止

1. 工具调用 `background().dispatch`；core 在数据库登记 job，登记即提交点，提交前取消不建任务。随后 spawn，所有权归任务运行时，丢弃原工具 future 不影响已提交任务。
2. 收尾工具批时对照 job 表：已提交调用一定落 Dispatched；未提交且被停止的才补 Cancelled。
3. 超时、`job_stop`、Web 停止均丢弃 future 并按条件终态收尾。普通停止键不触及。
4. micnext 正常退出丢弃全部后台 future；`-p` 同样在退出前清理，不为后台结果延长寿命。

### P1.6 重启收尾

沿用启动待命，不新增唤醒分支：worker 启动前把遗留 `running` 的 tool job 记 `interrupted`，写 Completion（`report_message_id` 非空跳过）；与其他未处理输入一起 held，不 Wake；该 Completion 在聊天中即可见，不另落 Notification。不重放命令，不按持久化 PID 盲杀；正文写明“执行中断，外部进程状态未知”。

### P1.7 Web

聊天页右侧任务列表：工具名、描述、状态、耗时、输出尾部，可终止、可展开读输出。只列当前 Root 名下任务；有任务时标题栏出现开关，宽屏默认展开，窄屏浮层。收到 `jobs_changed` 与 `ready` 时重查。

### P1.8 验收

1. 前台 bash 行为不变；后台成功 / 非零退出 / 启动失败 / 超时 / 不限时 / 大量输出（只留最近 1 MiB、`available_from` 正确）可观测，进程组能清理。
2. 派出提交前后分别触发停止：已提交的一定 Dispatched 且不同时收到 Cancelled；提交后崩溃按 `(origin_run_id, origin_tool_call_id)` 找回，不重放。
3. 完成与终止并发、重复终止只有一次完成事实；模型终止不回注，Web 终止回注「已被用户终止」。
4. 父繁忙 / 空闲 / 结束边界回注均不丢。
5. 登记、运行、提交、恢复各阶段退出后重启：不补跑、不唤醒，通知可见且不重复。

## P2：子 agent

### P2.1 子 session 设置

- `SessionKind::Task { parent_tool_call_id }` 保留，表示创建它的调用，不随追问改写。
- 不复制父历史：模型解析成父当前所用模型，工作目录/推理设置继承，任务说明作为首条 `UserInput`。保留框架 instructions 与用户 general_prompt，去掉 persona 与 Channel 提示；父 agent 须在 task 里交代必要上下文。压缩、trace、上下文构成走同一路径。
- 工具表 = 父工具范围 ∩ core 子任务白名单（文件、shell、web fetch、管理工具），去掉 `agent`；不扩大父权限。子 agent 与管理工具由 core 生成，与登记工具合成一张最终工具表，重名装配失败；请求与 system prompt 共用此表。
- 追问的 person 取发起本次追问的 person；回注父 session 用本次子 run 的 person。

### P2.2 agent 工具

| 参数 | 行为 |
|---|---|
| 新建 `task, description`；追问 `job_id, task`（互斥） | 写子 session 输入并唤醒，返回 Dispatched；追问返回同一 job ID |

新建时同事务创建 job、子 session 与首条输入，登记即提交点，交接规则同 P1.5。

### P2.3 限制与交付

- 子 run `max_turns = min(全局 max_turns, 12)`，沿用 run-execution §4.3 的计数：最多 12 轮可用工具的对话调用，用尽后额外一次无工具总结；重试与上下文压缩调用另计，不是所有上游请求合计最多 12 次。墙钟上限 30 分钟，超时取消收尾，run 记新增的 `RunState::TimedOut`（不借用表示用户主动停止的 `Stopped`）。
- 每个子 run 收尾写一次 Completion：成功取本次 run 最后一条无工具调用的回复；空产出、失败、轮次上限、超时、停止、中断明确写状态，不拿上一轮回复冒充。终态、Completion、`report_message_id` 同事务。
- 交付上限 32 KiB，超出带截断标记与子 session 引用。
- 追问与后台 Completion 同时到达沿用 claim/absorb，同一 run 吸收的只回注一次。
- `job_stop` 停 agent：关闭该子树派出/认领入口，取消当前 run，终止名下后台任务，已排队未认领输入标 held，清理期间的完成事实只记录不 Wake；完成后释放。休眠 session 不需要“杀”，之后追问照常。
- 停止由谁发起决定交付：用户 → `Cancelled "Stopped by the user."`；owner 模型自己 `job_stop` → 不写（结果已由 `job_stop` 返回）；其他祖先 → 注明发起会话。子会话未在执行且名下无运行任务时返回 `already_idle`。停止期间入口闸门由 core 内的执行登记表持有，与执行槽位一处管理。
- 本期不加全局并发或累计费用预算。

### P2.4 可见性

- 超过 14 天无活动、休眠且无活跃后代任务的子 session 默认不列出；`include_old` 列全部；按 ID 追问不受影响。已结束 tool job 同窗口过滤。不删除任何历史。最近活动由已有输入/run/job 事实查询，不另维护时钟。
- Task 不进侧栏；Web 普通发送接口拒绝 Task。任务列表里 agent 显示描述、运行/休眠、最近结果，不提供进入会话或追问；子会话过程在 trace 可查。
- 重启：遗留子 run 按 P1.6 收尾，并在所属 Root 追加一条聚合通知列出相关 job ID（与收尾同事务，不重复）；休眠子 session 不标中断。
- `-p` 不暴露 `agent`（派出的子 agent 必然收不到交付）。

### P2.5 验收

1. 子 session 首次执行、休眠、追问、失败无正文、停止半截、轮次上限、墙钟超时：每次 run 只回注一次，不重复旧交付。
2. 子 agent 名下后台任务先唤醒子 agent，再交付父 session；父各边界不丢；追问与回注并发正常。
3. 不能跨兄弟操作；子 agent 工具范围不超过父、无 `agent`。
4. 停 agent 后排队输入不复活，再追问正常执行。
5. 旧子 session 默认隐藏、活跃的始终可见；重启聚合通知不重复，休眠 session 不误标中断。

补充自动审查（2026-10-16）：3 项临时测试通过，跑完移除，不增加常驻测试或依赖。使用 `cargo test -p mic-core --features tokio/test-util review_remaining_ -- --nocapture`：

- 虚拟时间推进超过 30 分钟：实际取消挂起的模型调用，子 run 落 `timed_out`，父会话收到超时交付。
- 构造超过 14 天的任务时间戳：默认列表过滤、含旧任务查询可见；名下有运行中后台任务时仍可见。
- 以子会话的 port 登记受控后台任务：完成后唤醒休眠子会话，再向父会话交付。验证 core 回注链路，不等于模型调用真实后台 bash 的完整端到端验收。

### P2.6 已知边界（低优先级，human 2026-10-16 确认暂不处理）

单进程个人 agent 当前接受以下极短窗口中的交错，不阻塞 P2 提交与验收；不为此新增回执表、锁或状态机：

1. **追问写入后、父会话的 Dispatched 落盘前退出**：子会话已有追问，但恢复只能按创建调用关联 job，本次追问调用可能被闭合为 Cancelled、结果未知。父模型之后可能重复追问。此问题需要在提交窗口中退出，不代表正常追问会丢失。
2. **停止与追问同时发生**：追问通过停止检查后，停止方先清理完排队输入；随后追问才写入，而 Wake 因仍在停止中被跳过。父模型可能收到 Dispatched，但该输入暂不执行，之后可能被另一条输入唤醒。此问题不需要重启，但要求停止与追问恰好交错。

审查已用临时测试构造出上述交错，尚无日常使用中的发生频率证据。未来如实际影响使用，再评估统一提交边界；当前不以理论可发生为由扩展协调机制。

## P3：归档联动与费用范围

### P3.1 归档

- 前端归档前用 `jobs` 列表与会话执行状态提示“还有 N 个任务在运行，归档会终止它们”；归档操作本身即停止授权。
- Kernel 先写归档标记（标记即闸门，执行/认领/派出入口见标记即拒绝），再取消主/子 run、标题任务与全部后台任务，待认领输入标 held，全部结果只记事实、不唤醒，收尾完成后返回 204。停止失败如实报错，标记不回滚。
- 归档后保持归档态（human 2026-10-16 定）：拒绝新消息、压缩等一切执行入口，知道链接也不能发送；Gateway 返回 409，Web 显示已归档并禁用输入；历史、trace、任务输出仍可读。`Session` / 会话详情暴露 `archived_at`。这修改了现行契约“归档只隐藏，已知 URL 仍可发送”。
- 恢复不在本文：7 天内由 Web 恢复（另起，见 todo），超期手工清库中 `archived_at`。恢复只清标记；归档时 held 的输入留作历史，不补跑。

### P3.2 trace 费用范围

- Store / Kernel 新增 `session_usage(session, include_children) -> SessionUsage`；HTTP `GET /trace/sessions/{id}/usage?include_children=false`。
- 返回 input / output / cache_read tokens、complete 标志与 cost，口径与现有 trace 共用，前端不再算。开启时递归包含所有后代（含归档、隐藏的），每条调用只算一次；后台工具不产生模型费用。
- 开关默认关，只影响此汇总；run 列表、聊天 run-stats、单次调用费用、上下文占用不变。trace 从任务入口可直接打开子 session。

### P3.3 验收

1. 归档期间持续派出/追问/发消息，收尾后无执行或排队输入复活；重复归档成功；已归档会话写接口 409，历史可读；清标记后恢复正常发送。
2. trace 开关只改变用量/费用汇总，不重复计算。

## 收尾

每期：`cargo fmt`、`cargo clippy -- -D warnings`，前端改动在 `web/` 跑 `bun run check`；不为纯字段映射堆单元测试。本地自动验收用脚本 Provider、临时数据目录和受控 shell。任务列表、归档提示、trace 开关观感与真实 dev server 由 human 验收。三期全部通过后标 CLOSED，todo 条目移入 todo-archive。
