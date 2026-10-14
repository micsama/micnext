# B2：轻量 trace 与存储整理

**状态：实现与本地验收完成，待 human 验收页面观感及真实环境（2026-10-14）。**

依据：[trace B1](../brainstorm/run-trace.md)、[存储 B1](../brainstorm/sqlite-organization.md)。原型 `tmp/trace-preview/index.html`（模拟数据，不进 git）。B1 已拍板的取舍不再复述，本文只写契约、采集点、重建规则与验收。

## 一、用户看到什么

- **trace 页**：左栏 Root 会话（按渠道分组，带失败/报错/中断信号）；中栏按会话连续显示已结束的 run，规则见 B1「展示 · 本轮验收」；右栏详情（概览 / 预览 / 输入）。执行中的 run 不显示。trace 聚焦 agent 执行，不显示推送行，Channel 投递细节不在本轮。
- **看板页**：按天的 run 数与成功率、调用数与失败率、token、费用、平均耗时，按模型、工具分组。
- **模型设置**：每个模型可填单价（输入、缓存读、缓存写、输出，美元 / 百万 token），不填则费用显示未知。
- **升级**：旧业务库不能打开，启动报错，列出要删除的业务库与日志库路径；删除后重建为空库，微信需重新登录，历史对话清空。
- 不在本轮：推送行、子 agent / 定时任务区、运行中 run、长期清理、外部导出。

## 二、Schema 基线

core、wechat 各一份基线（core v10、wechat v3），删除历史迁移链与旧 payload 版本检查；以后的增量仍走 `Migration`（说明从“从 1 连续递增”改为“从基线起连续递增”）。按模块判断：没有该模块的版本记录 → 直接建基线（空库、首次启用微信）；有记录且低于基线 → `StoreError::SchemaRebuild { module, version, baseline }`；高于程序版本 → 沿用现有拒绝。Store 不知道日志库：bin 按数据目录生成提示“数据库格式已更新，请删除 {业务库} 与 {日志库} 后重启；微信需重新登录”。不做自动删除。

| 表 | 相对现状的变化 |
|---|---|
| `core_runs` | 只留 `id, session_id, state, created_at, finished_at`；删 persona_*、general_prompt、max_turns、model_id、model_name、compaction_*、reasoning_effort |
| `core_model_calls` | `purpose` 非空；新增 `model_id`（可空外键）、`reasoning_effort`、`first_output_at`、`stop`、`context_skip`（整数，默认 0）；usage 列与 `error` 独立（失败也可有值）；索引 `(run_id)`、`(session_id)` |
| `core_model_call_inputs` | 新表 `(call_id, message_id, role)`，主键三者；role ∈ `excluded`、`tail`、`instruction`，语义见第五节 |
| `core_messages` | Boundary 行写 `model_call_id`（与其压缩调用关联） |
| `core_models` | 与 `context_window` 同级新增 `price_input, price_cache_read, price_cache_write, price_output`（REAL，可空，≥0） |
| `core_sessions` | 删 `delivery_version`；`DeliveryTarget.version` 字段、微信构造处与版本检查一并删除 |
| wechat | 删 `plan_version`、`snapshot_version`；建表从 `account.rs` 移到模块 schema 文件 |

消息 payload：`ToolResult` 增加 `started_at: Option<i64>`（恢复补的 `Cancelled` 为 `None`）；`Compaction` 删 `extra`。

表目录：`schema.rs` 旁一份静态数组（表名、分组、一句说明），`GET /developer/sql/schema` 按它返回分组，HTTP 每张表增加 `group`、`description` 字段，SQL 页按 `group` 分组折叠；删掉 `lib.rs` 表清单与 `mic-store.md` 表概览，改指向它。

logs 库：`log` 表增 `session_id, run_id`（写入时从字段 `session`/`run` 取），索引 `(run_id)`；`SCHEMA_VERSION` 2，沿用现有的版本不符即重建。

