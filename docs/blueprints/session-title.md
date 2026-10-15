# B2：会话自动标题与重新生成

**状态：CLOSED（2026-10-14 human 验收通过）。**

依据：[B1](../brainstorm/session-title.md)。本 B2 只负责标题用途配置、生成与展示，不修改模型注册或会话档位契约。

## 一、用户看到什么

- 常驻模式的 Root 会话首次含非空文字的 UserInput 落盘，且没有标题时，后台尝试取标题；不等正文回答。命令、Completion、纯图不触发；纯图后第一条文字可触发。包含 held 的 UserInput 也按同一规则判断。
- 旧会话已有文字，之后的新消息不是首条文字，天然不会自动触发；没有批量补标题。`-p` 不触发自动标题、不等待标题任务。
- 标题旁提供重新生成按钮，生成中禁用；旧会话可手动生成。成功替换标题，失败保留旧标题并显示错误。再次调用不保证措辞一定不同。
- 设置 → 对话偏好增加「标题生成」：模型选择 + 复用的思考滑条。初始为「使用默认模型」且档位为空；选具体模型后可单独选择 none，聊天会话可同时用同模型的 high。
- 默认模型模式只允许档位为空；要固定 none 就选择具体模型。无模型、指定模型已删、档位失效或上游错误都明确失败，不切换供应商/模型兜底。
- 显示规则为 `title ?? preview ?? 会话编号`；标题不是聊天消息，不进入上下文，不改变最近活动时间和侧栏排序。

展示边界（human 已定，2026-10-13）：**当前打开会话的页头、浏览器标题与侧栏对应条目及时更新；未打开会话不做实时全局同步。** 后者在页面刷新、窗口重新获得焦点、切换会话或展开其它渠道时随列表请求更新；已加载的更早页在重新取到该页/打开该会话时更新。保留现有按会话 SSE，不新增全局事件流，也不增加定时轮询。

## 二、公开契约变更

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| `mic-store` 类型 | `TitleModelSelection { model_id: Option<ModelId>, reasoning_effort: Option<ReasoningEffort> }`；`SessionSummary` 增加 `title`、`title_error`（均 `Option<String>`） | core 标题调用、Gateway 摘要、Web 列表使用 |
| Store 配置方法 | `title_model() -> Result<TitleModelSelection, StoreError>`；`set_title_model(selection) -> Result<(), ModelSettingsError>` | 只由 Kernel 读写；空模型不得存具体档位 |
| Store 调用快照 | `title_model_snapshot() -> Result<ClaimedModel, StoreError>` | core 标题 worker 在同一读事务里取得用途选择、模型配置及凭据，复用现有模型快照类型 |
| Store 输入查询 | `first_text_input(session) -> Result<Option<Message>, StoreError>`；`recent_text_inputs(session, limit: NonZeroU32) -> Result<Vec<Message>, StoreError>` | 自动入口比较新 message id 与首条文字 id；手动取有界输入 |
| Store 标题写入 | `save_title_result(session, result: Result<String, String>, call: Option<NewModelCall>) -> Result<(), StoreError>` | 成功改 title 并清 error；失败只改 error；调用记录存在时同事务写入 |
| Kernel | `title_model()`、`set_title_model(selection)`；`generate_session_title(session) -> Result<(), KernelError>`；async `title_generating(session) -> bool` | Gateway 使用；手动生成返回已接受，不等待模型；生成中查询只读 core 内存 |
| `GET/PUT /settings/title-model` | 两个必填字段：`model_id`（nullable number）、`reasoning_effort`（nullable string）；PUT 成功 204 | GET 返回选择，节点复用 `/models`；具体模型组合校验同思考选择 B2 |
| `GET /sessions` 与 `/sessions/{id}` | 摘要增加 `title`、`title_error`；会话详情另加 `title_generating: bool`，列表不带 | 列表复用摘要；详情扩展其 DTO；SessionView 仅以 info 持有标题字段 |
| `POST /sessions/{id}/title` | 无 body，成功接受 202 | 所有 Root 可手动生成；不存在 404、非 Root 403、重复生成 409、没有文字 400 |
| `KernelEventKind` | 新增无 payload 的 `SessionTitleChanged`，沿用事件已有 session/channel 字段 | Gateway 当前会话 SSE 转发 `session_title_changed`，只是状态失效通知 |
| 模型调用审计 | `ModelCallPurpose { Conversation, Compaction, Title }`；`NewModelCall` 增加 purpose，调用表列允许 NULL | 新调用标明用途，诊断 SQL 可区分；旧调用不回填 |

标题模型配置验证失败即时返回 400/404/409；已经接受的生成若配置改变或调用失败，通过 title_error 呈现。显式模型删除不清空标题选择，GET 保留失效引用供重选。

采纳可选精简：标题更新重读已有 `GET /sessions/{id}`，不增加标题 GET 或 `SessionTitleView`。`POST /sessions/{id}/title` 仍是生成命令；详情中的运行状态由 Gateway 查询 Kernel 后组装，Store 不持有它。

core 每次标题调用读定标题选择、模型配置与凭据，复用 Provider 工厂；默认模式读当时全局默认。工具列表为空且禁用工具。调用记录 `run_id=None`、context 的 boundary/through 均为空、blocks 为空，不产生 Reply。现行上下文用量通过 run JOIN 取值，标题不会参与；purpose 用于审计，不改该统计路径。

## 三、生成边界与同步

### 输入与输出

