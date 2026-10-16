# B2：后台 shell 与子 session

**状态：DRAFT（2026-10-16），待 human 批准；不是实现授权。**

依据：[B1](../brainstorm/async-jobs.md) 及其 review 后 human 确认。本文是联合契约草案；批准前不覆盖现行 B2，不改代码。

## 一、已确认的行为与范围

- 子 agent 本质是 `SessionKind::Task` 的 session。没有“交付后结束”的生命周期：当前 query（实现中的 run）跑完即休眠，追问或后台结果到达时照常处理新的输入。
- shell 与子 session 共用任务管理入口，但不共用执行状态机。shell 状态来自进程执行；子 session 是否执行中来自现有 run。
- 普通停止键和 `/stop` 只停止当前 run，不动异步任务。任务列表的终止操作另见 §六。
- 归档表示这棵会话树不再需要：停止主 session、所有子 session 的执行与后台 shell，完成收尾再归档。历史保留。
- 重启只留下用户可见的中断事实，不自动执行普通会话或子 session。未来定时任务独立发起 query，本次不做调度或补跑。
- Web 与模型默认不列出长期休眠的子 session；隐藏不删除，也不妨碍按 ID 追问。
- 费用在 trace 用“包含子 session”开关控制汇总范围；上下文占用仍只算当前 session。

本次包括后台 shell、异步子 agent、任务查询/输出/终止、Web 列表、归档联动与 trace 费用范围。同步子 agent、提醒正则、递归派子 agent、codemode、定时任务均不做；不预留无消费者的同步接口。

## 二、实体与一处真相

### 2.1 Session、run、job

| 实体 | 职责 | 唯一事实来源 |
|---|---|---|
| session | 上下文、父子关系、工具范围、模型选择 | 现有 `core_sessions` |
| run | 一次认领输入后的执行、状态、调用与用量 | 现有 run / 调用 / 消息记录 |
| shell job | 一次后台进程执行与输出位置 | 新增 job 表与输出文件 |
| agent job | 管理工具可引用的子 session 身份及发起归属 | 新增 job 表关联子 session；执行状态查询 run |

新增 `JobId`（整数 newtype，由 `mic-store` 定义）。模型和消息中的 `exec_id` 用其十进制字符串表示。一个 agent job 对应一个子 session，追问不创建新的 agent job。父子关系只存在 session 表，job 不再存另一棵树。

### 2.2 存储新增字段

`core_jobs`：

- 共用：`id`、`owner_session_id`、`person_id`、`origin_run_id`、`origin_tool_call_id`、`created_at`、`description`、`kind`。
- agent：`child_session_id`，唯一；不存一份与 run 重复的运行状态。
- shell：`command`、`workdir`、`timeout_ms`（NULL 为不限时）、`state`、`started_at`、`finished_at`、`exit_code`、`signal`、`output_bytes`。输出文件位置由 job ID 派生，不存重复路径。
- shell 状态：`pending → running → completed / failed / timed_out / cancelled / interrupted`；启动失败也落为 failed。
- `(owner_session_id, origin_tool_call_id)` 唯一，重复提交返回已有 job，不重复启动。

`core_job_reports` 只保存回注关联：`job_id`、`child_run_id`、`message_id`。shell 每个 job 最多一条，agent 每个子 run 最多一条；分别用唯一索引约束。不复制产出正文，正文只存 Completion 消息。

agent 初始任务与追问都写子 session 的 `UserInput`；person 取发起本次调用的 person。自动唤醒产生的 Completion 使用该后台任务保存的 person；向父回注用本次子 run 的 person，不能永远使用最初创建者。

子 session 最近活动时间由已有输入/run 与 job 活动事实查询，不为展示再维护一份时钟。默认过滤只隐藏休眠且没有活跃后代任务的旧子 session。

### 2.3 对已有类型的影响

- `SessionKind::Task { parent_tool_call_id }` 保留，字段表示创建该子 session 的调用，不随追问改写。
- `MessageBody::Completion` / `ToolResultOutcome::Dispatched` 结构不改；关联细节由 job/report 表表达。一个 agent job 可以产生多条 Completion，分别对应不同子 run。
- agent 任务视图状态仅为 `running` / `sleeping`，最近执行结果另带 `RunState`。终止一次执行不永久封禁 session；后续显式追问可以再次唤醒。
- shell 超时记 job 的 `timed_out`，回注使用 `ExecOutcome::Cancelled` 并说明超时；不为此给所有 run 新增超时状态。

## 三、模块边界与公开 port

