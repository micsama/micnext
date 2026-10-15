# Web 更新后消息尾部不可见

状态：已在受测 Safari 复现与 WebKit 322545 一致的行为，human 定采用原生 EventSource + 一次性票据，进入 [B2](../blueprints/web-sse-eventsource.md)；已实现、补注释已删，Safari/iPhone 现场待 human 验收（2026-10-15）。

历史处理：
- Chrome：消息分批渲染时贴底判断出现竞争，导致停在中间（3b98736）。
- Safari/iOS：曾按 WebKit 滞留流数据处理，每批事件后补空注释（fb75c7d）；2026-10-15 HTTPS 下偶发复现，工作区改为静默 200ms、再 1s 各补一条。两次补注释只是时序缓解，已随 EventSource 迁移删除；“首条与批尾同包”没有现场证据。

## 2026-10-15：根治研究

### 证据与判断

项目使用 `web/src/api/sse.ts` 的 `fetch → TextDecoderStream → reader.read()`，不是原生 EventSource。收到帧后同步调用状态更新；会话流与开发者日志流共用这条路径。服务端用 Axum Event 序列化，现有代码未见漏掉 SSE 空行的证据。

WebKit [缺陷 322545](https://bugs.webkit.org/show_bug.cgi?id=322545) 报告了高度吻合的 fetch 流读取回归：消费者忙时，数据暂存后没有在下一次 pull 时交付，直到新网络数据到来。报告中的 EventSource 对照不受影响，identity 编码也能复现。因此不应将现象泛化为所有 Safari SSE 都会缓冲，或直接归因压缩。此为上游证据，尚非本项目现场确诊。

[上游修复 718031552ba1](https://github.com/WebKit/WebKit/commit/718031552ba1) 已在 2026-08-27 合入：pull 时经 feedStream 交付暂存数据，并新增回归测试。主干修复不等于已进入用户安装的 Safari；仍需核对 Safari/系统版本及实际测试。

15 秒与本项目心跳吻合，支持“后续数据唤醒”的推断，但单靠延迟不能定位层级。当前两次注释是在静默后约 0.2 秒和 1.2 秒发出；新业务事件会重置这组计时。没有协议保证这些写入分别成为网络包，更没有保证第二次到来时 reader 正在拉取。即便分包，也不能承诺消除此缺陷。

Caddy [reverse_proxy 文档](https://caddyserver.com/docs/caddyfile/directives/reverse_proxy#streaming) 明确对 `text/event-stream` 即时刷新；盲加 `flush_interval -1` 不是根治证明。[encode 文档](https://caddyserver.com/docs/caddyfile/directives/encode) 描述了当前版本的流式行为，但现场版本、配置和响应 Content-Encoding 未知，不能据此排除实际代理问题。

### 2026-10-15 真机复现

本地 HTTP 直连（不经 Caddy），服务端连发三段后保持连接、8 秒不再发字节，读到首段后卡主线程 1 秒：

| 读取方式 | Safari 27.2（macOS 27.2） | Chrome |
|---|---|---|
| fetch + TextDecoderStream，不暂停 | 正常 | 正常 |
| fetch 原始字节，卡 1 秒 | 复现：第 3 段与 8 秒后的下一段同时读到 | 正常 |
| fetch + TextDecoderStream，卡 1 秒 | 复现 | 正常 |
| 原生 EventSource，卡 1 秒 | 正常 | 正常 |

结论：当前最新 Safari 仍有该缺陷，无需代理即可触发；原生 EventSource 不受影响。等浏览器修复不可行，应用侧迁移 EventSource 有依据。Playwright 自带 WebKit 26.0 不复现，不能代替真机验收。复现页（另含 await 暂停用例、同时到达判定、后台与超时判无效）在会话草稿目录，迁移验收时沿用其方法。

### 候选与取舍

| 方案 | 能解决什么 | 代价与边界 |
|---|---|---|
| 使用含上游修复的 Safari | 修复 fetch 的底层缺陷，应用可保留现有协议 | 最小改动优先验证；需要确认实际版本，旧设备仍可能受影响 |
| 原生 EventSource | 绕开本次 fetch ReadableStream 缺陷，继续使用 SSE | 应用侧首选候选；原生接口不能自定义 Authorization 头，需设计同源 Cookie 鉴权；错误响应和重连也需重新约定 |
| XHR 增量读取 | 可保留 Bearer，避开 fetch 读取路径 | 仅为待验证候选；长连接 responseText 累积内存，需轮换连接，增加生命周期复杂度 |
| WebSocket | 避开 HTTP 响应体流读取路径 | 改动更大，需定义连接鉴权、恢复、错误与关闭语义；当前单向推送不足以证明值得迁移 |
| 提前 read、去掉 TextDecoderStream、Worker、增加注释或更短心跳 | 可能降低触发概率 | 未证明能消除底层缺陷；不能以短期不复现作为根治验收 |
| 首屏历史改普通 JSON，随后订阅增量 | 首屏历史不再依赖长流 | 实时最终 Reply/run_finished 仍可能滞留；还要处理快照与订阅交接，不作为单独根治方案 |

建议先用最小复现核对浏览器修复。如果需要兼容受影响 Safari，再选择原生 EventSource，并按 B2 写清公开契约后实施。不要把长期 Bearer token 放进 URL，也不要用仍基于 fetch 的 EventSource polyfill 当作绕开方案。

EventSource 的 B2 必须覆盖：Cookie 发放、作用域、有效期、退出及安全属性；哪些接口接受 Cookie，以及相应跨站请求防护；命名事件的注册；401/404/协议错误如何继续 fail fast（原生 error 不直接暴露 HTTP 状态）；现有 after 游标和原生 Last-Event-ID 的关系；只保留一个重连调度者；会话流与日志流一起迁移。标准接口与重连行为见 [HTML EventSource 规范](https://html.spec.whatwg.org/multipage/server-sent-events.html#the-eventsource-interface)。这些尚未定案，不在本次研究中更改鉴权或传输。

### 可证伪的验证与关闭条件

1. 记录受影响 Safari/系统版本、Caddy 版本、HTTP 协议、Content-Type/Content-Encoding；测试保持前台可见。
2. 参考上游修复中的回归测试：连续写三段数据，中间短间隔；客户端读完首段后刻意停读，再恢复读取；服务端此时保持连接打开且不再发送。验证余下数据能在期限内交付，而不是靠下一次写入或 EOF 放出。增加与业务相同的 TextDecoderStream 和解析/状态更新路径。
3. 对照直连与 HTTPS/Caddy、fetch 与原生 EventSource、受影响 Safari 与含修复版本。诊断端点应隔离于生产 API，不为实验放开现有业务鉴权。
4. 项目现场按同一事件 id 分层记录：服务端入队/响应流 yield、客户端原始字节读取、帧解析、状态应用、DOM 内容与滚动位置。入队或 yield 时间都不等于到达客户端时间；必要时结合代理出口观测。
5. 网络面板显示时间只能辅助定位：消息显示晚可能是浏览器交付/面板处理晚，不能直接判定服务端或 Caddy 发晚。原始字节早、帧晚查分帧；帧早、DOM 晚查应用；DOM 已有但不可见查滚动。
6. 最终方案必须在不补空注释、不靠短心跳的实验中及时交付最后一条，覆盖大批历史、最终回复、run_finished、日志流、重连去重、鉴权失效和长时间连接。正常保活心跳可以保留，但不能充当业务数据正确性的条件。

（研究阶段）当时只核对代码与上游资料，真机复现见「2026-10-15 真机复现」一节；现有 Rust 未提交改动保留。此前 fmt/clippy 通过仅说明静态检查通过，不证明该时序问题修复。

以下为最初排查记录。

## 已观察现象

- 网页执行「检查并更新」、服务重启后，部分会话只显示前面的工具调用，后面的消息内容不可见。
- 刷新页面无法恢复；发送一条新消息后，中间不可见的旧内容又会出现。
- 访问方式为服务器 IP:端口直连，不能据此归因于反向代理缓冲。

## 已核对与未知项

- 历史回放从 `Store::messages_after` 读取会话全部稳定消息，没有按工具调用或条数截断。
- 历史与实时消息共用 `/api/sessions/{id}/stream` SSE；前端将稳定消息追加到 `SessionView.messages`。
- 对 SSE 解析器进行 153 个字节分包位置的临时检查，中文、emoji、CRLF 和尾部消息均完整；尚未复现用户现场。
- 内容随后能恢复，提示应核对接收、渲染和滚动位置；目前没有证明消息存储丢失，也没有确认根因。
- 待采集：浏览器、受影响会话、缺失时 SSE 是否已收到尾部 `message`/`ready`、控制台异常，以及正文是 DOM 中缺失还是仅不可见。

下一步先复现和采样，再决定修复位置；不以未验证的代理缓冲假设修改代码。
