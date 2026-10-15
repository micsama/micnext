# B2：Web 实时流改用原生 EventSource + 一次性连接票据

**状态：已批准并实现（2026-10-15）**，补注释已删。本地已验：票据契约用 curl 逐条验过（缺票、过期外的重复使用、换目标、其它接口带票都 401，`no-store`）；Chromium、WebKit 用 Playwright 跑过会话回放、日志流、服务重启后带 `after` 续传，没有残留连接。待 human 验收：受测 Safari 经 HTTPS/Caddy 复测，再加 iPhone 真机；通过后标 CLOSED。

依据：[排查记录](../brainstorm/web-message-replay.md)。受测 Safari 27.2 上，`fetch` 读流在页面忙过之后会压住已到达的尾段，要等下一批网络数据才放出（与 WebKit 322545 一致）；同条件下原生 `EventSource` 正常，Chrome 都正常。human 选定：原生 EventSource + 一次性票据；其余接口保持 Bearer，不做全站 Cookie，不改 WebSocket。

本文修订 [gateway](gateway.md) 中两条流的鉴权与 [web-ui](web-ui.md) 的接收方式。SSE 事件名、载荷、回放与 `after` 续传语义不变。

## 一、范围

| 流 | 现在 | 改后 |
|---|---|---|
| 会话流 `GET /api/sessions/{id}/stream` | fetch + Bearer 头，前端自己分帧 | EventSource + `?ticket=` |
| 开发者日志流 `GET /api/developer/logs/stream` | 同上 | 同上 |

两条流一起迁移，前端只保留一条接收路径，不按浏览器分支。普通 JSON 接口不动。

## 二、HTTP 契约

### 2.1 取票（新增，Bearer 鉴权）

| 接口 | 成功 | 失败 |
|---|---|---|
| `POST /api/sessions/{id}/stream-ticket` | `200 {"ticket": "<hex>"}` | 401 令牌无效；404 会话不存在 |
| `POST /api/developer/logs/stream-ticket` | 同上 | 401 |

- 票据是 32 字节密码学随机数的十六进制（沿用 `random_hex`），绑定**一条指定的流**：会话流票据只能连该会话，日志流票据只能连日志流。
- 未兑换的票据 **60 秒**过期；兑换即作废，同一张并发兑换只有一个成功。
- 响应带 `Cache-Control: no-store`。

### 2.2 建流（修改，改为票据鉴权）

- `GET /api/sessions/{id}/stream?ticket=<t>[&after=<id>]`
- `GET /api/developer/logs/stream?ticket=<t>`

这两条路由移出 Bearer 中间件，只认票据；其它接口不接受票据，Bearer 中间件也不接受 URL 中的令牌。票据缺失、过期、已用或与目标不符一律 401。会话不存在仍 404（取票时已查过，这里是取票后被删的情况）。

已建立的连接不受票据过期影响。令牌在进程内固定、无撤销，已建连接照旧持续到断开或进程退出，与现在相同。

### 2.3 服务端实现约束

- 单进程，票据表放进程内（`Mutex<HashMap>`），取票时顺带清掉过期项；不引入 JWT 或外部存储。
- 网关不记录请求 URL，票据不进日志。Caddy 若开了访问日志，需由 human 在 Caddyfile 用 `log` 的查询参数过滤删去 `ticket`。
- 删除 `sse::respond` 的补空注释与 `SSE_NUDGES`，只留正常心跳（已随本次实现删除）。

## 三、前端行为

- `api/sse.ts` 的手写分帧删掉，改为：取票 → `new EventSource(url)` → 按事件名注册监听（会话流 `message`、`text_delta`、`reasoning_delta`、`draft_discarded`、`ready`、`run_started`、`run_finished`、`session_title_changed`；日志流 `ready`、`log`）。上层 `session.svelte.ts` 与 `developer-logs.svelte.ts` 的事件处理不变。
- **重连只由应用调度**：EventSource 的 `error` 回调里先同步 `close()`，不用浏览器自带重连（旧票据已作废）；再走现有退避循环，重新取票，会话流仍用 `after = 已应用的最后一条消息 id` 续传，回放去重照旧。不使用 `Last-Event-ID`。
- **错误可见性**：
  - 取票阶段的 401、404 与现在相同：401 让令牌失效，404 显示「会话不存在」，不重试。
  - 建流阶段 EventSource 不暴露状态码，一律按断线处理并重连；连续 **5 次**取票成功但未收到 `ready` 即判为失败，显示「实时连接反复失败」，停止自动重连，需手动重试。收到 `ready` 后计数清零。
  - 事件解析失败仍是 `ProtocolError`，停止并显示。
- 切换会话、组件销毁、令牌失效时关闭连接、清掉重连定时；取票回来时若已取消，不再建流。
- 前端没有基于心跳的活跃检测，迁移后也不新增。

## 四、调用方影响

- 前端：`api/sse.ts` 重写、`api/stream.ts` 改签名；两个状态类的重连循环加「未到 ready 计数」。
- 网关：新增两个取票路由、票据表；两条流的路由挪出鉴权中间件并改为校验票据；完成后删补注释。
- 部署：Caddy 访问日志按需过滤 `ticket`。无数据库变化。

## 五、验收与关闭

1. **最小复现**（会话草稿目录的复现页方法）：受测 Safari 上，主线程阻塞与 `await` 暂停两种情况，服务端静默期间不发任何字节，尾段在恢复后 2 秒内到达；不以「是否与下一批同时到达」作为唯一判据。
2. **真实业务**，经 HTTPS/Caddy、关闭补注释：
   - 长会话首屏回放；
   - 最终回复与 `run_finished`；
   - 日志流；
   - 「检查并更新、服务重启后重新打开会话」。
   - 尾部 DOM 内容与贴底都正确。
3. **恢复与权限**：
   - 断线续传不缺不重；
   - 票据过期、重复使用、换目标均 401；
   - 令牌失效走登录；
   - 切会话后没有残留连接。
4. 本地 Chromium/WebKit 自动跑 2、3 中能自动化的部分；Safari 与 iPhone 真机交 human。iPhone 未回归前不宣称 iOS 已验收。