不新增 crate。`mic-tool-shell` 不能依赖 `mic-core` / `mic-store`；派出和监视通过 `mic-tool` port 依赖倒置。

### 3.1 mic-tool

以下为待批准的新公开契约：

```rust
pub enum ToolOutput {
    Completed(Vec<ContentPart>),
    Dispatched { exec_id: String },
}

pub struct ShellDispatch {
    pub command: String,
    pub description: String,
    pub workdir: PathBuf,
    pub timeout: Option<Duration>,
}

pub trait BackgroundShell: Send + Sync {
    fn dispatch(&self, request: ShellDispatch)
        -> Pin<Box<dyn Future<Output = Result<String, ToolError>> + Send + '_>>;
}

pub struct ShellExit {
    pub exit_code: Option<i32>,
    pub signal: Option<i32>,
    pub timed_out: bool,
    pub output_limit_reached: bool,
    pub output_bytes: u64,
}

pub trait ShellExecutor: Send + Sync {
    fn execute(&self, request: ShellDispatch, output: PathBuf)
        -> Pin<Box<dyn Future<Output = Result<ShellExit, ToolError>> + Send + '_>>;
}
```

- `Tool::execute` 和 `ToolHandle::invoke` 成功值由 `Vec<ContentPart>` 改为 `ToolOutput`；所有现有工具显式返回 Completed。
- `ToolContext::new` 改为接收 `cwd: PathBuf` 和 `background: Arc<dyn BackgroundShell>`；新增 `background_shell(&self) -> &dyn BackgroundShell`。不保留旧构造兼容层。
- port 实例由 core 为每次调用创建，内部绑定 session、run、person、tool_call_id；工具不能传入别人的身份。`ToolContext` 不暴露 Store 或 Kernel。
- shell 命令执行、进程组终止、输出采集实现在 `mic-tool-shell`。上述 `ShellExecutor` 在 mic-tool 定义，core 通过装配注入的实现启动后台执行，输出路径由 core 提供。丢弃执行 future 必须终止进程组并释放句柄。shell 模块内部前后台共用进程执行逻辑，输出收集方式分别为受限内存尾部与文件，不复制两套命令执行路径。
- `Registry::background_shell(Arc<dyn ShellExecutor>)` 新增一次登记入口，由 shell 模块激活时登记，重复登记装配失败。core 的后台派出 port 依赖该执行器，不依赖具体 shell crate。
- 前台 future 被丢弃仍表示取消；后台提交成功后，所有权转给任务运行时，丢弃原工具 future 不终止已提交任务。

### 3.2 mic-core 与 mic-store

- core 内部任务运行时持有 shell 执行句柄、取消令牌和会话树操作闸门；复用现有 scheduler 唤醒子 session。
- 子 agent 与管理工具由 core 生成，和登记工具合成一次最终工具表；名字冲突装配失败。模型请求与 system prompt 共用该表。
- store 新增 job 创建/查询、子树查询、原子 shell 收尾、原子子 run 收尾与回注、归档树收尾事务。按本文字段定义导出 `JobId`、`JobView`、`JobPage`、`JobOutput`、`StopJobOutcome`；查询均由 core 先检查归属。
- `finish_run` 对 Task session 的扩展：本次 run 终态、本次回注消息与 report 关联同事务提交。run 没有正文也必须产生明确结果。发布事件与 Wake 在提交后进行。
- `Kernel` 新增 `jobs(session, include_old, after, limit)`、`job_output(session, job, offset, limit)`、`stop_job(session, job)`，分别返回 `JobPage`、`JobOutput`、`StopJobOutcome`。session 参数是管理者，权限范围是它自己与后代。
- `JobView` 含 ID、种类、归属 session、子 session ID（如有）、描述、展示状态、最近 run 状态、创建/开始/结束时间、输出字节数与受限尾部。`JobPage` 为 jobs + next 游标。
- 现有 session 读取/历史/trace 支持 Task；Task 不进入 Root 侧栏。Web 不允许通过普通发送接口直接给 Task 发消息，追问走本次入口，避免绕过归属检查。

## 四、模型工具与默认值（草案建议）

### 4.1 bash

现有参数加 `background: bool = false`。`timeoutMs` 在边界解析为 `Default / Finite / Unlimited`：缺省为 Default，正整数为 Finite，后台模式允许 0 表示 Unlimited；前台 0 仍报 input 错误。前台现行默认 120 秒、上限 600 秒不变。

后台默认超时建议 10 分钟，显式正整数不沿用前台 600 秒截顶；0 为不限时。派出返回 Dispatched，不宣称服务就绪。dev server 必须由 agent 查看输出或主动探测成功后再回复“已启动”。后台 stdin 关闭，不支持交互式输入。

