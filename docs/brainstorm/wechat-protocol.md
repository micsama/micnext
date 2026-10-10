# 微信接入：iLink Bot 协议事实

**状态**：协议事实参考。取自官方 npm 包 `@tencent-weixin/openclaw-weixin@2.4.9` 的源码
（OpenClaw 框架的 Channel 插件）及接入实测；只记协议事实，契约见 [微信 B2](../blueprints/wechat-channel.md)。
插件里 `channel.ts`、`process-message.ts` 等是对接 OpenClaw 的胶水，不在学习范围。

## 一、已定方向（2026-10-08 与 human 确认）

| # | 决定 |
|---|---|
| 1 | 凭据一律从 Web 设置页扫码登录获得，写入 SQLite；不手填，不进 `config.toml` |
| 2 | 无流式。**每次模型调用的输出里带文本，就发一条微信消息**（与落盘的 `Reply` 对齐，见 §五） |
| 3 | 要做 typing 指示（续发与取消时机见 B2 §6.3） |
| 4 | §七 的未验证点不再调研，**遇到时实测并回填本文** |
| 5 | 一个微信对象对应一个扫码用户和会话；同号续会话，用户身份归属及统一上下文能力见 B2 |

## 二、协议总览

纯 HTTP + JSON，无 websocket。

- API：`https://ilinkai.weixin.qq.com`（登录可能被重定向到其它机房，见 §三）
- CDN：`https://novac2c.cdn.weixin.qq.com/c2c`

**每个请求的头**：`AuthorizationType: ilink_bot_token`、`Authorization: Bearer <bot_token>`、
`X-WECHAT-UIN`（随机 u32 → 十进制字符串 → base64）、`iLink-App-Id`、`iLink-App-ClientVersion`
（`major<<16 | minor<<8 | patch`）、可选 `SKRouteTag`。POST body 带 `base_info: { channel_version, bot_agent }`，
仅用于可观测。GET 登录接口只带 `iLink-App-*` 与 `SKRouteTag`，不带 Authorization。

| 端点 | 作用 |
|---|---|
| `POST ilink/bot/get_bot_qrcode?bot_type=3` | 取登录二维码，body `{ local_token_list }` |
| `GET ilink/bot/get_qrcode_status?qrcode=…[&verify_code=…]` | 长轮询扫码状态 |
| `POST ilink/bot/getupdates` | 长轮询收消息 |
| `POST ilink/bot/sendmessage` | 发消息 |
| `POST ilink/bot/getconfig` | 取 `typing_ticket` |
| `POST ilink/bot/sendtyping` | 输入指示 |
| `POST ilink/bot/getuploadurl` | 取媒体上传地址 |
| `POST ilink/bot/msg/notifystart` / `notifystop` | 客户端启停通知 |

响应可含 `ret`（0 成功）、`errcode`、`errmsg`；缺失视为成功（实测 `getupdates` 成功响应不带 `ret`），`-14` 表示 token 失效。
`message_id` 是 uint64，Rust 侧按十进制字符串或整数接收，不走 f64；item `msg_id`、ref `svr_id` 实测为不透明字符串（如 `v1:…`）。
实测请求头取值：`iLink-App-Id: bot`，`bot_type=3`，ClientVersion 按 2.4.9 编码。

## 三、登录（扫码）

1. `get_bot_qrcode` → `{ qrcode, qrcode_img_content }`；后者是二维码内容（链接），Web 端自行渲染。
2. 循环 `get_qrcode_status`（客户端超时 35 秒，超时和网关错误一律当 `wait` 继续）。二维码约 5 分钟过期，可重取，有次数上限。
3. 状态：

| 状态 | 含义 |
|---|---|
| `wait` / `scaned` | 继续轮询 |
| `scaned_but_redirect` | 带 `redirect_host`，此后轮询改到 `https://<redirect_host>` |
| `need_verifycode` | 需用户输入配对码，之后每次轮询带 `verify_code` |
| `verify_code_blocked` | 验证码被封，需重取二维码 |
| `binded_redirect` | 该 bot 已绑定，不发新凭据，视为已完成 |
| `expired` | 过期，重取 |
| `confirmed` | 返回 `bot_token`、`ilink_bot_id`（账号 ID）、`baseurl`、`ilink_user_id`（扫码人） |

**落库字段**：`bot_token`、`ilink_bot_id`、`baseurl`、`ilink_user_id`（作 owner 白名单）。Web 设置页需要：
二维码展示、验证码输入框、过期重取、成功/失败回显、重新登录入口。

## 四、收消息

- 请求 `{ get_updates_buf, base_info }`，首次传 `""`。
- 响应 `{ ret, errcode, errmsg, msgs[], get_updates_buf, longpolling_timeout_ms }`。
- `get_updates_buf` 是服务端给的**不透明游标**，整串持久化，下次原样带回；重启后凭它续接。
- 默认长轮询超时 35 秒，服务端可经 `longpolling_timeout_ms` 调整；客户端超时当空响应重试。
- 错误：`-14` → token 失效，插件暂停该账号 1 小时（我们改为置「需重新登录」并在设置页提示）；
  其它错误连续 3 次后退避 30 秒，否则 2 秒重试。

**`WeixinMessage` 字段**：`seq`、`message_id`、`from_user_id`、`to_user_id`、`client_id`、`session_id`、`group_id`、
`message_type`（1 用户 / 2 bot）、`message_state`（0 新 / 1 生成中 / 2 完成）、`item_list[]`、`context_token`、
`run_id`、`create/update/delete_time_ms`。

实测另有 SDK 未声明的 `root_id`/`parent_id`（数字）。

