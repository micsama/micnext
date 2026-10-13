# B2：错误传播与统一日志

**状态：已实现，本地验收通过，待 human 验收（2026-10-13）。**

## 目标

1. **日志完整、不滥用**：每次失败在负责处置的地方记一条，带够排查用的上下文；不逐层打印，不刷屏。未被处置的错误与 panic 也要进日志库（不可用时落文本文件）。
2. **错误按标准流程传递**：保留各模块现有错误类型，传到处置边界再决定重试、反馈或退出；只在落库、HTTP、给用户/模型看时转成文本。
3. **为观测留接缝**：以后接 Langfuse 等，挂在现有的模型调用记录和 `tracing` 上，不另做第二套错误判断。

不新增通用错误包装、错误总线、统一调用框架、追踪编号或错误状态表；不改公开类型、HTTP 接口、日志 DTO 与表结构。

## 现状核对（2026-10-13 代码）

| 路径 | 现状 | 缺口 |
|---|---|---|
| 正文取 Provider（`run.rs` `Engine::provider`） | 返回 `String` 用户文案，文案内嵌完整 `ConfigError`（`字段：说明`），WARN 带 `session`/`run` | 信息无丢失，保持 |
| 标题取 Provider（`title.rs` `build_provider`） | 返回 `ConfigError`，WARN 后落库 | 与正文的工厂构建部分逐行重复 |
| 正文调用重试（`run.rs`） | 逐次落审计 + WARN（带 attempt） | WARN 缺 `session`/`run` |
| 压缩调用重试（`compaction.rs`） | 与正文同一套重试逻辑的复制；落审计、发通知 | **无任何日志**；输出校验失败也无日志 |
| 工具结果（`run.rs` `run_tools`） | `ToolError` 转 `ExecOutcome::Failed` 直接落盘给模型 | 依赖故障（`Dependency`）无日志 |
| 服务致命退出（`main.rs` Serve） | `logs.flush()` 在 `result?` 之前，错误只经 `eprintln!` | 致命错误进不了落盘日志和网页日志页 |
| Gateway（`error.rs`） | 4xx 不记，5xx/内部故障记 ERROR | 无，保持 |
| 日志采集（`logs.rs`） | 终端、落盘、网页消费同一 `tracing` 事件；**只取事件字段，不记 span** | 无，保持 |

`ProviderError`、`ConfigError`、`ToolError` 都是平铺文本，没有 `source` 链，`%error` 就是完整信息；带 `#[from]` 嵌套的错误（如 `StoreError`、`anyhow`）记录时用含原因链的格式（`{:#}`）。

## 规则

### 错误传递

- 库函数返回错误，不打印；处置边界（决定重试/反馈/退出的那一层）记一条日志，再转文本。
- 用户文案在边界生成，中文、先说怎么修；日志里保留原始错误，不只记文案。
- 不静默改参数、换模型或伪装成功；重试次数与规则不变，标题仍只调用一次。

### 日志级别

| 级别 | 用于 |
|---|---|
| ERROR | 进程或服务无法继续；HTTP 5xx；日志设施自身降级 |
| WARN | 一次失败已处置但需要人看：模型配置/调用失败（含每次重试）、压缩输出校验失败、工具依赖故障 |
| INFO | 生命周期：run 开始/结束、启动/停止、更新 |
| DEBUG | 下层诊断细节；不重复上层已记的失败摘要 |
| 不记 | 预期的业务反馈：HTTP 4xx、工具 `Input`/`Business` 失败（已作为结果交给模型）|

### 字段名

按实际可得信息附，命名固定：`session`、`run`、`purpose`（conversation/compaction/title）、`model`、`attempt`、`elapsed_ms`、`tool`、`kind`、`error`。字段直接写在事件上，不用 span —— 日志页只采事件字段，改 span 要连带改采集层，本项不做。

## 改动

均在 `mic-core` 与 `bin/micnext` 内部，不改公开契约。