### 4.2 子 agent 与管理

| 工具 | 参数 | 行为 |
|---|---|---|
| `agent` | 新建：`task, description`；追问：`job_id, task`，两种形式互斥 | 创建/追加子 session 输入，返回 Dispatched，追问返回相同 job ID |
| `jobs` | `include_old=false, after?, limit=20` | 列自己和后代任务，limit 上限 100；活跃任务排前，其余按最近活动与 ID 倒序，游标携带排序位置 |
| `job_output` | `job_id, offset?, limit=16384` | shell 默认尾部；给 offset 则按字节读取；返回 next_offset、eof；agent 返回最近执行结果与子 session 链接，中间记录用现有历史接口 |
| `job_stop` | `job_id` | shell 终止；agent 停止当前 run 及名下后台任务，session 保留，可再追问 |

子 session 的工具表为父工具范围与 core 子任务工具白名单的交集，去掉 agent 工具；不扩大父权限。当前白名单包含既有文件、shell、web fetch 和本次管理工具。管理操作范围始终检查归属，不能凭 ID 操作兄弟任务。

新建不复制父历史或快照：模型选择解析成父当前所用模型，工作目录/推理设置继承，任务说明作为首条输入。保留框架 instructions 与当前用户 general_prompt，去掉 persona 与 Channel 特有提示；父 agent 须在 task 中交代必要上下文和本次用户约束。后续压缩、trace、上下文构成走同一路径。

建议子 run 的 max_turns 为 `min(当前全局 max_turns, 12)`，整次 run 墙钟上限 30 分钟；超时走取消收尾，记录原因并回注。交付上限 32 KiB，超出返回明确截断标记与子 session/run 引用，可继续读取原文。

默认隐藏超过 14 天无活动的休眠子 session；include_old 可列全部。shell 已结束记录也按该窗口默认过滤，活跃任务永不因年龄隐藏。不删除历史或输出。上述常量放各所属模块 `limits.rs`，不新增启动配置或设置项。

本次不新增全局并发或累计费用预算；max_turns 仅是单次 run 限制，不宣称覆盖整个 session 的累计成本。

## 五、提交、完成与回注

### 5.1 派出交接

1. 工具参数已解析、归属已确定后，提交请求交给运行时。运行时与停止/归档操作共用会话树闸门。
2. 在数据库登记 job（agent 同事务创建子 session 与首条输入）。登记是提交点；提交前取消不创建任务。
3. 提交后由运行时独立持有执行，不依赖工具 future。shell 由运行时启动，agent 提交后唤醒。
4. core 收尾工具批时对照已提交记录：已提交调用一定落 Dispatched；未提交且被停止的调用才补 Cancelled。停止开始后，尚未提交的调用不允许越过闸门登记。相关落盘不接受取消。

数据库和 OS spawn 无法组成事务：pending/running 遗留在重启后均标中断，不重试命令。若进程已创建但回执尚未写，恢复按 job 事实补 Dispatched，不再补“未知前台调用”与之冲突。原始调用关联以 run + tool_call_id 定位，避免跨会话碰撞。

### 5.2 一次执行对应一次结果

- shell 退出、超时、取消的竞争由条件终态更新决定唯一胜者；胜者同事务写 Completion 与 report，后续事件不再写第二份。
- agent 每个 run 收尾写一次 Completion；成功取本次 run 最后一条无工具调用的完整回复，不能从整个 session 取“最后回复”。空产出、模型失败、轮次上限、停止或中断明确说明状态；不把上一轮回复冒充本轮结果。
- 追问与 shell Completion 同时到达，复用 claim_next/absorb；可能被同一 run 吸收，只回注该 run 的一次结果，不承诺每条输入各一条回复。
- 回注仍为 user 视图，由既有边界顺延处理。父空闲则 Wake；父运行则吸收，结束边界由 Dirty 接住。
- agent 的回注对象始终是其父 session；子 shell 回注其 owner 子 session，再由子 run 交付父 session。
- 杀任务的管理者 session 已从工具返回获知结果时，不再给它写重复 Completion；如果任务 owner 是另一个仍可执行的子 session，仍告知 owner。被整树停止覆盖的会话只留记录，不唤醒。

### 5.3 输出

后台 shell stdout/stderr 合并到一个文件，按读取到的顺序写入，不承诺精确重建两路的真实时间顺序。文件在数据目录的 jobs/<id>/output，进程运行时持续可读。短输出直接回注；长输出回注尾部、总字节数和引用。