## 三、公开契约变更

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| `mic-message` | `MessageBody::ToolResult.started_at`；删 `Compaction.extra` | 工具执行、恢复、Web 消息 DTO |
| `mic-store` 用量 | `Usage` 各项 `Option` 语义：`Some(0)` = Provider 确认不适用或为零，`None` = 上游未报；Provider 按协议填，不 `unwrap_or(0)`（Chat 无缓存写入概念 → `Some(0)`） | openai 两个实现 |
| `mic-store` 调用记录 | `StopReason` 从 mic-core 移入（core 重导出）；`ModelCallContext { boundary_id, through, skip: u32, excluded, tail, instruction: Vec<MessageId> }`；`NewModelCall` 增 `model_id: Option<ModelId>`、`reasoning_effort`、`first_output_at: Option<i64>`、`stop: Option<StopReason>`、`usage: Option<Usage>`；`ModelCallOutcome::{Replied { blocks }, Failed { error }}` | 对话、压缩、标题三处 |
| `mic-store` 窗口 | 读窗口同时返回 `filtered: Vec<MessageId>`：窗口范围内、`through` 之前因**未认领且非 held** 而排除的输入（held 输入照旧留在窗口，调度语义不变） | `call_model`、`compact` |
| Store 压缩 | `commit_compaction` 把调用 id 写入 Boundary 行 | 压缩模块 |
| Store 用量依据 | `context_window` 的用量只取 `purpose = conversation`，模型取调用行 `model_id` | 压缩触发判断 |
| Store 模型 | 模型条目带 `price: ModelPrice { input, cache_read, cache_write, output: Option<f64> }`，Store 校验 ≥0；Provider 的 `config_json` 不变 | 模型设置 HTTP 与 Web |
| Store trace 读 | 见第六节函数表 | Kernel |
| Store 打开 | `SchemaRebuild` 错误 | bin 启动报错 |
| Provider port | 新增 `ModelEvent::Usage(Usage)`（流结束前任意时刻，后到覆盖先到）与 `ModelEvent::ToolCallDelta`（工具参数增量，只作时点）；删 `ModelResponse.usage` | openai：Chat 在 usage chunk、Responses 在 completed/incomplete/failed 事件发 `Usage`；参数增量处发 `ToolCallDelta` |
| Kernel | trace 读入口逐一转发 Store（第六节）；另加 `call_input(call)` 与 `system_prompt(session)`；现有无参 `system_prompt()` 改名 `base_prompt()` | Gateway trace 接口；设置页 `settings.rs` 切到 `base_prompt()` |
| HTTP | 见第六节；模型条目 GET/PUT 增 `price` | Web |

## 四、采集点

**普通对话**（`call_model`）：读窗口得 `filtered` → 记 `excluded`；组装后 `tail` = 窗口内的待处理输入；流里首个非空正文/推理/工具参数增量记 `first_output_at`；收到 `Finished`/`Err` 即取 `finished_at`，在等待 publisher 之前；`Usage` 事件独立累计，成败都写；每次尝试一行。

**压缩**：`excluded` = `filtered` ∪ 本步待处理输入；`skip` = 预算裁掉的窗口前段条数，等于窗口长度即裁剪为空（只发摘要指令）；`instruction` = 本次附加文字的来源：`/compact` 为该 Command 消息，自动压缩为被排除的待处理输入，无附加文字为空。成功时调用与 Boundary 同事务并写关联；解析失败调用仍记成功，失败由现有通知表达；上游失败同对话。

**标题**：purpose `title`，记模型与档位，不记 context。

**工具**：`run_tools` 每个任务自己：取 `started_at` → 查找/调用 → 取返回时刻 → 取 publisher → 落盘（`created_at` = 返回时刻）→ 发布。持 publisher 落盘保证消息 id 顺序与发布顺序一致，SSE 按 id 去重不漏。收取循环只处理 panic 与写库错误。

