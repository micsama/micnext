# 会话与 Trace 加载卡顿审查

2026-10-28。状态：已修复（`b874279`），服务器更新后不再卡顿。

## 现象

- Web 会话和 Trace 加载慢，后来整页一直转圈：侧栏列表取不到，会话只显示「会话 25」这类占位名。单页、无痕窗口都能复现。
- 不访问业务库的接口（`update`、`stream-ticket`、`model-kinds`）几十毫秒返回；`settings`、`models`、会话列表等访问业务库的接口全部挂起。
- 服务器本机直连 `http://127.0.0.1:7878/api/settings`，带有效凭据，15 秒超时，没有收到任何字节。浏览器、公网和反向代理都排除在外，问题在应用内部。
- 过很久页面会自己恢复：浏览器断开后，服务端已派出的查询不会取消，要等排队的重查询全部跑完。

## 根因

1. `Store` 的所有业务读写共用一个 `Arc<Mutex<Connection>>`，查询和解码都在锁内执行。一个慢查询会让其他请求一起排队。
2. `trace/mod.rs` 的调用查询对每次模型调用按 `model_call_id + kind` 反查 3 次 `core_messages`，这两列没有索引，每次反查都全表扫描。用到它的有 Trace 每页 run、`session_usage`（Trace 顶栏）、`schedule_usage` 和看板。
3. 用量和看板复用了这条查询，带出汇总根本不用的回复、边界关联，还逐行算本地日期。仅逐行算日期一项，17,730 行就要约 0.9 秒。

在真实库上，单次会话用量查询要 96 秒，正好落在挂起的量级。

## 数据形状

数据来自服务器备份，16 MiB，schema core=29，统计见 [real-data-summary.json](real-data-summary.json)。

| 项目 | 数量 |
|---|---:|
| 会话 | 37 |
| run | 9,169 |
| 消息 | 37,365 |
| 模型调用 | 18,662 |
| 会话 25 的消息 / 调用 | 35,465 / 17,730（约占调用总数的 95%） |

- **会话 25 是怎么来的**：它是一个定时任务会话，规则是 `* * * * *`，10 月 22 日至 28 日共触发 8,854 次。数据库整体并不大，是一个高频自动化把问题放大了。
- **单会话长期增长本身是设计内的**：定时任务和微信都是一个会话一直累积，靠压缩边界控制送给模型的内容，所以查询必须按会话规模可扩展。
- **这个任务的消耗**：输入合计 9.39 亿 token，其中 99.6% 命中缓存；输出 84 万 token。触发频率和到点前压缩另见 [B2 schedule-limits](../../blueprints/schedule-limits.md)。

## 修复

修复在 `core-31.sql` 里，不改公开契约，由 build.sh 在停服后执行。

- **加索引**：
  - 调用反查（部分索引，只含有关联调用的消息）；
  - 定位边界；
  - 按 `(session_id, created_at, id)` 读窗口；
  - 看板的三个时间范围；
  - 会话的失败工具结果（部分索引）。
- **用量与看板只读需要的列**：看板先把本地日期换算成时间戳范围再读，不再逐行算日期。
- **读上下文窗口按范围定位**：边界带过来的未答输入单独取，代价只随窗口大小增长。
- **`subscribe_from_now` 只取最后一条 id**：以前要读整个会话，现在不再这样做。

## 前后对照

测量用 mic-store 的 `real_database_reads_stay_bounded`（`#[ignore]`）直接调 store 方法，不经过 HTTP。

两边是同一份真实库副本，都在本机 release 下运行。修改前是 `b0cc28e` 代码配 v30 库，修改后是 `b874279` 代码配 v31 库。

| 项 | 修改前 | 修改后 |
|---|---:|---:|
| context_window | 27ms | 4ms |
| context_usage | <1ms | <1ms |
| 最后一条 id（订阅） | 61ms | <1ms |
| trace_sessions | 21ms | 2ms |
| trace_runs（首页） | 64ms | 4ms |
| session_usage | 95.7s | 7ms |
| schedule_usage | 96.4s | 11ms |
| dashboard 30 天 | 95.4s | 22ms |
| dashboard 366 天 | 93.0s | 20ms |