建议每个 shell 输出文件上限 64 MiB，达到上限显式终止 job，并回注“输出达到上限”；不静默丢弃后继续运行。读取失败或落盘失败不能当正常 EOF。单次读取上限 64 KiB；偏移为字节，文本边界按 UTF-8 修整，响应返回实际消费的 next_offset。

文件是原始输出的唯一副本；数据库不存另一份全量。job 完成前先刷出已采集输出，再提交终态和回注。

## 六、停止与归档

### 6.1 普通停止与任务终止

普通停止仅取消绑定的当前 run，异步任务独立存活。任务列表终止 agent 时：先关闭该子树本次清理期间的派出/认领入口，取消正在执行的 run，清理全部后台 shell，等待收尾，再释放闸门。睡眠 session 无须“杀死”，后续显式追问照常接受。

清理覆盖已经入队但尚未认领的输入：标 held，不在停止后自动跑起来。清理期间到达的完成事实照常记录，但 held 且不 Wake，避免刚停完又被回注唤醒。终止完成后新追问是新工作，正常执行。

### 6.2 归档

- 前端先展示将停止的执行/后台进程数量。归档操作本身包含停止授权，不增加第二套确认协议；无论预览后数量是否变化，实际覆盖提交时整棵树。
- Kernel 先取得 Root 树闸门，阻止新输入触发执行、认领与派出；再取消主/子 run、标题任务和全部 shell，等待收尾。终止原因记为归档，全部结果只记事实，不唤醒树内任何 session。
- 将待认领输入 held，最终写归档标记；只有停止收尾完成才返回成功。失败不伪报归档成功；已经完成的停止不回滚。
- 归档后拒绝新消息、追问、压缩等执行入口；历史/trace/输出仍可读。相比旧契约“归档只隐藏，已知 URL 可继续发送”，这是有意修改。
- 需要在 `Session` / 会话详情暴露 archived_at；Gateway 对写入返回 409，Web 显示已归档并禁用输入。未来恢复功能另定，不在本次新增。

shell 正常关闭/超时/取消杀整个进程组并等待回收；不支持命令自行 daemonize/脱离进程组。正常 micnext 退出等待运行时清理。异常崩溃不承诺 Drop 已运行，也不据持久化 PID 盲杀可能复用的进程；重启记录“执行中断，副作用及外部进程状态未知”，不宣称 OS 进程已不存在。

## 七、重启收尾

沿用启动待命，不新增恢复唤醒分支：

1. 独占数据目录，在 worker 启动前恢复遗留 run 和 shell job。
2. 对遗留子 run、shell job 写中断终态、去重 report 和 Completion；pending shell 不重放。已回注事实不重复写。
3. 启动收尾中产生或重启前尚未认领的 Completion，与其他未处理输入一起 held；不发 Wake。
4. 中断事实使用现有 Notification 展示给用户；子任务中断时，在所属 Root 追加一条聚合通知，列出相关 job ID，避免用户必须进入休眠子 session 才看得到。该通知不触发模型。
5. 用户下一次正常发消息时，模型可读到 held 历史及中断事实；继续追问已有子 session 走同一路径。

重启途中再次退出，靠终态和 report 唯一约束继续收尾；Root 聚合通知与本次被收尾事实同事务写，避免重复通知。睡眠子 session 不标中断，因为没有正在执行的 run。

## 八、Web / HTTP 与 trace

新增接口均在 `/api` 下，已有认证沿用。session 参数必须存在且具有所请求 job 的管理范围；越界 403、不存在 404、非法参数 400、归档/清理中冲突 409。

| 接口 | 返回与含义 |
|---|---|
| `GET /sessions/{id}/jobs?include_old=false&after=...&limit=20` | 200 JobPage；和模型 jobs 共用查询口径 |
| `GET /sessions/{id}/jobs/{job}/output?offset=...&limit=16384` | 200 JobOutput；offset 缺省取尾部 |
| `POST /sessions/{id}/jobs/{job}/stop` | 200 `stopped / already_idle / already_finished`，完成收尾后返回；重复操作无重复回注 |
| `POST /sessions/{id}/jobs/{job}/messages` | 202；body `{task: string}`，仅 agent，归属检查后追加追问 |
| `POST /sessions/{id}/archive` | 保留路径、204；改为完成整树停止后成功 |
| `GET /trace/sessions/{id}/usage?include_children=false` | 200 会话用量与费用汇总，见下文 |

