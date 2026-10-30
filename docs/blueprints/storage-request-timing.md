# 请求与数据库耗时观测

状态：2026-10-28 human 同意尝试观测与并发验收；本文件把运行时改动细化为待批准契约。混合负载测试不改变公开契约，先行实现；运行时日志尚未实施。

## 问题与目标

索引及读取范围修复已经解除服务器卡顿，但现有日志无法区分请求慢在 blocking 调度、连接锁还是实际工作。复用开发者日志页面和独立日志存储，在 Gateway 和 Store 两个入口补事件；不新建 crate、监控后台或前端页面。

证据和复测见 [加载审查](../reviews/web-loading-2026-10-28/README.md)，执行隔离候选见 [存储并发 B1](../brainstorm/storage-concurrency.md)。本次不改连接数量、SQL 语义、事务、读写取消、请求超时或调度策略。

## 待批准的公开契约

- `mic-store` 增加 `tracing = "0.1"` 直接依赖，复用工作区已有版本；不依赖 Gateway，也不新增公开 Rust 类型/签名。
- HTTP 路由、响应体及响应头不变。现有开发者日志的通用 `fields` 增加下述事件字段，旧日志仍可读，不改日志数据库结构。
- Gateway 的日志采集层只从请求 span 补取白名单字段 `request_id`，不收集任意父 span 的内容。显式 event 字段优先，同名字段不重复。原来“不记录 span”调整为“不记录 span 本身，仅继承请求关联 id”。

## 事件与落点

### Store

在现有 `store.rs` 的私有 `call` / `call_settings` / `blocking` 入口统一计时。私有入口增加固定操作名参数，各调用点使用 `模块.操作`，不能传 SQL、搜索词或任意用户输入；公开 `with_module_tx` 保持签名，先记录为统一的 `module.transaction`，不为了细分外部模块修改其契约。

固定事件：`store operation completed`。

| 字段 | 口径 |
|---|---|
| `operation` | 固定操作标签 |
| `dispatch_wait_ms` | 提交 spawn_blocking 前到闭包实际开始 |
| `lock_wait_ms` | 闭包开始到取得连接锁 |
| `work_ms` | 取得锁到业务闭包返回，包含 SQL、取行、解码、Rust 汇总与事务提交 |
| `total_ms` | 提交到业务闭包返回，不包含 await 方重新被调度或日志写入 |
| `outcome` | `ok` / `error`；错误原因仍在现有消费边界记录，避免重复 |
| `request_id` | 请求关联时由采集层继承；后台操作允许缺省 |

捕获当前 tracing span 并显式带进 blocking 闭包；闭包内同步 enter，不能持有 enter guard 跨 await。先释放业务连接锁，再发日志。事件本身不进入业务 SQLite。启动初始化不在第一版覆盖范围内。

调用者取消后，已开始的 blocking 操作仍可能完成，完成事件应由闭包发出。日志描述实际执行完成，不表示原 HTTP 调用方仍在等待，更不代表所有写操作可取消。panic 沿现有失败路径处理，不伪造正常完成事件。本次不为观测增加后台任务或改变提交/发布所有权。

### Gateway

在请求入口放一个内部中间件，生成本进程单调 request id，通过 `.instrument(...)` 为请求 future 建立请求 span；不采信客户端传入的关联 id。关联 id 在一次进程生命周期内唯一，重启使用现有日志 boot 标识区分。

固定事件：`http response prepared`，字段 `request_id`、`method`、`route`（匹配模板）、`status`、`elapsed_ms`、`streaming`。未知路由使用固定占位名，不记录原始 URI、query、Referer、鉴权头或正文。将中间件放在能够覆盖鉴权失败、且能够取得匹配路由模板的位置。

`elapsed_ms` 截止响应对象准备完成，不是客户端收到首字节、完整下载或渲染完成。SSE 的建立响应也记录这个阶段；第一版不改 stream body，不记录连接总存续时间，也不声称测到了首事件。请求 future 提前被丢弃时可以没有 response-prepared 事件，不能把它视为成功或查询已取消。

