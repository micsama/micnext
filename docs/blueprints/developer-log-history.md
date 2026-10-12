# B2：开发者日志留痕

**状态：已批准并实现（2026-10-12），fmt/clippy/前端 check 通过，待人工验收后 CLOSED。**

依据：[B1](../brainstorm/developer-log-history.md)。目标是打通流程：重启（手动或一键更新）后网页仍能看到之前的日志。存哪些、删多少都通过 `limits.rs` 常量调整，后续按使用情况再调。本文替代 [开发者诊断](developer-diagnostics.md) §2 中「内存日志、重启清空」一条，其余条款不变。

## 一、用户行为

- 日志页打开时，先显示之前启动的最近日志，再接本次启动的实时日志。启动之间显示分隔行（「—— 启动于 <时间> ——」）。
- 新增快捷范围「全部 / 本次启动以来」，默认「全部」。切换只改显示，搜索、级别、模块筛选和复制都作用于当前范围。
- 日志库不可用时，页面顶部提示「日志库不可用（原因），已改写到 <路径>」或「日志只在内存」。服务照常运行。
- 页面说明文字由「内存日志，重启清空」改为「保留最近日志，最多 100 MB」。

## 二、Rust 公开契约

`mic-gateway`：

```rust
impl DeveloperLogs {
    pub fn new() -> Self;              // 不变；同时记下 boot_id = 当前毫秒时间戳
    pub fn layer<S>(&self) -> impl Layer<S> + ...;   // 不变
    /// 开始把日志落到 `dir`（不存在则创建）。只调用一次；之前已采集的本次记录一并补写。
    /// 不返回错误：失败按 §三 降级并 emit 一条 ERROR。
    pub fn persist(&self, dir: &Path);
    /// 等待已排队的记录写完，最多 `LOG_FLUSH_TIMEOUT`；超时则放弃剩余记录。阻塞调用。
    pub fn flush(&self);
}
```

`mic-core`：

```rust
impl Assembly {
    /// 已解析的数据目录。
    pub fn data_dir(&self) -> &Path;
}
```

新依赖：`mic-gateway` 增加 `rusqlite`（与 `mic-store` 同版本，`bundled`）和 `chrono`（用于降级文件的时间格式，与 `mic-message` 同配置）。不新增跨内部 crate 依赖。

## 三、存储与降级

目录 `<data_dir>/logs/`。写入只有一个后台 OS 线程，由 `persist` 启动。

- **落盘范围**：INFO 及以上，记录内容与内存中一致（已有单条 16 KiB 截断）。DEBUG 只留在内存。
- **入队**：采集层在 `push` 持有缓冲锁时 `try_send` 到有界队列（`LOG_PERSIST_QUEUE`）。`persist` 在同一把锁内装上发送端，并把缓冲中已有的本次 INFO+ 记录入队，保证顺序不漏不重。队列满时丢弃并计数，写入线程在下一批写入一条 WARN「日志落盘跟不上，丢弃 N 条」。
- **SQLite**（`logs.db`，独立连接，WAL，`auto_vacuum = INCREMENTAL`）：

  ```sql
  CREATE TABLE log (
    id           INTEGER PRIMARY KEY,
    boot_id      INTEGER NOT NULL,
    timestamp_ms INTEGER NOT NULL,
    level        TEXT    NOT NULL,   -- info / warn / error
    target       TEXT    NOT NULL,
    message      TEXT    NOT NULL,
    fields       TEXT    NOT NULL,   -- JSON [{name, value}]
    file         TEXT,
    line         INTEGER,
    truncated    INTEGER NOT NULL
  );
  ```

  `user_version = 1`。版本不符时删库重建，并 emit WARN（日志可丢，不阻塞启动）。按批写入，一批一个事务。每写入 `LOG_TRIM_EVERY` 批检查一次库大小，超过 `LOG_DB_BYTES`（80 MB）就删掉最旧的 1/5，然后执行 `incremental_vacuum` 和 WAL checkpoint。
- **降级**：打开或写入 SQLite 失败后，本进程改为追加写 `logs/micnext.log`，不再尝试 SQLite。每条一行：`<RFC3339 本地时间> <LEVEL> <target> <message> k=v …`。文件超过 `LOG_FILE_BYTES`（16 MB）时清空重写。降级文件也写失败，就只留内存，丢弃之后入队的记录。每次降级 emit 一条 ERROR 写明原因。
- **状态**：当前存储状态（`Sqlite` / `File { path, reason }` / `Memory { reason }`）保存在 `DeveloperLogs` 内，供 §四 读取。
- **自发日志**：写入线程自己 emit 的事件同样经过 `try_send`，不阻塞，不会形成自等待。`flush` 用独立的应答通道，等待有超时上限。