右侧列表显示类型、描述、执行/休眠或 shell 终态、耗时和尾部，agent 可进入子 session 的历史及 trace。Task 页面用专用追问入口，停止键仍针对当前 run；归档只允许 Root。

新增 `KernelEventKind::JobsChanged`，session_id 是所属 Root；提交 job 变化、子 run 开始/结束后发布，列表收到后重查。沿用 SSE 落后重连/ready 重查机制，不单独维护可靠事件队列；输出运行中每秒最多通知一次，有列表的页面才拉取尾部。

trace 开关默认关闭，只控制新增 usage 汇总查询，不改变 run 列表、聊天 run-stats 或单个调用费用：

- Store / Kernel 新增 `session_usage(session: SessionId, include_children: bool) -> SessionUsage`。
- 返回 `input_tokens, output_tokens, cache_read_tokens`、各项 complete 标志与 cost；缺失用量/单价沿用现有统计口径。
- 开启时递归选择 session 与所有后代，各调用行只算一次，包括已归档、默认隐藏的子 session；shell 不产生模型费用。
- 用量/费用算法与已有 trace 共用，不在前端再算；当前价格口径不变，不建设历史价格快照。
- 上下文占用和耗时不按子树相加。trace 从任务入口可直接查看子 session，Root trace 列表保持现有默认范围。

## 九、牵连契约与调用方

批准后实施时，同步修订以下现行文档；草案阶段仅以本文列出差异：

| 文档/调用方 | 修订 |
|---|---|
| mic-message、provider-port | 类型不改；Completion 可对应同一 job 的多次子 run 交付，消息头说明更新 |
| mic-store | job/report schema、唯一约束、子树与归属查询、run 原子回注、归档读写、Session archived_at |
| mic-tool、tools-basic | ToolOutput、后台 shell ports 与注册、所有工具/测试调用方迁移；bash 参数与输出上限 |
| mic-core-module、run-execution | 装配与运行时、派出提交点、写者表、事件顺序、子 session 设置与启动待命 |
| run-stop、session-archive | 当前 run 停止与整树停止区分；归档写入拒绝、排队输入 held、标题取消 |
| system-prompt-layout、context-breakdown | 子 session 无 persona、任务为输入、工具表一处组装；trace 与实际请求一致 |
| gateway、web-ui、run-trace、chat-run-stats | 新接口、Task 浏览/追问、列表事件、trace usage 开关；聊天统计口径不变 |
| bin/micnext | 装配注入执行器；`-p` 退出前清理后台 shell，不为后台结果延长单次调试寿命；终态由下次启动收尾记录 |
| product-roadmap、cross-check、B1 | 批准后以本文替代旧 run_in_background 方向；提醒正则/定时任务仍延期 |

schema 随整体数据库迁移，不给 job payload、prompt 或配置独立版本号。logs 仍只记框架运行与错误，任务状态、输出引用、回注和 trace 使用业务库事实。

## 十、验收与关闭

本地自动验收优先用脚本 Provider、临时数据目录和受控 shell 做模块联动：

1. 前台 bash 行为不变；后台成功/非零退出/启动失败/超时/无限时/输出超限可观测，输出可分页，进程组能清理。
2. 派出提交前后分别触发停止；没有活任务缺失身份/回执，没有已提交任务同时收到 Cancelled。提交后崩溃不重复启动。
3. 子 session 首次执行、休眠、追问、失败无正文、停止半截回复、轮次上限、墙钟超时；只回注本次 run 一次，不重复旧交付。
4. 子 shell 唤醒子 session，再回注父 session；父繁忙/空闲/结束边界均不丢；追问与回注并发沿用 absorb。
5. 完成与终止并发、重复请求只有一次完成事实；权限不能跨兄弟 session，子任务工具范围不扩大。
6. 归档期间持续尝试派出/追问/提交输入，收尾后无执行或排队输入自动复活；重复归档成功，已归档写接口 409，历史可读。
7. 在登记、启动、完成提交和恢复期间退出；重启不补跑、不自动唤醒，通知可见且不重复；休眠 session 不误标中断。
8. 旧子 session 默认隐藏、活跃任务始终可见，include_old 与按 ID 追问有效；trace 开关只改变用量/费用汇总，不重复计算。
9. `cargo fmt`、`cargo clippy -- -D warnings`；前端改动在 `web/` 跑 `bun run check`。不为纯字段映射堆单元测试。

Web 右侧列表、进入子 session、归档提示、trace 开关的观感以及真实长驻 dev server 由 human 验收。通过后标 CLOSED，todo 条目移入 todo-archive；未经批准不开始实现。
