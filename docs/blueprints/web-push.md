# B2：iPhone 系统通知（Web Push）

状态：实现完成，待验收。

来源：微信主动推送经常被拒（`ret=-2`，见 [推送降级 B1](../brainstorm/push-fallback.md)），human 希望 iPhone 能可靠收到提醒。human 2026-11-11 定：系统通知与右上角通知中心保持一致；服务经 HTTPS + IP 访问，iPhone 上无证书警告。

## 行为

- **通知中心里出现一条，订阅了的设备就弹一条系统通知**。它是通知中心的镜像，不是推送渠道：不进「推送渠道」勾选，不在推送的投递状态里显示，也不影响微信投递。
- **通知内容**：标题为来源会话标题（没有标题时用「micnext」），正文为推送文字，截到约 200 字；带文件的末尾注「附 N 个文件」。点开后进入来源会话；应用已经打开时，聚焦它并跳转过去。
- **开启方式**：设置 →「对话偏好」的推送区域加「本设备系统通知」开关，按设备分别开启。
  - iPhone 需先在 Safari 里「添加到主屏幕」，并从主屏幕打开。没有这样打开时，开关不可用，旁边一行提示怎么做。
  - 打开开关时系统会请求授权。用户拒绝后开关保持关闭，提示去 iOS 设置里允许。
  - 电脑浏览器同样能开，规则一致。
- **失效清理**：推送服务返回 404/410 时，删除该订阅；设备上重新打开开关即可恢复。

## 实现

- **前端**
  - 新增 `manifest.webmanifest`（`display: standalone`，复用现有图标），让「添加到主屏幕」以独立应用打开。
  - 新增 service worker：`push` 事件里显示通知，`notificationclick` 时聚焦或打开 `/s/<session>`。只处理通知，不缓存页面。
  - 开关通过 `PushManager.subscribe` 订阅，把订阅交给后端；关闭时取消订阅并通知后端。
- **后端（mic-gateway）**
  - 在 [web-login](web-login.md) 新增的模块 schema `web` 里加两张表（两份 B2 一起上线，schema 版本只升一次）：
    - `web_push_keys`：VAPID 密钥对，首次使用时生成，私钥标为 `SecretColumn`（诊断 SQL 里不可见）。
    - `web_push_subscriptions`：`endpoint` 为主键，另存 `p256dh`、`auth`、`created_at`。
  - 订阅 kernel 的 `PushCreated` 事件，按事件里的 `push_id` 读出通知中心同一条推送：标题取 `source_session_title`，正文取 `text`，文件数取 `files`，跳转地址为 `/s/<source_session_id>`。然后逐个订阅发送。
    - **最多推一次**：事件只在内存中广播，启动时不回放历史，重启前没发出去的也不补发。所以重启或重连不会重复推送；宕机期间的推送只留在通知中心。
    - 单个请求 10 秒超时，不重试：结果未知时不重放。
    - 失败记一条 warn，带上推送 id、endpoint 域名和原因。
    - 404/410 删除该订阅。
    - 发送放在后台任务里，不阻塞推送本身。
  - HTTP 接口：都需要登录（见 [web-login](web-login.md)），错误响应体同其他接口 `{"error": …}`。
    - `GET /api/web-push/key`：返回 `{ "key": "<VAPID 公钥，base64url>" }`。
    - `PUT /api/web-push/subscriptions`：请求体就是浏览器 `PushSubscription.toJSON()` 的结果，即 `{ "endpoint": string, "keys": { "p256dh": string, "auth": string } }`，未知字段忽略（浏览器会附带 `expirationTime`）。`endpoint` 相同则覆盖。返回 204。
    - `DELETE /api/web-push/subscriptions`：请求体 `{ "endpoint": string }`，返回 204；订阅不存在也返回 204。
  - 边界校验：`endpoint` 必须是 `https://` 加域名的合法 URL，不能是 IP，长度不超过 2048。`p256dh` 解码后必须是 65 字节的 P-256 公钥，`auth` 解码后必须是 16 字节。不合法返回 400。这些检查保证订阅只能指向公网推送服务。登记接口本身要求登录，所以不另设推送服务域名白名单。
- **依赖**：新增 `web-push-native`，纯 Rust 实现 VAPID 签名和 RFC 8291 加密，只构造请求，由现有 reqwest 发送。不选 `web-push` crate，因为它默认带 OpenSSL 和自己的 HTTP 客户端。
- **备份**：设置备份不包含 `web` 模块。订阅属于具体设备，换机后重新打开开关即可。

## 不做

- 已读同步（在网页里读过之后撤回系统通知）、通知分级、按会话免打扰。
- 页面在前台时不弹通知。不做这项，系统通知照常弹出。

## 验收

- 本地：`cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。用模拟推送服务覆盖：载荷能正确解密、404/410 时删除订阅、超时只记一次 warn。
- human：iPhone 添加到主屏幕后打开开关，让 agent `push` 一条，锁屏能收到，点开进入来源会话；关闭开关后不再收到。