修改前整轮用时 381 秒，修改后 0.09 秒。这份库只有一周历史，所以 30 天和 366 天看板覆盖的数据差不多。

### 复测方法

1. 在仓库外复制一份库，把 `core_endpoints.key_cipher` 清空。这样不需要 `master.key`，测试会在副本旁生成临时密钥。
2. 用 `python3 scripts/db/convert.py --db <副本> --apply` 把副本转换到当前版本。
3. 运行 `MICNEXT_PERF_DB=<副本> cargo test -p mic-store --release real_database -- --ignored --nocapture`。

测量取消息最多的会话，任一项超过 300ms 即失败。副本含真实对话，只留在本地，测试输出只有耗时。

数据不做放大：复制行会撞上唯一约束和关联 id，而且放大出来的也不是单个超长会话的形状。

## 遗留与候选

下面这些目前都不慢，没有实施，等服务器实测后再定。

| 问题 | 证据 | 候选 |
|---|---|---|
| 单连接互斥仍在：一个重查询会拖住所有请求 | 真实库上让会话用量重查询先占锁，settings 要等 938ms；重查询改用独立 WAL 只读连接后，settings 只要 0.05ms | 给 Trace、看板、搜索开有界的只读连接 |
| 搜索不能走索引 | `search_sessions` 用 `instr(lower(body), term)` 扫描正文，查不到时会扫完全部候选 | 按真实用量再定，可以考虑 FTS5，但要注意中文短词的匹配语义 |
| Trace 每页解码完整正文 | `trace_runs` 完整读取 20 个 run 的消息，再截成 200 字预览 | 只读预览需要的字段 |
| 前端串行等待 | 会话页先取会话再取票据，最后才连流；Trace 要等会话树和 peers 都返回才加载 run | 让这些请求并发 |
| 会话列表刷新面过宽 | 任意 `session_activity` 都会重拉所有渠道已加载的页 | 按事件里的会话 id 缩小刷新范围 |

存储引擎、分表和多租户的取舍见 [B1 storage-concurrency](../../brainstorm/storage-concurrency.md)。结论是先不换 SQLite，也不按压缩边界做物理分表。

## 下一步怎样观测和验收（候选，未实现）

不新建性能平台或独立 crate；沿现有职责放置，复用开发者日志链路。

| 位置 | 记录或验证什么 |
|---|---|
| `mic-store/src/store.rs::blocking` | 固定业务操作标签；提交到阻塞闭包开始的调度等待、获得连接锁前的等待、持锁工作的耗时。锁释放后发完成事件。持锁工作包含 SQL、取行、解码和汇总，不能标成纯 SQL 时间；热点再单独分段 |
| Gateway 内部请求中间件 | 请求关联 id、方法、路由模板、状态、响应构建耗时；关联 id 显式带入 Store 的阻塞任务。第一版 SSE 只记录响应准备完成，首事件阶段留待有需要时补充，正常长连接存续不算慢请求。服务端耗时不包含浏览器下载和渲染 |
| Store 显式启用的性能测试 | 先测轻读/轻写基线，再并发叠加看板、Trace、搜索；限制并发和提交间隔，记录样本数、实际吞吐、p50/p95/p99、最大耗时、错误和超过 300ms 的次数。仅用真实库临时副本，经 Store 写入后检查结果，退出前等待阻塞任务结束；不启动模型、定时任务或外部 Channel |

现有 `mic-gateway/src/logs.rs` 已有有界缓冲、独立落盘线程和日志页面：项目 DEBUG 入内存，INFO 及以上落盘，**不记录 span**。因此只添加 `#[instrument]` 不会自动在页面显示耗时和父 span 的关联字段。首版发显式结构化 event，正常明细使用 DEBUG，慢操作才持久化；需要观察尚未完成的操作时，诊断模式增加配对开始事件。不要记录原始 URL/query（含 stream ticket）、Authorization 或正文，也不要把并发子操作的耗时之和当作请求墙钟时间。

