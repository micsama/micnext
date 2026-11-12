# 慢的会话切换自动留证

状态：已批准并实现（2026-11-12），待线上收集。**这是临时诊断**：找到原因后按文末「移除」整体删掉。

## 问题

长会话切换偶尔卡 1–3 秒，事后复现不了。已经排除的项：
- 服务端：生产副本各接口 ≤7ms；线上本机直连中位 2.4ms，有会话在跑时也一样，CPU 约九成空闲。
- 数据量：首屏只有约 23 条，约 70KB。
- 协议与带宽：线上走 HTTP/2，带宽约 465KB/s。

Safari 下 EventSource 正常首屏 140–383ms；有一次建连就花了 241ms（平时约 40ms）。剩下的嫌疑是网络或 Caddy 抖动、Safari 主线程忙（尤其旁边有会话在流式输出时）、标签页被节流。

单记一句「这次慢了」没法排查。目标是：**慢的那一次自动把分段耗时和各段的旁证一起记下，看日志就能定位到具体哪一段、为什么慢。**

## 做法

前端按阶段给每次打开会话打点。超过阈值的那次，把分段和旁证 POST 给服务端，记一条 WARN 到运行日志。快的不报。

### 分段（前端，相对点击时刻，单位 ms）

| 字段 | 时刻 |
|---|---|
| `session` | `getSession` 返回（已缓存则为 null） |
| `open` | EventSource `onopen`，即流响应头到达 |
| `first` | 第一条 `message` 事件 |
| `ready` | `ready` 事件 |
| `painted` | `ready` 之后 Svelte `tick` 加一帧 `requestAnimationFrame`，即列表画出来 |

### 旁证：用来解释「为什么慢」

| 字段 | 内容 | 能说明什么 |
|---|---|---|
| `server` | 服务端从收到流请求到发出 `ready` 的分段：`replay_ms`（查回放页）、`state_ms`（查运行状态）、`total_ms` | `ready - open` 远大于 `total_ms`：数据卡在网络或代理途中；`total_ms` 本身大：服务端慢 |
| `net` | `getSession` 请求的 Resource Timing：DNS、建连、TLS、首字节、下载，`nextHopProtocol`，是否复用连接（建连为 0） | 建连或 TLS 大：连接被断开后重建；首字节大：网络往返慢 |
| `stream` | 到 `ready` 为止的事件数、数据字符数（`chars`）、相邻事件的最大间隔 | 最大间隔接近总耗时：流被整段压住后一次放出（代理缓冲或 Safari 压尾） |
| `lag_ms` | 加载期间主线程的最大延迟（50ms 心跳定时器的实际偏差；Safari 不支持 longtask） | 大：主线程忙，事件到了却处理不了 |
| `render_ms` | `painted - ready` | 大：渲染慢，跟消息条数、`running` 对照 |
| `hidden` | 加载开始时标签页是否隐藏，加载期间是否切过可见性 | 后台标签被节流，这种慢不算问题 |
| `running` | 当时侧栏里正在运行的会话数 | 验证「别的会话在跑时更卡」 |
| `reconnect` | 是首次打开还是断线重连 | 重连多带一次退避等待，分开看 |
| `messages` | 渲染出来的消息条数 | 跟 `render_ms` 对照 |
| `ua` | User-Agent，截到 200 字 | 区分浏览器和设备 |

## 契约变更

- **`ready` 事件** 加一个字段 `timing: { replay_ms, state_ms, total_ms }`。服务端在 `feed()` 里用 `Instant` 计时；重连时 `replay_ms` 指续传查询。前端只读这个字段，旧字段不变。
- **新路由** `POST /api/developer/session-load`，需要登录，返回 204。
  - 请求体是上面的强类型结构，`deny_unknown_fields`，数字为 `u32`，`ua` 截断，超过 4KB 返回 400。
  - 服务端记一条 `tracing::warn!(target: "web", session_id, ...各字段, "session load slow")`，按现有规则落到日志库，在 开发者诊断 → 运行日志 里可见、可筛选。
  - 不新建表，日志库结构不变。字段数在 `LOG_FIELDS` 范围内。
- **阈值**：超过 800ms 才上报，写成前端常量。画出来时、或没等到就离开（切走、断线）时结算，每次打开最多报一次，失败就丢弃，不重试。没等到就离开的那条，后面几个阶段为 `-`。
- 不加依赖。

## 不做

- 不给快的加载记日志，不做采样统计和看板。
- 不加请求 id 关联：`ready` 自带服务端耗时，单条记录就够判断。
- [请求与数据库耗时观测](storage-request-timing.md) 照旧挂起。这次以前端为主，服务端只加 `ready` 里的那一段。

## 附带发现（不在本 B2 内）

每次切换会话时，`sessions/{id}`、`seen` 和 `context` 各请求了两次（其中一处是 `ready` 后 `#refreshTitle` 又取了一遍）。这是私有整理，不改契约，已一起修掉，**是常驻修复，不随诊断删除**：回放中的消息不再逐条触发 `context` 和 `seen`，由 `ready` 统一刷新一次；首次连接不在 `ready` 后重取会话信息，只有重连才取。

## 验收

- 本地：用 devtools 把网络限速到 Slow 3G 并切换会话，运行日志出现一条 WARN，各段数字和限速一致；不限速时切换不产生日志。
- 线上：human 正常使用一段时间，再从运行日志里按 `session load slow` 筛出记录，看能不能用上面的旁证表把每次慢都归到一个原因。

## 移除

所有临时代码都带「临时诊断（session-load-diagnostics）」注释，`grep -rn 临时诊断 crates web/src` 即可列全：
- Gateway：`load_report.rs` 整个文件，`lib.rs` 的 `mod load_report`，`service.rs` 的路由，`limits.rs` 的 `LOAD_REPORT_*`，`stream.rs` 的 `Timing` 与 `ready.timing`。
- Web：`state/load-probe.ts` 整个文件，`api/client.ts` 的 `reportSessionLoad`，`api/types.ts` 的 `ReadyTiming` 与解码，`api/stream.ts` 的 `chars` 和 `onOpen` 参数，`state/session.svelte.ts` 里 `probe` 相关各行。
- 本蓝图标 `CLOSED`，在结论里写清楚查出的原因。
