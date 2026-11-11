# B1：借用 Gemini / Grok 订阅额度

状态：调研完成，待 human 定方向。

来源：human 2026-11-12 提出。现在 ChatGPT 订阅已经通过 Codex 后端接入，想用同样的方式接 Gemini（Gemini CLI 那种登录方式）和 Grok，借用订阅额度，不按 API 计费。

## 现状：Codex 是怎么接的

- 在 `mic-provider-openai` 里加了一个预设 `Preset::Chatgpt`，地址固定为 `chatgpt.com/backend-api/codex`，走 Responses 协议。请求带 `session_id` 头，上游才会做前缀缓存。
- 凭据是用户手动粘贴的 Codex CLI `access_token`（JWT）。`token.rs` 只读取 JWT 里的声明，不验签；token 过期后提示用户在本机运行一次 codex，再重新粘贴。**micnext 不做 OAuth 登录，也不刷新 token。**

## Gemini：订阅这条路已经关闭

| 时间 | 事实 |
|---|---|
| 2026-03-18 | Google 公告从 3-25 起检测违规用法，点名「在第三方软件里使用 Gemini CLI OAuth」；免费层只剩 Flash。 |
| 2026-05-19 | Google 宣布 6-18 起 Gemini CLI 不再服务免费层和 Google AI Pro / Ultra 消费者账号，这部分用户改用 Antigravity CLI。Gemini CLI 只留给企业 Code Assist 和 API key 用户。 |
| 2026-06 起 | 社区工具（如 Hermes）改用 Antigravity 桌面端内置的 OAuth client 冒名接入。 |
| 现行 | Antigravity 服务条款第 6 节写明：用第三方软件或工具访问服务即违约，并举例「using OpenClaw with Antigravity OAuth」，后果是「may be grounds for suspension or termination of your Antigravity and/or Gemini CLI accounts」。 |
| 2026-11 | 开发者论坛有人问第三方应用有没有官方途径用订阅额度，至今没有 Google 的回复；11 月上旬有人报告账号被封，原因还没核实。 |

结论：Gemini CLI 的订阅入口本身已经停了。剩下的 Antigravity OAuth 在条款里被明确列为违约，有封号风险，而被封的是你自己的 Google 账号。和 Codex 不同，这条路没有官方认可。

**可行替代**：用 AI Studio 的 API key 按量付费。参见 [model-settings](model-settings.md)。可以先用 generic 预设接 `https://generativelanguage.googleapis.com/v1beta/openai` 试一下。风险是：Gemini 3 的思考模型调用工具后，下一轮要原样带回思考签名，而 `ReplyBlock::ToolCall` 没有存签名的字段，连续调用工具可能报 400。这一点还没实测。

## Grok：官方开放，但有门槛

**官方态度**
- xAI 在 2026-05-21 的公告里写明「Use your SuperGrok or X Premium subscription inside OpenCode」，并先后支持了 Hermes、OpenClaw、OpenCode，还说之后会支持更多开源 agent。和 Gemini 相反，这是官方认可的用法。

**协议**（来自第三方实现，xAI 没有公开开发者文档）
- 登录用 OAuth 设备码（RFC 8628），授权服务器是 `auth.x.ai` / `accounts.x.ai`。用户在浏览器里输入验证码后，客户端轮询 `auth.x.ai/oauth2/token` 拿到 token。
- client_id 是 xAI 给这几个合作 agent 用的共享 client，没有开放自助注册。micnext 要用，只能沿用同一个 client_id，算不算授权使用并不明确。
- access_token 有效期约 6 小时，refresh_token 会轮换。
- 推理接口是 `https://api.x.ai/v1`，Hermes 直接用它的 Responses 适配器就能跑通（推理、工具调用、流式、缓存都正常）。也就是说，协议层可以复用我们现有的 Responses 实现。

