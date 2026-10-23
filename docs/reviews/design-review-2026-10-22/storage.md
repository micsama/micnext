# 存储、消息与媒体设计审查

后续实施进度（2026-10-23）见 [README](README.md#实施进度2026-10-23)。本文保留原始审查证据与当时的行号，不代表这些问题仍未处理。

日期：2026-10-22。按本次更新后的 `CLAUDE.md` 审查；仅记录证据与候选，没有修改业务代码。

## 覆盖范围与结论

静态梳理 `mic-message`、`mic-store`、`mic-media`，重点逐文件核对消息/调用/上下文/任务/调度/推送 schema、写入事务、读取投影、设置备份和图片边界；对疑点追踪到 `mic-core` 调用方、Gateway 和 Web 消费者。凭据部分核对了存储与备份调用边界；只读诊断作静态阅读，没有开展密码学或 SQLite 安全专项审计。

底层整体符合“用少量事实自然组合行为”的方向，暂未发现需要更换存储架构的依据：

- Reply 的模型和用量只落调用行，消息读取时 JOIN 还原，没有再写一份事实。
- FireOutcome 由触发记录、消息认领、run 终态和 push 关联推导，没有另存一套调度执行状态。
- 推送投递复用目标 Notification 的送达/失败事实。
- 用户输入、图片、hold 和伴随通知同事务；任务终态与 Completion 同事务。
- `mic-media` 只负责 inspect/normalize，接收端决定拒收或规整，图片字节在一个边界通过完整解码后原样保存。

主要问题出在同一调用事实的多种统计投影已经分叉；这适合局部整理，不需要统计事实表或通用事件框架。

## 已确认缺陷

### S1 · P2：失败调用已经产生的已知费用被 run 汇总丢弃，缺用量又被表示为完整

**触发场景**：服务商已上报 usage，随后流失败；或者一个 run 内失败尝试包含未知 usage。此情形不是仅由 Store API 人工拼出的非法状态：`mic-core/src/provider.rs:218` 明确允许失败流保留用量，`mic-provider-openai/src/stream.rs:125` 的 `fail_with` 会先交出 Usage 再交出错误，`mic-core/src/run.rs:514` 将失败结果和 Facts 一并落盘。

**证据**：

- `mic-store/src/trace/mod.rs:346` 按 usage 生成单次调用费用，不以成功为前提。
- 同文件 `:419`—`:425` 的 `run_stats` 建立 `settled`，排除 error 非空和 Stopped 的调用；`:451` 只累计该集合费用。
- 同文件 `:161` 的 session_usage 与 `:951` 的 CallStat::add 对所有调用累计已知用量、费用和未知次数。
- Gateway 的 `mic-gateway/src/trace.rs:46` 原样返回含这些 stats 的 TraceRunPage。

**公开 API 复现**：在内存库创建 session、claim run，依次 `record_model_call` 两次失败（第一次有 input/output 各 1,000,000、cache_read=0，输入价 $1、输出价 $2；第二次 usage=None），然后 finish_run(ProviderFailed)，调用 trace_runs 和 session_usage：

```text
calls[].cost = [3.0, null]
run.stats.cost = 0.0
run.stats.input_tokens = 1000000
run.stats.output_tokens = 1000000
run.stats.usage_complete = true
session.cost = 3.0
session.unknown_usage = 1
session.usage_complete = false
```

**实际影响**：同一个已记录 run，HTTP 执行合计可表示为 $0 且用量完整，单次调用和会话合计却分别有 $3 及未知用量。Web `TraceTab.svelte:527` 使用 stats.usage_complete 决定是否加 `≥`，所以失败未知用量没有被标注为下界。当前 Web 费用主面板从 calls 重新聚合，未发现它直接使用错误的 stats.cost；该费用错误首先是公开返回数据和维护口径问题，不能扩大描述为所有页面均显示 $0。

**整理方向**：让执行成败和计费/用量是否已知独立。明确“一次调用未知是否令汇总未知，或汇总只给已知部分并另报未知次数”，然后在现有统计模块内统一，不增持久化字段。现有测试 `trace/tests.rs:203` 明确沿用“失败不算缺用量”的旧假设，因此改此口径涉及既有公开字段语义，需 B2 确认，不能在审查阶段直接改。

**契约分歧**：`archive/chat-run-stats.md:27` 曾明确假设“失败与被停止的调用无用量，按零计”；`archive/run-trace.md:24` 又明确 usage 与 error 独立，失败也可有值，并要求失败调用保留 usage。现行实现已经累计失败调用的已知 token，却仍沿用旧假设排除费用和完整性影响。本项确认的是事实与投影、现行契约之间的矛盾；“未知失败调用是否影响聊天完整性”的最终口径须在修订 B2 中一并定清。

### S2 · P2：会话含未知用量调用时，缓存总量仍被标完整

**触发场景**：至少一次调用有 usage 且 cache_read_tokens 已上报，同时另一次调用 usage=None；网络失败、停止中的调用都可能形成该组合。

**证据与调用链**：

- `mic-store/src/trace/mod.rs:177` 只检查 `known` 子集的 cache_read_tokens，没有考虑 unknown_usage。
- `mic-core/src/kernel.rs:835` → `mic-gateway/src/trace.rs:30` 原样返回 session usage。
- `web/src/components/developer/TraceUsage.svelte:25` 用该字段构成 `cacheKnown`；`:15` 用它决定会话树提示中的“缓存读取”显示数值还是未知。
- `web/src/components/developer/trace-presentation.ts:116` 对单步/轮次也复制了相同判定，因此不是只修 Rust 一处就能保持展示一致。

**实证**：S1 同一个公开 API 复现中，session 返回 `unknown_usage=1, usage_complete=false, cache_complete=true`。这是实际可复现的矛盾；已知缓存和可以显示，但不应标为完整总计。

**实际影响**：用户查看含未知调用的会话/会话树时，缓存读取数值被表现为确定值；单步、轮次和全会话可能同时传播这一口径。不会损坏业务数据。

**整理方向**：由统一的聚合规则明确“已知部分”和“完整”，再同步 Rust 与 Web 的消费。字段形状可以不变；改变公开语义仍应纳入 S1 的 B2。

## 结构摩擦与局部重构候选

### R1：围绕调用事实收拢统计规则，删除已无必要的汇总分叉

**当前代价**：run_stats、session_usage、CallStat::add 和 Web usageOf 分别定义失败是否排除、未知是否计数、费用是否为 null、缓存何时完整。S1、S2 已证明这不是抽象的代码风格问题。TraceRun 同时返回 calls 与一部分可由 calls 重算的 stats，而 Web 又以自己的 usageOf 算费用。

**最小合理方向**：先确定当前真实消费者需要哪些汇总，再在 `trace` 模块内部用一个小型调用统计累加器表达“已知和 + 未知数”；耗时和步骤归组仍在其各自职责中。对于完整返回 calls 的页面，确认保留哪一处统计权威，允许展示层做格式化，但不再各自定义未知/失败语义。无需再引入 crate、trait 或持久化统计表。

**可删内容**：`settled` 对计费/完整性的特殊过滤，重复定义缓存完整性的分支；若经消费者核对确实无人使用，可删除 RunStats 中重复的计费汇总字段，而不是一边继续返回一边让前端另算。

**契约影响**：仅抽内部累加器不改公开契约；统一统计语义、删除/调整 HTTP 字段需 B2。应一并修正 S1、S2 的验收样例，避免只逐行补判定。

### R2：任务关系字段存在已无运行消费者的保留项，可在下一次任务整理时收缩

**当前证据**：全仓库引用检索显示，`core_jobs.report_message_id` 在 `schema.rs:243` 定义，在 `jobs.rs:347` 写入，运行路径没有读取；agent 的最新交付实际读取 `core_runs.report_message_id`（`jobs.rs:423`）。工具任务的一次性收尾由 `WHERE state='running'` 的条件更新决定，并不靠这个列去重。另一个 `core_sessions.parent_tool_call_id` 经 `types.rs:236`/row.rs 投影保留，登记 agent 时又把同一值写到 `core_jobs.origin_tool_call_id`；当前恢复查询实际使用后者与 origin_run_id 的唯一关系（jobs.rs:389）。

**当前代价**：字段与 API 携带了额外关系，读代码时必须辨认哪个关联真正用于恢复与交付。暂未证明产生用户可见错误，不列为缺陷。

**最小合理方向**：下一次整理任务关系时，逐项明确真实读者。保留 agent 的逐 run 交付关联与原始 tool-call 来源关联；考虑删除无人读取的工具任务 report_message_id 及只用于原样投影的重复字段。历史蓝图确实明确保留了这些字段，不能以本次规则为由未经确认删去；诊断查询若被 human 实际使用，也是有效消费者。

**可删内容与契约影响**：可能删除一个只写列、相应 UPDATE，以及 SessionKind::Task 的冗余携带；涉及 schema 迁移和公开类型变更，须 B2。没有为这一清理建立通用关系层的理由。

**现有保护与删除边界**：tool job 的原子终态更新已经保证只回注一次，`report_message_id` 当前没有参与额外保护；删该列不应改变去重策略，也不应增加新的状态位替代。Task 的创建调用关联可从其唯一 agent job 读到，若删重复字段，任务创建者和恢复事实仍保留在原有 job 关系中。这里是减去并行携带的事实，不是删用户消息或历史结果。

### 过度防御核对

本轮没有发现值得单独立项的大型重复校验机制。以下保护有当前明确职责，不建议机械地按“内部互信”删除：

- jobs 的 JobState::parse 使用 schema CHECK 后的穷尽解释；没有层层再验证状态转换。
- row.rs 对持久化 JSON/枚举值解码出错返回错误，属于磁盘边界；它不是同一不变量在内部重复验证。
- 图片 GET 按 session_id 限定归属，归档沿祖先检查，属于会随调用目标或时间变化的条件。
- create/finish job 的事务和条件更新保护取消/结束竞争；不能为了减少代码删掉后再让调用方靠约定串行。

当前最有收益的减复杂度候选是 R1 的统计分叉和 R2 的无读者字段；没有证据需要增加存储抽象层、事件溯源、泛型仓储或通用状态机。

## 已排除和待验证

- **已排除：Trigger 无 person 造成后台任务失败。** `jobs::run_person` 确实要求首条输入有 person；但 scheduled-tasks 蓝图 `:142`—`:143` 明确禁止任务会话后台执行/派子 agent，`mic-core/src/run.rs:73`、`:649` 与 `jobs.rs:755` 实现了工具可用性和后台派发限制。未找到当前可达的违规调用链，不报缺陷。
- **已排除：删除标题模型后无法导出备份。** `archive/settings-backup.md:29` 明确要求先重选/清空已删除标题模型，否则导出报错；实现符合契约。
- **已排除：上下文按 created_at 而消息按 id 导致排序不同。** 已核对 `archive/chat-timeline-order.md`：显示位置是 `(created_at,id)`，id 保留作落盘集合/游标，压缩读取也改为位置比较。不能用旧注释推断这是缺陷。
- **待验证：调度任务硬删后的 id 复用。** schedule 表用 `INTEGER PRIMARY KEY`，DELETE 后最高 id 可复用；旧任务会话保留 trigger_ref。但当前调用方大多通过 session_id 关联任务，未找到 trigger_ref 被误解析为新任务的运行路径，因此不列已确认缺陷。若要制定“所有外部 id 永不复用”契约，先说明真实消费者需求。
- 没有运行真实媒体/微信/服务商网络场景，也未检验数据库损坏后的行为；这不构成本次修改建议。

## 验证

- `cargo test -p mic-store --offline`：19 项通过。
- 公开 API 复现程序：`/private/tmp/micnext-storage-review`。执行命令：`cargo run --manifest-path /private/tmp/micnext-storage-review/Cargo.toml --target-dir /Users/xingji/workspace/tools/micnext/target --offline --quiet`。仅使用内存数据库，没有访问实际用户数据库或网络。
- 仓库仅新增本报告；复现代码在临时目录，没有给业务代码/测试加补丁，没有提交。

## 附录：额外的零消费者与复杂度核对

此附录重新检索当前仓库全部 Rust/TypeScript/Svelte 定义、构造与调用；10-19 审查仅作搜索线索。下面均为清理候选，不冒充运行缺陷。扫描范围是本仓库，不能据此断言不存在仓库外使用这些公开 Rust API 的人。

### E1：三个 Store 公开方法已经没有仓库内调用者

| 方法与定义 | 当前真实路径 | 删除收益与影响 |
|---|---|---|
| `Store::bind_identity`，`mic-store/src/people.rs:20` | 全仓库只有定义。当前入站走 `mic-core/src/kernel.rs:324` → `Store::resolve_identity`；微信 `account.rs:182` 使用该路径，默认 person 是当前 owner。启动 `assembly.rs:434` 只 ensure owner，没有按配置遍历身份改绑。 | 删除无使用方的显式改绑入口及其事务，减少“启动配置还负责身份改绑”的旧契约。保留 resolve_identity、身份表和现有 person 关联，删除方法不应改写历史身份或删除用户数据。 |
| `Store::set_pwd`，`mic-store/src/sessions.rs:90` | 全仓库只有定义。工作目录在创建会话时设定；`mic-gateway/src/api.rs:268` 为新会话取 default_workdir，`-p` 取 cwd，子任务继承已有会话。 | 删除无调用写入口，避免维护一个没有产品入口的“运行中修改目录”能力；保留 pwd 字段及创建/继承读取。不能把它误当成 default_workdir 配置的实现。 |
| `Store::sessions_with_unclaimed_input`，`mic-store/src/scheduling.rs:191` | 全仓库只有定义。启动 `assembly.rs:432`—`:434` 恢复并将遗留输入 held；正常调度 `scheduler.rs:31` 接收 wake，`:87` 循环 claim_next，不扫全库补跑。 | 删除死查询，消除一个容易被重新接到启动流程、改变既定“不补跑”语义的入口；不需要另加一个扫描器接替它。 |

**契约影响**：三者均为 `pub`，删除需小型 B2，更新过时的 Store 蓝图说明。没有 schema 或消息格式变化，也没有理由为它们增加新消费者。当前未见测试调用，不需保留兼容转发。收益虽小，但边界明确，可与下一次 Store 公共面整理一起完成。

### E2：ContentPart::File/FileRef 没有当前生产者，却让多层承担处理分支

**当前证据**：

- `mic-message/src/lib.rs:27` 的 FileRef 只有类型定义；全仓库没有 `FileRef { ... }` 构造，也没有 `ContentPart::File(...)` 生产表达式。现有命中均为类型定义、匹配处理、文档。
- `mic-core/src/input.rs:8` 的 IncomingPart 和 `mic-store/src/types.rs:507` 的 NewInputPart 都只有 Text/Image，正常入站不能构造 File。
- 微信 `service.rs:749`—`:759` 的附件链路先保存文件，再 `IncomingPart::Text(placeholder)`；`media.rs:88` 把绝对路径写入文字。内置工具输出也均为 Text。
- 保留的读者分散在 `mic-store/src/trace/mod.rs:485`、`mic-core/src/request.rs:237`、`context_usage.rs:21`、CLI `main.rs:278`、三个 Provider 的 unsupported_file 分支，以及 Web ContentPart decoder、MessageItem、TraceContent、outcome 文本转换。

**当前代价**：一个没有生产路径、Provider 又只会拒绝的变体，仍要求每次扩展内容片段时维护所有协议和 UI 的穷尽分支。它不代表已有可用附件系统。FileRef 的 `path` 注释约定 `<data_dir>/files`，也并非微信当前实际附件路径模型。

**最小方向和收益**：若确认没有需要保留的历史 File 消息，删除这个变体、类型及其多层读取/拒绝/展示分支；当前微信文件收取、落盘和文字路径继续工作。无需把微信文本附件迁回 File 来“用起来”，也无需建立附件生命周期框架。

**不确定性与契约影响**：Serde 可以从旧数据库 payload 解码 File；没有读取用户实际数据库，因此“没有当前生产者”不等于“没有历史数据”。删除涉及公开 MessageBody 内嵌类型、HTTP 消息形状和数据库解码，需 B2 先确认现有数据；若有历史值，应制定一次性迁移而非默默使旧消息不可读。这里只列候选，不能直接删变体，更不能清掉用户文件。

### E3：Activation::WhenConfigured 是当前没有使用方的装配维度

**当前证据**：

- `mic-core/src/module.rs:15`—`:29` 定义 Module::activation 缺省 WhenConfigured 与 Activation 两变体。
- 唯一装配消费在 `mic-core/src/assembly.rs:148`：缺配置时只安装 Always。
- 当前六个 Module 实现全部覆盖为 Always：Gateway `lib.rs:70`、OpenAI `lib.rs:26`、Shell `lib.rs:21`、FS `lib.rs:27`、WebFetch `lib.rs:21`、微信 `service.rs:40`。没有测试实现依赖缺省；二进制 `main.rs:107`—`:120` 仅装配这些模块，微信由编译 feature 决定是否加入。

**最小方向和收益**：直接规定已传入 Assembly 的模块均安装、缺配置收到空表；保留现有“配置段必须是表”和未知模块名校验。可删除 Activation 公开枚举、trait 方法、六个一律返回 Always 的实现以及条件分支。各模块内部仍负责解释其启动配置。此举移除一个目前只有单一实际取值的变化轴，减少新增模块必须记住覆盖缺省值的约定。

**现有保护与删除边界**：WhenConfigured 原本用于“没配置就不装配”，当前没有模块选用，因此对本仓库运行行为没有保护作用。它也不是微信启用开关；不能连带删除微信 feature 或模块自身配置校验。

**契约影响**：公开 Module trait/Activation/Assembly 行为发生收缩，需 B2 并更新调用方导入和蓝图；没有 DB/HTTP 变化。若 human 明确仍支持仓库外自定义 Module，须先确认这项扩展契约，不能仅按仓库零消费者删掉。