浏览器 Network 继续负责网络分阶段与传输诊断，服务端日志补上排队和持锁信息；无需重做 Network 面板。HTTP 路径的并发验收另做，Store 测量不能冒充端到端结果。

`mic-store` 当前没有 `tracing` 依赖。正式落地需先按 B2 写清依赖、观测字段及取消语义；Store 只依赖通用 tracing，不反向依赖 Gateway。第一版不必新增 HTTP 响应头或跨 crate 的观测服务。只读连接隔离作为后续独立变更，由混合负载结果确定优先级。

### 首轮混合负载结果（2026-10-28）

已新增 `trace::tests::real_database_mixed_load`（ignored）。本机 Darwin arm64、Rust 1.96、release，运行代码基于 `c587b6d`，同一服务器备份的独立临时副本转换到 core=31。测试未改变生产运行逻辑，没有修改原下载库；结束核对原文件与先前只读基准副本的 SHA-256 一致。

每阶段约 3 秒，每种操作一个 worker、最多一个请求在途，两次起始间隔至少 10ms；慢时不追赶，因此是有界闭环负载，并非保证固定 QPS 的到达率测试。基线与恢复阶段为 settings + append；混合阶段另外运行 dashboard(366)、trace_runs(20) 和不存在词的 Root 会话搜索。消息追加到临时副本里的独立测试会话，不启动内核、模型或任务。

| 操作 | 基线 p95 / 样本数 | 混合负载 p95 / 样本数 | 恢复阶段 p95 / 样本数 |
|---|---:|---:|---:|
| settings | 0.141ms / 254 | 105.940ms / 30 | 0.333ms / 256 |
| append | 0.542ms / 253 | 105.744ms / 30 | 0.598ms / 253 |

混合阶段全部操作的最大耗时为 151.992ms；没有失败，没有超过 300ms，成功返回的 536 条追加全部存在，quick_check 和外键检查通过。混合阶段轻请求吞吐从每类约 84 次/秒下降到约 10 次/秒，恢复阶段回到约 84–85 次/秒。原始样本未入库，复测方法见下。

**新结论**：优化后这份库在本次有界混合负载下没有卡死，但重读对轻读/轻写的干扰仍可测得，且停止重读后恢复。当前只测 Store 调用总时间，不能把增加的全部时间直接归为锁等待；下一步分阶段观测正用于区分调度、锁及 CPU/磁盘工作。每项混合负载只有约 30 个样本，p99 几乎就是最大值，不能当作稳定的尾延迟承诺。没有验证固定到达率下的积压、HTTP 并发、取消、长期 WAL 增长或服务器容量，也没有证明新增只读连接后的改善程度。

复测：先按上方方法准备**可丢弃副本**，再运行 `MICNEXT_PERF_WRITABLE_DB=<副本> cargo test -p mic-store --release real_database_mixed_load -- --ignored --nocapture`。该测试会写入新测试会话与消息，并在副本旁写 `.mixed-results.json`；不能指向原备份或服务正在使用的库。观察阈值不是取消期限，测试等待已发出的操作完成。

运行时观测已细化到 [B2 请求与数据库耗时](../../blueprints/storage-request-timing.md)，新增依赖和日志字段契约待批准。先行代码通过 `cargo fmt`、`cargo clippy -- -D warnings`、`cargo clippy -p mic-store --tests -- -D warnings`；Store 普通测试 19 项通过，真实库混合测试单独通过。运行时日志尚未实施。

## 实验脚本

- 真实库 SQL 计时：[real_probe.py](real_probe.py)、[real_followup.py](real_followup.py)，结果在 [real-probe-result.json](real-probe-result.json)、[real-followup-result.json](real-followup-result.json)。
- 窗口查询对照：[context_probe.py](context_probe.py)，结果在 [context-probe-result.json](context-probe-result.json)。
- 部分索引的体积与效果：[index-choice-result.json](index-choice-result.json)。
- 合成库上的锁机制实验：[probe.py](probe.py)，结果在 [probe-result.json](probe-result.json)。

这些脚本只输出计时、行数和查询计划，不输出正文和凭据。