自动只用首次文字问题；手动用最近最多 8 条文字问题，按时间正序拼接，总计最多 2,000 Unicode 字符，超出时保留最近内容。只取 Text，不发送图片原件、工具结果或完整历史。提示按用户语言输出简短主题。一次调用、不自动重试；~~总超时 60 秒~~ 改为与正文同样的流式空闲判定（见 [model-stall B2](model-stall.md)，2026-10-15）；限制进 core `limits.rs`。

输出规则（human 已定，2026-10-13）：提示词要求只输出一行纯文本短标题，不带 Markdown、引号、前缀或解释。边界只做 trim，不做格式剥离或截断；正常 EndTurn 后须单行、非空且最多 40 字符，否则明确失败。

### 任务与生命周期

标题有独立的 core 任务管理者：runtime 持有并轮询它的 JoinSet，Kernel 克隆句柄只持有请求入口和共享的生成中集合；不改正文 scheduler 的消息或串行规则。单会话最多一个标题任务，自动/手动复用同一执行路径；runtime 退出取消任务。Store 错误或 panic 按现行 fail fast，模型/config/输出错误只结束标题任务。

generating 只存在内存，认领请求时原子占位；完成先落库再释放占位并发事件，失败也释放。自动触发比较**这条新入站 message 的 id**与库中第一条非空文字 UserInput 的 id，并要求无 title；不是检查「目前共有一条文字」。判断在入站落盘后进行，始终针对新消息，不回放旧消息或重启补跑。后续文字即使首问失败也不再自动重试。无需自动资格标记或特殊迁移事务。

重启后自然不在生成，保留已有标题/错误，不给取消中的任务伪造 failed 状态；没有持久化 generating、启动收尾或自动重跑逻辑。

### 当前会话与侧栏

生成开始、完成/失败均发失效通知。会话 SSE 已在回放前订阅内核事件；客户端在每次 `ready` 后重读会话详情，不需要新的标题快照切点或发布锁。

客户端重读一次只跑一个请求，用 dirty 标记合并刷新：事件到来时标 dirty，当前请求完成后若 dirty 则再读，直到赶上最新状态。仅将响应的 title/title_error/title_generating 合入当前 info，避免覆盖执行中的模型/档位选择保存。事件不携带可能过时的值，所以队列里的旧事件只导致重新读取，不会把旧快照覆盖到新标题。断线/lagged 沿现有重连，ready 再读；GET 失败有可见错误并保留现有显示。

读取成功同步当前页与对应侧栏条目的标题字段，不改变该条目的排序键。列表刷新不能用旧标题覆盖当前打开会话的已读取视图（当前项显示以该 SessionView 为准）。切换会话新增一次列表刷新；其它刷新时机沿用已有 focus/展开机制。未打开会话的刷新边界见第一节，不承诺全列表立即同步。

## 四、迁移

core_sessions 只增加 nullable `title`、`title_error` 两列，初始均 NULL；全局标题选择增加 model/effort 两列，初始 NULL。不增加生成状态、资格或标题历史表。

core_model_calls 增加 nullable purpose，旧记录留 NULL；新普通/压缩/标题调用标明用途，不按 run_id 猜填旧记录。静态模型档位清理仅属于依赖的思考选择 B2，本项不重复迁移。

## 五、验收与关闭

本地验收：首条文字自动一次、纯图后文字、held 文字及命令边界、旧文字会话不补跑；正文并行、手动改名与重复请求；超长/多行/非 EndTurn 拒绝；失败保留旧标题和错误；默认配置、模型删除与非法档位；退出/重启无生成中遗留；当前页接入/重连/事件期间状态最终一致，当前侧栏条目更新且不改排序；未打开条目在约定刷新时更新，`-p` 无标题调用。模拟上游检查输入、工具禁用、档位和调用审计用途。

完成 fmt、clippy、Web check/构建；human 验收真实模型标题质量、标题按钮与用途滑条观感，后关闭，todo 移至归档。此项独立评审和验收，不把思考选择已完成的验收重复作为其关闭条件。

### 本地验收记录（2026-10-13）

- `cargo fmt`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace` 与 `web/` 下 `bun run check`、`bun run build` 均通过。
- 按 human 的测试偏好，本项只新增一条 core/Store/runtime 联动测试，覆盖 held 首问、命令不触发、旧输入不补跑与首问失败后不自动重试；不新增纯解析或内部实现单元测试。
- [整体功能验收脚本](../../scripts/check-session-title.py) 的 11 组隔离 HTTP/SQLite/模拟 Provider 链路通过：标题配置校验，首问自动与重复请求，正文并行，用途审计和消息/排序不变，失败保留旧标题，多行/超长/空白/非 EndTurn 拒绝及只 trim，纯图后首问，旧会话边界，手动输入有界，SSE 开始/完成通知，显式模型删除不兜底，重启无重跑，`-p` 不取标题。脚本只访问本机模拟模型，保留隔离数据目录供排查。
- 页面观感及真实模型标题质量交给 human，暂不标 CLOSED。

### 模型选择闪烁修复（CLOSED，2026-10-13）

- 聊天入口与标题设置共用的模型选择器、思考滑条保存时保持外观稳定，不再变淡或显示等待鼠标；弹层仍可开关，保存期间继续禁止重复修改。
- 标题模型点击后立即显示选择，失败回退并显示错误；「生成模型」旁的保存转圈与「已保存」提示保留。
- 提交 `b337512`；Web check、fmt、clippy 通过。Human 已在服务器确认闪烁解决，本修复关闭。