**恢复**：补 `Cancelled` 不带 `started_at`；崩溃中的模型尝试无记录，显示“在最后一条事实后中断”。

**重试**：不加列；同 run 内连续、purpose / boundary / through 相同的调用即一次逻辑调用的多次尝试。

## 五、输入重建

重建只用调用时记下的事实，不靠后来的认领结果推断：

1. 按 `boundary_id` 读截至 `through` 的全部消息（不按认领状态过滤）。
2. 去掉 `excluded`，走共用 `order` 排序；压缩在排序后跳过前 `skip` 条（允许跳空）。
3. 走该用途的同一组装（不重复排序）：对话 `request::build` 后把 `tail` 挪到末尾；压缩追加摘要指令，附加文字为 `instruction` 消息的文字（Command 取其参数，输入取正文，与 `pending_text` 同一函数）。

步骤 3 与请求组装共用同一组消息选择、排序、压缩指令函数，不另写一套。只返回消息部分。B1 里“认领与读窗口同一事务”的前提不再需要，调度不变。

## 六、读取与 HTTP

Store 函数经 Kernel 同名转发。未知值一律 `null`（不用 0）；时间为 Unix 毫秒。

| 接口 | Store / Kernel | 返回 |
|---|---|---|
| `GET /trace/sessions` | `trace_sessions()` | `[{ session, channel, title, last_run_at, failed_runs, interrupted_runs, tool_errors }]`，Root 会话 |
| `GET /trace/sessions/{id}?after=<run_id>&limit=` | `trace_runs(session, after, limit)` | `{ runs, title_calls, next_after }`。run：`{ id, state, created_at, finished_at, calls, messages }`；call：`{ id, purpose, model_id, model, reasoning_effort, started_at, first_output_at, finished_at, stop, error, usage, reply_id, boundary_id }`；message：`{ id, kind, created_at, preview（≤200 字）, tool?: { name, call_id, started_at, outcome, error } }`。按完整 run 旧到新，`next_after` 为 `null` 即到底；`title_calls` 只在首页 |
| `GET /trace/sessions/{id}/system` | `system_prompt(session)` | 当前配置下的系统提示 |
| `GET /trace/messages/{id}` | `message(id)` | 完整消息（现有消息 DTO） |
| `GET /trace/calls/{id}/input` | `call_input(call)` | 重建的输入消息；标题调用返回 404 |
| `GET /trace/runs/{id}/logs` | logs 库 `by_run(run)` | 该 run 的 WARN/ERROR；查询前走现有 `flush` 屏障，保证已发生的日志可见 |
| `GET /trace/peers` | `call_peers()` | `{ models: [{ purpose, model_id, median_ms, samples }], tools: [{ name, median_ms, samples }] }`：全库成功样本，模型按 `(purpose, model_id)`（`model_id` 为空不入样本），工具按工具名 |
| `GET /trace/dashboard?days=` | `dashboard(days)` | 按天、按模型、按工具的聚合；token 已知合计 + `unknown_usage` 次数，费用已知合计 + `unknown_cost` 次数 |

**费用**：调用费用 =（输入 − 缓存读 − 缓存写）× 输入价 + 缓存读 × 缓存读价 + 缓存写 × 缓存写价 + 输出 × 输出价，单价按百万 token。推理 token 已含在输出里，不另计。用量未知、公式里任一 token 项为 `None`、或某项 token > 0 而对应单价未填 → 该调用费用未知；单价填 0 即免费。按当前单价估算。

**一处口径**：重试归组、耗时、费用各一个计算函数，trace 与看板共用；不引入 span 框架或统计事实表。

聚合不读正文和图片；输入、全文、日志在右栏页签打开时才请求。

## 七、Web

会话列表流按时间混排 `run` 与 `side` 两类条目。旁路调用属于 session，不借用任何 run；本轮标题调用显示「旁路 · 标题」标记，提供概览与已有产出预览，不提供 run 日志或会话输入重建。现有 `title_calls` 作为旁路条目的数据来源，未来意图分类扩展用途与读取接口时沿用 `side` 条目结构，不预建用途或存储。

