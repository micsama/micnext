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

## 实验脚本

- 真实库 SQL 计时：[real_probe.py](real_probe.py)、[real_followup.py](real_followup.py)，结果在 [real-probe-result.json](real-probe-result.json)、[real-followup-result.json](real-followup-result.json)。
- 窗口查询对照：[context_probe.py](context_probe.py)，结果在 [context-probe-result.json](context-probe-result.json)。
- 部分索引的体积与效果：[index-choice-result.json](index-choice-result.json)。
- 合成库上的锁机制实验：[probe.py](probe.py)，结果在 [probe-result.json](probe-result.json)。

这些脚本只输出计时、行数和查询计划，不输出正文和凭据。