1. **共用 Provider 构建**：`Factories::build` 负责已认领模型 → `Result<Arc<dyn Provider>, ConfigError>`（工厂查找、key 回退、思考档位），正文与标题共用；`Missing`/`Deleted` 的文案仍由各自调用方写（两处措辞本就不同）。正文文案已内嵌 `ConfigError` 全文，不另改返回类型。
2. **共用重试判定与日志**：`Exec::retry_wait` 供正文与压缩共用，输入本次 `ProviderError` 与 `attempt`，记一条 `model call failed` WARN（含 `wait`）并返回「等待多久重试」或「放弃」。各自保留调用方式（流式 / 非流式）、审计落库和用户通知。压缩因此自动获得逐次日志，不在复制品里再补一份。
3. **压缩输出校验失败**：`parse_summary` 失败处记 WARN（`purpose=compaction`、`model`、原因）。
4. **工具依赖故障**：`run_tools` 中 `kind == Dependency` 时记 WARN（`session`、`run`、`tool`、`error`）；`Input`/`Business` 不记。`ToolError`、`ExecOutcome` 形状不变，不增加 source 字段。
5. **服务致命退出**：Serve 分支在 `logs.flush()` 之前对 `assembly.run` 的错误记一条 ERROR（含原因链），随后直接返回失败退出码，不再经 `main` 的 `eprintln!` 输出第二遍。启动早期（日志落盘前）错误与 `-p` 模式仍走 `eprintln!`。
6. 正文、压缩、标题的失败日志补齐 `session`/`run`/`purpose` 字段。
7. **panic 可排查**：`main` 在日志初始化后用标准 `std::panic::set_hook` 替换默认 hook：记一条 ERROR（panic 信息与位置、线程、强制捕获的 backtrace），再同步 `logs.flush()`。终端由日志层输出，不再链上默认 hook，避免打印两遍。日志落盘在独立线程，tokio 任务里的 panic 也在 panic 现场触发 hook，均能进入日志库（库不可用时落文本文件，与现有降级一致）。不引入 panic 相关依赖，不改 `panic` 策略。
8. **吞错点过一遍**：已核对 `let _ =`、`.ok()` 等忽略结果处，均为预期（接收方已关闭的通道、尽力清理、可选解析、grep 单文件读失败按 rg 跳过），无需改动。
9. **Provider 底层日志**：`upstream http failure` 由 WARN 降为 DEBUG，避免与 core 的处置 WARN 重复；状态码与上游提示已在 core 的 `error` 中。字段 `message` 与 tracing 事件消息撞名（日志页被拼进正文），改名 `upstream_message`。未知流事件仍为 WARN（无上层处置记录）。

## 观测接入的接缝（Langfuse 等）

本项不接入，不加依赖与配置；只确认将来接入不需要返工：

- **模型调用**：正文、压缩、标题都经 `record_model_call` 落一条 `NewModelCall`（purpose、session、run、model、起止时间、用量/失败原因）。这是唯一的模型调用真相，将来上报 generation 从这里取，失败标记与审计一致，不另判一次。
- **日志**：都走 `tracing`，将来可加一个导出层（如 OpenTelemetry → Langfuse 的 OTLP 入口），不改调用点。若届时需要 trace 层级（run → 模型调用 → 工具），再引入 span 并同步让日志页采 span 字段；上面固定的字段名可直接平移为 span 字段。
- **已知盲区**：工具耗时与首 token 时间尚未记录（见 todo 统计行条目），接入时一并补，不在本项。压缩调用的审计记录 `run_id` 为空（[context-compaction](context-compaction.md) 既定契约），归入 trace 时需按会话与时间关联。
- 用现有 SQLite 自建 trace 视图另见 [B1 run-trace](../brainstorm/run-trace.md)；本项固定的 `session`/`run` 字段让日志能按 trace 关联。

## 验收与关闭

以联动与整体验收为主，不写格式化/字段拼装的单元测试：

- 模拟模型缺失/删除/配置错、上游 Transient 重试至放弃、Account 失败、压缩输出为空、工具依赖故障：核对用户反馈与审计不变，日志页可见带字段的 WARN，重试逐次一条，无上下层重复摘要。
- 让一个服务启动后失败：落盘日志与日志页可见 ERROR，终端只输出一次，退出码为失败。
- 临时注入主线程与 tokio 任务内的 panic：日志库可查到带位置与 backtrace 的 ERROR；验后移除注入。
- 完成 fmt、clippy。真实环境日志由 human 验收，通过后关闭并归档 todo。

### 本地验收记录（2026-10-13）

隔离数据目录 + 本地假上游（503 / SSE 脚本）经 Web API 驱动，核对日志库 `logs.db`：

- 正文 503 重试：attempt 1/2/3 各一条 `model call failed`（`purpose=conversation`，`wait` 2s/4s/None），带 `session`/`run`/`model`；Provider 的上游失败只在 DEBUG，未落盘重复。
- `/compact` 遇 503：同样逐次三条（`purpose=compaction`）；审计三行、用户通知文案不变。
- 压缩输出为空：`compaction output rejected`（`error=总结正文为空`）。
- `read` 读无权限文件：`tool dependency failed`（`tool=read`）；web_fetch 的 Business 失败不记。
- 端口被占：`stopped on failure` ERROR 进日志库，终端仅一次，退出码 1。
- tokio 任务内注入 panic：ERROR 含位置、`thread=tokio-rt-worker` 与 backtrace，终端仅一次；注入已移除。
- 未覆盖：模型缺失/删除/配置错与 Account 失败（走既有 WARN，逻辑未改）、主线程 panic（与任务内同一 hook）。