两者合计上限：库 80 MB，加上 WAL 上限（`journal_size_limit` 4 MB），再加降级文件 16 MB，约 100 MB。

## 四、HTTP

`GET /api/developer/logs/history`，Bearer 鉴权，无 query，`Cache-Control: no-store`。

```json
{
  "storage": { "kind": "sqlite" }
           | { "kind": "file", "path": "...", "reason": "..." }
           | { "kind": "memory", "reason": "..." },
  "boot_id": 1760250000000,
  "records": [ { "boot_id": 1760240000000, "timestamp_ms": ..., "level": ..., "target": ..., "message": ..., "fields": [...], "file": ..., "line": ..., "truncated": false } ]
}
```

`records` 是 `boot_id != 当前` 的最近 `LOG_HISTORY_RECORDS`（2,000）条，按时间升序。在 `spawn_blocking` 里用只读连接查询。存储不是 `sqlite` 时返回空数组，不解析降级文件。查询失败返回 503「读取日志库失败：…」。`persist` 之前状态为 `memory`（「未启用落盘」）。

SSE 流不变。

## 五、页面

`api/developer.ts` 新增 history 的解码器。`LogsTab`：每次收到 SSE `ready` 时重新拉取 history（与重连一致），history 记录放在实时记录之前，并按 `boot_id` 插入分隔行。范围切换与提示条按 §一。history 拉取失败时提示条报错，实时日志不受影响。

## 六、main

```rust
let logs = DeveloperLogs::new();          // 不变
// … Assembly::new 之后，仅 Serve：
logs.persist(&assembly.data_dir().join("logs"));
let result = assembly.run(stop).await;
tracing::info!("stopped");
logs.flush();
result?;
// 之后照旧 exec
```

`-p` 不调用 `persist`，不创建日志目录。配置解析失败等早于 `persist` 的错误不落盘，照旧打到 stderr。

## 七、兼容

| 调用方 | 影响 |
|---|---|
| main：`DeveloperLogs::new` / `layer` / `GatewayModule::new` | 不变 |
| main：Serve 分支 | 新增 `persist`、`flush`；`run` 出错时也先 flush |
| `Assembly` 其他调用方 | 新增 getter，无影响 |
| SSE `/developer/logs/stream` | 不变 |
| 前端 LogsTab | 新增 history 拉取、范围切换、提示条 |
| [developer-diagnostics](developer-diagnostics.md) §2「重启清空」 | 由本文替代；关闭时在该文档补一行指向本文 |

## 八、实体与不变量

| 实体 | 唯一写者 | 生命周期 |
|---|---|---|
| 内存缓冲 / seq | 采集层 | 不变 |
| 落盘队列 | 采集层（`try_send`） | `persist` 前不存在 → 运行 → 进程退出 |
| `logs.db` / `micnext.log` | 写入线程 | 跨进程保留，超限删最旧 |
| 存储状态 | 写入线程 | Sqlite → File → Memory，只降不升 |
| boot_id | `DeveloperLogs::new` | 进程内不变 |

不变量：

- 采集和 tracing 永远不因磁盘阻塞。
- 日志存储故障不影响服务运行，只降级并在页面可见。
- 日志库与业务库 `micnext.db` 分离，不占用业务连接。

## 九、实现顺序与验收

`Assembly::data_dir` → 写入线程与 SQLite/降级文件 → `persist`/`flush` 接入 main → history 接口 → 前端 → 收尾 `cargo fmt`、`cargo clippy -- -D warnings`、前端 check/build。

人工验收：

1. 启动后发几条消息，在网页一键更新或手动重启，页面能看到上一次启动的日志和分隔行。切到「本次启动以来」后只剩本次。
2. `chmod 000 <data_dir>/logs/logs.db` 后重启：服务正常，页面提示已改写到 `micnext.log`，文件有内容。
3. 临时把 `LOG_DB_BYTES` 调小，运行一段时间后库大小稳定在上限附近，最旧的记录被删。
4. `-p` 运行不产生 `logs/` 目录。
