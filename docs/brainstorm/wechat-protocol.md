# 微信接入：iLink Bot 协议事实与接入方向

**状态**：B1 素材（2026-10-08）。协议事实取自官方 npm 包 `@tencent-weixin/openclaw-weixin@2.4.9` 的源码
（OpenClaw 框架的 Channel 插件）。本文只记协议事实、已定方向和待实测项，不含 B2 契约。
插件里 `channel.ts`、`process-message.ts` 等是对接 OpenClaw 的胶水，不在学习范围。
接入方向以 [`wechat-channel.md`](wechat-channel.md) 的 2026-10-08 human review 决定为准；
下文协议事实保留，自动补发、单对象限制等早期方向已由该文修订。

## 一、已定方向（2026-10-08 与 human 确认）

| # | 决定 |
|---|---|
| 1 | 凭据一律从 Web 设置页扫码登录获得，写入 SQLite；不手填，不进 `config.toml` |
| 2 | 无流式。**每次模型调用的输出里带文本，就发一条微信消息**（与落盘的 `Reply` 对齐，见 §五） |
| 3 | 要做 typing 指示：run 开始发「输入中」，结束取消 |
| 4 | 下面 §六 的未验证点，不再调研，**接入后自己实测并回填本文** |
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

响应统一含 `ret`（0 成功）、`errcode`、`errmsg`；`errcode == -14` 表示 token 失效。
`message_id` / `msg_id` / `svr_id` 是 uint64，Rust 侧按 `u64` 或字符串接收，不走 f64。

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

**`MessageItem`**：`type`（1 文本 / 2 图片 / 3 语音 / 4 文件 / 5 视频 / 11 工具调用开始 / 12 工具调用结果）、
`msg_id`、`ref_msg`（引用：`message_item`、`title` 摘要、`svr_id`、`partial_text`）、各类 `*_item`。
语音带 `encode_type`（6 = silk）、`playtime`、可能的转写 `text`。上游字段按「暂无消费者也完整接收」建模。

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
接法：run 开始发 1，run 结束（`run_finished`，任意终态）发 2；ticket 取不到则静默跳过 typing，不影响收发。
长 run 期间 typing 是否会自动过期、是否要周期续发，待实测（§六）。

## 六、媒体

**上传**：随机 16 字节 AES key 与 `filekey`；`getuploadurl { filekey, media_type(1 图/2 视频/3 文件/4 语音), to_user_id, rawsize, rawfilemd5, filesize(密文大小), no_need_thumb, aeskey(hex) }`
→ `upload_full_url`（或旧式 `upload_param`）；AES-128-ECB(PKCS7) 密文 `POST` 到该地址，最多重试 3 次，4xx 不重试；
响应头 `x-encrypted-param` 即下载参数。再发 `image_item.media = { encrypt_query_param, aes_key(base64), encrypt_type: 1 }`
（文件另带 `file_name`、`len`；视频带 `video_size`）。

**下载**：`media.full_url`，没有则 `<cdn>/download?encrypted_query_param=…`；AES-128-ECB 解密。
key 有两种形态：base64 解出 16 字节；或解出 32 字符 hex 串再转字节。入站图片另有 `aeskey` hex 字段，优先用。

入站图片解密后落阶段二的 SQLite blob 路径。语音先只取服务端 `voice_item.text` 转写，不转码 silk。

## 七、对 micnext 设计的含义

- **Gateway 不是统一中间层。** `mic-gateway` 是 **Web 这个 Channel 自己的前端适配器**（HTTP API + SSE + 嵌入前端），
  不被任何 crate 依赖。统一层是 `mic-core` 的 `Kernel`：入站写入（`resolve_root_session`、`append_user_input`）、
  事件订阅（`subscribe`）、稳定历史、投递记账都在它上面。微信 Channel 与 Web 平级，都只依赖 `mic-core`，
  直接调 `Kernel`；它不需要也不应该经过 Gateway。`mic-store.md` §九、`mic-core-module.md` 里「经 Gateway 入站和发送」
  的措辞是早期不精确的写法，B2 时一并修正。
- **形态**：微信 Channel 是一个 `Module`，登记一个 `Service` 跑 `getupdates` 循环 + 订阅事件发消息 + typing。
  纯 HTTP，`reqwest` 足够，无需外部进程。
- **设置页**：登录/状态/重新登录的接口放在 `mic-gateway`（Web 的设置接口）里，但**调用微信模块提供的登录能力**
  （二维码、轮询、落库）；这里 Gateway 与微信模块之间如何不互相依赖，由 B2 定（候选：登录能力经 `Kernel`/Registry 暴露的 port）。
- **持久状态全进 SQLite**：`bot_token`/账号/`baseurl`/owner、`get_updates_buf`、`context_token`、投递记录。
  `config.toml` 只需一个「是否启用微信」的启动开关或干脆不放，由 B2 定。
- **入站顺序**：先写入消息（`append_user_input`）再保存 `get_updates_buf`，崩溃后可能重放，
  这是早期草图，已由 [B2 提案](../blueprints/wechat-channel.md) §五 替代：先持久化批次现场与新游标，再导入。
  本版不建入站去重表；异常重启待命，不自动重放旧任务。
- 微信无法编辑已发消息，故流式不适用；`message_state=GENERATING` 插件本身也未使用，忽略。

## 八、待实测（接入后回填）

| # | 问题 | 结果 |
|---|---|---|
| 1 | 不带 `context_token` 的主动推送能否送达；`context_token` 有无时间窗口 | |
| 2 | 服务端是否按 `client_id` 去重 | |
| 3 | `iLink-App-Id` 的取值；`bot_type=3` 的含义；自己的实现用什么值 | |
| 4 | typing 在长 run 中是否自动过期，是否需周期续发 | |
| 5 | 同一用户连发多条时 `getupdates` 的顺序、批量与重复行为 | |
| 6 | `-14` 之外 token 失效的其它表现；重新扫码后旧 `get_updates_buf` 是否仍有效 | |
| 7 | 单条文本实际上限与超限时的服务端行为 | |
| 8 | `ret != 0` 时 `sendmessage` 的错误码分类（限流、被拉黑、token 失效） | |

## 九、不学的部分

群聊（`group_id`）、多账号、框架配对流程（`pairing`）、`/echo` 与 debug 命令、完整引用消息存储（`quote-store`，656 行）、
silk 转码、`notifystart/stop`（是否必需待实测，缺省先不接）。