现有日志采集层只补取 request id，仍按原有条数/字节预算截断；日志消费页面无需修改。普通后台 Store 操作独立可读；不为追踪所有跨任务因果链改动 Core 的任务入口。

### 级别与开销

完成耗时达到 300ms 的 Store 或 HTTP 事件用 INFO 持久化，正常明细用 DEBUG 进入现有有界内存缓冲。300ms 是诊断起点，不是新的请求超时或性能承诺，分别放入所属 crate 的 `limits.rs`。慢请求和慢 Store 是两个不同层次的事件，通过 request id 关联，不把并行子操作耗时直接相加。

第一版只记录完成事件，没有额外定时扫描器、正在执行任务注册表和开关配置；尚未返回的请求仍结合 Network 观察。若生产日志量实测不可接受，再针对已测开销制定采样策略，不在本轮引入隐式丢弃业务或日志的机制。

## 先行混合负载测试

在现有 `trace/tests.rs` 增加 ignored 测试，使用独立的 `MICNEXT_PERF_WRITABLE_DB` 指向可丢弃真实库副本。分 baseline、mixed、recovery 三阶段；baseline/recovery 同时运行 settings 和 append，mixed 再加看板、20 个 run 的 Trace 和无命中搜索。

每类操作一个 worker、最多一个在途操作；每次起始间隔至少 10ms，慢时不追赶积压，是**有界闭环负载**，不是固定到达率测试。每阶段约 3 秒停止提交，然后等待在途任务结束。不注入取消或重试，不设置会制造未决写入的客户端超时；超过 300ms 仅计数，不冒充 timeout。p99 是本次有限样本统计，必须同时报告样本数和实际吞吐。

报告只含各次耗时、分位数、失败数、超过 300ms 的次数与吞吐；保存到副本旁的 `.mixed-results.json`。测试独立创建消息会话，结束检查成功写入数、quick_check 和外键约束。它不运行 Gateway、Kernel、Provider、定时任务或 Channel，因此不能证明 HTTP、事件发布、WAL 长时间增长或多人容量。搜索只覆盖现有 Root 会话范围，不能把定时会话的大数据量算作已搜索。

## 批准后验收

1. Store：构造连接锁争用，确认锁等待与 work 能分开；结果/事务行为与取消后完成日志不受影响。
2. Gateway/日志：匹配路由、鉴权失败、并发请求 id 隔离；敏感 URL 与正文不进入日志；现有页面能看到 event 的关联字段；SSE 建立立即记响应准备耗时，不等断流。
3. 在同一真实库副本上对比观测开启前后混合负载，说明测量开销。CPU/磁盘资源共享仍可能影响结果，不承诺只读连接会消除所有干扰。
4. `cargo fmt`、`cargo clippy -- -D warnings`、相关模块测试；无前端改动无需前端检查。服务器日志可用性由 human 验收后 CLOSED。

## 先行验收记录

真实库副本 release 测试通过：settings p95 从基线 0.141ms 升至混合负载 105.940ms，append 从 0.542ms 升至 105.744ms；恢复阶段分别为 0.333ms、0.598ms。零错误、零超过 300ms，536 条写入全部验证存在；quick_check、外键检查通过。每种混合操作约 30 个样本，只作为干扰证据，不作稳定 p99 或服务器容量承诺。

详见 [审查中的完整口径](../reviews/web-loading-2026-10-28/README.md#首轮混合负载结果2026-10-28) 和 [样本](../reviews/web-loading-2026-10-28/mixed-load-result.json)。`cargo fmt`、工作区 Clippy、Store 测试目标 Clippy、19 项普通 Store 测试及独立运行的真实库混合测试通过。运行时改动未实施，尚不能从这组结果分离锁等待与实际工作耗时。