入口修订（2026-10-14，human 明确）：会话顶部工作目录/渠道旁的 Trace 按钮进入 `/s/{id}/trace`，自动选择会话并筛选对应渠道；侧栏独立「运行追踪」进入 `/trace`。追踪占满应用窗口、隐藏聊天侧栏，保留返回会话入口；看板作为该全屏视图的页签，不再放开发者诊断。

新增 trace 路由与组件：会话导航、轮次流、时间条、详情面板、看板；模型设置加单价。复用主题变量、Markdown、Icon、CopyButton、工具结果格式化、`api/client`。`ReplyView`、`ContextBoundaryView` 不原样嵌入。选中态（hover 底色 + 1px 边框 + 1px 光晕）等通用样式进共享主题；时间条布局与 trace 专用 CSS 留在组件内。

## 八、实施顺序与验收

1. `store.rs` 按需机械拆分私有模块，公开契约不变。
2. 新基线、类型、采集。模块功能测试（假 Provider）覆盖：插队、调用后才 held 的输入、同 run 后续吸收的输入、并行工具起止与消息发布顺序、重试成功、重试全失败（只有调用行）、压缩成功关联 Boundary、压缩解析失败、压缩裁剪为空、同 run 先 `/compact` 再自动压缩、恢复补 `Cancelled`、失败调用保留 usage；**每次调用把实际请求消息与 `call_input` 重建结果对比**（比较模型可见内容与顺序，不比临时消息的 id 与时间）；旧库报错、空库建基线、首次启用微信建基线。
3. trace 查询、Kernel 入口与 HTTP；日志按 run 查询。
4. Web trace 页（观感交给 human）。
5. 看板、中位数、单价与费用。

收尾：`cargo fmt`、`cargo clippy -- -D warnings`、`web/` 下 `bun run check`；删除旧迁移测试；更新 `provider-port.md`、`context-compaction.md`（extra）、`mic-store.md`（表概览）。

## 九、实施进度（交接记录，2026-10-14）

实现与本地自动验收完成，未提交。待 human 验收页面观感及真实模型/微信环境，通过后再标 CLOSED 并归档 todo。

**实现**
- 存储私有模块拆分；core v10 / wechat v3 基线；旧库拒绝打开并提示删库重建；微信建表迁入 `mic-channel-wechat/src/schema.rs`。
- 调用事实采集、工具各自起止与有序发布、输入重建、trace/看板查询、Kernel 与 `/trace/*` HTTP、按 run 查 WARN/ERROR 日志。
- 全屏 `/trace` 与 `/s/{id}/trace` 快捷入口；会话顶部 Trace 按钮、侧栏独立总入口，内含看板页签。模型四项单价；SQL 表目录分组折叠与说明。
- 预览与输入复用 Markdown / 工具结果格式化；系统提示注明是当前配置；共享 `selection-active` 选中态。
- 按 human 补充方向：标题属于 session 旁路，按时间混入 run 列表，以「旁路 · 标题」及虚线边框标记；不绑定 run，不提供不适用的输入/日志页签。前端 `Entry` 保留 `side` 类型扩展口，未来意图分类扩展用途和接口时复用。
- 修复详情请求与选中项切换竞态、分页重复追加、看板切换日期的旧请求覆盖；压缩解析失败保留模型调用成功事实；中位数按中间两项平均；看板显示成功率/失败率。
- 更新 provider-port、context-compaction、mic-store；明确 skip 在 order 之后应用。