**`MessageItem`**：`type`（1 文本 / 2 图片 / 3 语音 / 4 文件 / 5 视频 / 11 工具调用开始 / 12 工具调用结果）、
`msg_id`、`ref_msg`（引用：`message_item`、`title` 摘要、`svr_id`、`partial_text`）、各类 `*_item`。
实测另有 `button_item_list`（仅见空数组）、`at_bot_username_list`。无表情包类型：默认表情以 `[发呆]` 这类文本到达。
语音带 `encode_type`（6 = silk）、`playtime`、可能的转写 `text`。上游字段按「暂无消费者也完整接收」建模。

**引用**：`ref_msg.message_item.msg_id` 是被引消息的顶层 `message_id`（十进制，用户消息与 bot 发送段同此）；
`partial_text { start, end, startindex, endindex, quotemd5 }` 描述部分选区，MD5 按微信显示文字计算。样本见 [V2 B1](wechat-v2.md) §三。

**过滤**：只处理 `from_user_id == 扫码人` 且无 `group_id` 的消息；其余忽略。

## 五、发消息、context_token 与 typing

`sendmessage` body `{ msg }`：`from_user_id: ""`、`to_user_id`、`client_id`（客户端生成）、`message_type: 2`、
`message_state: 2`、`item_list: [单个 item]`、`context_token`、`run_id`。响应 `{ ret, errmsg, message_id }`。

- `context_token` 随每条入站消息下发，发送时原样回传；插件按「账号:用户」持久化并在重启后恢复。
  **我们同样存 SQLite**（每条入站覆盖更新）。
- 一次只放一个 item；文字加图片拆成多条。
- 无送达回执：「发送完成」只能是 `ret == 0` 且带 `message_id`。
- 插件声明单条文本上限 `textChunkLimit: 4000`，超长自行分段。
- 微信不渲染 Markdown，插件内有 373 行的流式 Markdown 过滤器；转换策略我们自己定。

**对应到我们的「每次模型调用出话就发一条」**：

- 一次模型调用的文本落为一条 `Reply`；`Reply` 的文本块非空就发一条微信消息；只有工具调用没文本则不发。
- 投递复用 `pending_deliveries(channel)` / `delivered_at`；B2 已选择失败仅重试一次，重启不自动补发，
  失败跳过另记事实，不冒充 delivered。超时后重试仍可能重复发送，不承诺恰好一次。
- 其余可见内容（`Notification`，如失败、中断提示）同走这条路径。

**typing**：`getconfig`（`ilink_user_id` + `context_token`）→ `typing_ticket`；
`sendtyping { ilink_user_id, typing_ticket, status }`，1 = 输入中，2 = 取消。插件按用户缓存 ticket（24 小时内随机刷新，失败指数退避）。
SDK 每 5 秒续发一次 1。我们按此续发，回复送达后才发 2（详见 B2 §6.3）。

## 六、媒体

**上传**：随机 16 字节 AES key 与 `filekey`；`getuploadurl { filekey, media_type(1 图/2 视频/3 文件/4 语音), to_user_id, rawsize, rawfilemd5, filesize(密文大小), no_need_thumb, aeskey(hex) }`
→ `upload_full_url`（或旧式 `upload_param`）；AES-128-ECB(PKCS7) 密文 `POST` 到该地址，最多重试 3 次，4xx 不重试；
响应头 `x-encrypted-param` 即下载参数。再发 `image_item.media = { encrypt_query_param, aes_key(base64), encrypt_type: 1 }`
（文件另带 `file_name`、`len`；视频带 `video_size`）。

**下载**：`media.full_url`，没有则 `<cdn>/download?encrypted_query_param=…`；AES-128-ECB(PKCS7) 解密，无 key 则为明文。
key 有两种形态：base64 解出 16 字节；或解出 32 字符 hex 串再转字节。入站图片另有 `aeskey` hex 字段，优先用。
入站图片已按此实现并服务器验收（[入站图片 B2](../blueprints/wechat-inbound-images.md)）。语音只取服务端 `voice_item.text` 转写，不转码 silk。

## 七、待实测（遇到再回填）

| # | 问题 | 结果 |
|---|---|---|
| 1 | 不带 `context_token` 的主动推送能否送达；`context_token` 有无时间窗口 | |
| 2 | 服务端是否按 `client_id` 去重 | |
| 3 | `iLink-App-Id` 的取值；`bot_type=3` 的含义；自己的实现用什么值 | 用 SDK 值 `bot` / `3`，服务器登录可用；含义未知 |
| 4 | typing 在长 run 中是否自动过期，是否需周期续发 | 按 SDK 每 5 秒续发，服务器显示正常；不续发是否过期未单独测 |
| 5 | 同一用户连发多条时 `getupdates` 的顺序、批量与重复行为 | |
| 6 | `-14` 之外 token 失效的其它表现；重新扫码后旧 `get_updates_buf` 是否仍有效 | |
| 7 | 单条文本实际上限与超限时的服务端行为 | |
| 8 | `ret != 0` 时 `sendmessage` 的错误码分类（限流、被拉黑、token 失效） | |
| 9 | 新凭据以空游标首次 `getupdates` 是否带回旧历史 | |
| 10 | `binded_redirect` 后重新取二维码能否正常登录 | |

## 八、不学的部分

群聊（`group_id`）、多账号、框架配对流程（`pairing`）、完整引用消息存储（`quote-store`，656 行；我们复用 core 正文与出站计划）、
silk 转码、工具进度 item、Markdown 过滤。`/echo` 等斜杠指令与 `notifystart/stop` 的取舍见 [V2 B1](wechat-v2.md)。