**风险**
- 有用户用 $30 的 SuperGrok 登录成功，但推理返回 403。xAI 按订阅档位限制 OAuth API，具体是哪些档位没有公开。
  - X Premium：5 月 OpenClaw 用户登录成功，推理 403（「run out of available resources or do not have an active Grok subscription」）。有人猜是 X 账号和 Grok 账号邮箱不一致导致的，没有证实；也没有人确认 X Premium 能用。
  - SuperGrok $30：5 月 Hermes 报告 403；8 月有用户说连续用了几周正常。可能是 xAI 后来放开了，不确定。
  - 最便宜的一档不保证能用；买之前没法验证，只能买月付后马上用 OpenCode 试。
- 2026-10 推出的新 X Subscription 套餐，文档写的是额度仅供「first-party usage」，第三方 OAuth 还能不能用没有确认。**你的订阅是哪一档，决定了这条路能不能走通。**

**照搬 Codex 的做法：借用官方 CLI 的 token**
- 官方 CLI 是 Grok Build（`grok`），开源在 xai-org/grok-build。`grok login` 把凭据写进 `~/.grok/auth.json`，以 `https://auth.x.ai::<client_id>` 为键，字段有 `key`（access token）、`refresh_token`、`expires_at`。CLI 会在过期前约 5 分钟后台刷新。
- wiremux 实测：把这个 token 当作 `XAI_API_KEY` 直接请求 `api.x.ai`，返回 200。所以 micnext 用 generic 预设接 `https://api.x.ai/v1`、把 token 填作 key，不改代码就能先跑起来。
- 问题是有效期。官方文档只说会自动刷新，没写多长；第三方实现记录的是约 6 小时。照 Codex 那样手动粘贴，一天要粘好几次；要自动刷新，就得用 Grok Build 的 client_id 去换 token，等于冒充官方客户端（wiremux 明确不做这一步）。
- 2026-11 没有搜到新 Plus（$8）档用第三方 OAuth 的成功或 403 报告。有定价文章说 Plus 包含 Grok Build 和 Grok API，但没有核实。

## 对 micnext 的改动（如果做 Grok）

和 Codex 最大的区别是 **token 6 小时就过期**，手动粘贴 access_token 的办法行不通，需要新机制：

1. **Web 端登录**：服务商设置里点「登录 xAI」，后端申请设备码，页面显示验证码和链接，后端轮询直到用户授权。服务器本身没有浏览器，设备码方式正好合适。
2. **刷新与持久化**：保存 refresh_token；access_token 快过期或收到 401 时刷新，并原子写回轮换后的新 refresh_token。刷新失败时把服务商标记为「需要重新登录」，不自动重试。这是现有服务商配置里没有的「会自己变化的凭据」，需要明确由谁持有、什么时候写库、多个请求同时触发刷新时怎么串行。
3. **协议**：在 openai crate 里加 `Preset::Grok` 和 `Backend::Xai`，复用 Responses 实现；模型列表照常查 `/models`。

前两项会改公开契约（`ProviderFactory` 要能提供或刷新凭据，服务商还要有登录状态和对应的 HTTP 接口），需要写 B2。顺带也可以把 Codex 改成同样的登录方式，免去手动粘贴。

## 候选

| | 做法 | 收益 | 代价与风险 |
|---|---|---|---|
| A | 只接 Grok 订阅：设备码登录 + 刷新 | 官方认可，额度来自订阅 | 新增凭据刷新机制；client_id 借用；档位可能 403 |
| B | 先手动验证：在本机用 OpenCode 登录 xAI，确认你的档位不会 403，再决定是否做 A | 不写代码就能排除最大的不确定因素 | 多一步人工操作 |
| C | Gemini 走 API key（generic 预设先试） | 合规，不用改代码 | 按量付费，不用订阅额度 |
| D | Gemini 走 Antigravity OAuth | 能用订阅额度 | 违反条款，可能封掉 Google 账号；不建议 |

建议先做 B：用 OpenCode 登录 xAI 发一条消息，确认不会 403 后再写 A 的 B2。Gemini 订阅不接，需要时走 C。