**本地验收（以功能/模块流程为主，不新增函数级单测）**
- `cargo fmt`、`cargo clippy -- -D warnings`、`git diff --check` 通过。
- `web/` 下 `bun run check`（0 错误/0 警告）、`bun run build` 通过；`cargo build -p micnext` 通过。
- `cargo test -p mic-core -p mic-store -p mic-channel-wechat -p mic-gateway` 通过：core 9、store 12、wechat 1；Gateway 通过下述 HTTP 功能验收。
- `cargo test -p mic-provider-openai` 9 项通过（模拟上游需本机监听端口，在沙箱外执行）。
- core 模块流程覆盖重试成功/全失败、失败 usage/首输出、调用后 held 与同 run 后续吸收、工具间插入输入、并行工具时点/发布顺序、中断补 Cancelled、压缩 Boundary 关联、解析失败、裁空、同 run 显式后自动压缩、分页和旁路归属。新增执行场景逐次对比实际请求消息与 `Kernel::call_input` 重建消息的内容/顺序。
- 完整进程 HTTP 验收：本机模拟 Chat SSE 上游 + 独立新库，7 组通过：对话/价格/标题旁路；输入/预览/系统提示；compact/Boundary/指令重建；分页；失败 run/日志 flush 与过滤；看板/peers/表目录；非法查询参数。验收脚本 `/tmp/micnext_trace_acceptance.py`；结果 `/var/folders/83/sxqmj_fx5csf5v3229nc99y80000gn/T/micnext-trace-accept-w8cb67vk/result.json`。临时服务已停止。

**尚待 human**
- 页面观感与真实环境验收；旧库可丢弃，无兼容迁移工作。
- 未提交、未暂存；工作区原有 human 改动与 `.worktree/` 保留。

### 前端原型对齐（2026-10-14）

依据本地原型重做三栏面板、紧凑角色行、固定指标列、工具色/用户色、旁路虚线、调用与回复合并展示、详情指标/摘要/关联/折叠明细，增加会话搜索、渠道筛选、异常筛选、轮次折叠、详情放大与 Esc 退出；看板补总览指标卡。使用现有主题变量适配明暗。human 要求不使用 computer，未完成浏览器视觉验收，交由 human 检查。Web 类型检查与构建通过后提交推送，B2 保持待人工验收。

### 2026-10-14 页面反馈调整

- 中栏时间条以单次模型调用及其工具批次为尺度；模型行展示模型 / 工具两段，工具行共享本步尺度。工具按前一条 Reply 归属，保留执行时间顺序。
- 会话顶部增加 System 入口，展示当前配置并明确非历史快照。
- 概览采用紧凑指标、重试明细、Markdown 摘要；关联仅展示来源用户、所属调用及工具批次，按执行顺序排列，附轮次信息。
- 模型设置使用紧凑行（模型 ID、上下文、最大输出），计费设置折叠，明确美元 / 百万 token 单位；服务商名称留空时保存预设类型名称。输入值使用正文色，与灰色占位提示区分。
- 以上均复用既有 HTTP 契约。页面观感仍待 human 验收。

### 升序列表与概览调整（用户确认实施）

- 分页契约改为 `after` / `next_after`，Store、Kernel、HTTP、Web 同步切换；首屏最早执行，后续向下追加。不保留旧倒序参数。
- 轮次是当前会话内已结束执行的顺序编号，从 1 开始；数据库 run ID 仍用于请求和日志定位，不用作显示序号。
- 标题旁路按开始时间升序混排。分页未加载到的时间范围不提前展示旁路，避免跨过尚未加载的轮次。
- 概览突出状态、耗时与用量，工具状态及耗时置于摘要前；摘要和关联分区，次要调用参数默认折叠。

- 本轮验收：Core 9 项、Store 12 项既有测试通过；升序混排、旁路分页范围、会话编号模块场景通过；完整 HTTP 验收 7 组通过（含升序分页），结果 `/var/folders/83/sxqmj_fx5csf5v3229nc99y80000gn/T/micnext-trace-accept-9su1eg18/result.json`。`cargo fmt`、Clippy、Web check/build 均通过；未使用电脑控制，页面观感仍待 human 验收。
