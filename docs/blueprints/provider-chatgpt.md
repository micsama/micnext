# B2: ChatGPT 订阅预设（手贴 access_token）

**状态**：APPROVED（2026-10-10），已实现，fmt/clippy/Web check 通过，待 §七 验收后 CLOSED
**方向来源**：human 选定走 Codex 内部后端（方案 A），先只贴 access_token、不做刷新，不够用再换方案。
**修订**：[provider-sdk-responses](provider-sdk-responses.md) §五「终态 `response.output` 是唯一内容来源」改为 §三；现行契约同步到 [provider-openai](provider-openai.md)。

## 一、已验证事实（2026-10-10，human 本机 curl，codex-cli 0.161.0）

1. Codex CLI 的 access_token 调 `api.openai.com/v1` 报缺 `api.model.read` / `api.responses.write` 权限，公开 API 不可用。
2. `POST https://chatgpt.com/backend-api/codex/responses`，带 `Authorization: Bearer`、`chatgpt-account-id`、`OpenAI-Beta: responses=experimental`、`originator: codex_cli_rs` → 200，标准 Responses SSE。请求体与现有 Responses 一致（`store=false`、`stream=true`、`reasoning`、`include`）。
3. 终态 `response.completed` 的 `output` 为 `[]`，完整条目只在 `response.output_item.done` 中；usage 五项齐全。
4. 后端多出的字段（`access_programs`、`tool_usage`、`usage.attribution`、`reasoning.context` 等）SDK 0.42 解析通过，被忽略。
5. `GET /models?client_version=0.161.0` → `{"models":[{"slug": ...}, ...]}`（非标准 `data[].id`）；`client_version=0.1.0` 返回空列表，按版本过滤。
6. access_token 是 JWT，`https://api.openai.com/auth.chatgpt_account_id` 存在；有效期 10 天（`exp - iat`）。
7. `originator` 为 `codex_cli_rs` / `micnext` / 不发，`OpenAI-Beta` 发或不发，四种组合 responses 均 200。

## 二、用户视角

- 新建服务商选「ChatGPT 订阅（Codex）」，地址固定不填；key 框粘贴 `~/.codex/auth.json` 里的 `tokens.access_token`（加密入库，同其它 key）。
- 「测试连接」列出订阅可用模型（如 `gpt-6.1-sol`）；推理强度同 OpenAI；不提供输出上限。
- 最多 10 天需重新复制一次 token。
- token 过期时，本轮直接报「access_token 已过期（N 天前），请在本机运行一次 codex 后重新复制粘贴」，不发请求。
- micnext 不刷新、不持有 refresh_token，因此不会与本机 Codex CLI 互相顶掉登录。

## 三、契约

**预设** `chatgpt`：协议 Responses；地址固定 `https://chatgpt.com/backend-api/codex`，不接受 `base_url`；`key_env` 为 `None`（不读环境变量）。

**凭据**：key 即 access_token。`build` 时在边界解析 JWT 载荷（只读声明，不验签——本地不是信任边界，服务端会验）为强类型：

```rust
struct Claims { exp: u64, #[serde(rename = "https://api.openai.com/auth")] auth: { chatgpt_account_id: String } }
// 解析与过期判断在 headers() 内完成，产出 ChatgptToken { account_id }
```

| 情况 | 结果（`ConfigError`，经 run 呈现为「模型配置有误…请在 设置 → 模型 修改」） |
|---|---|
| 没填 key | 「请粘贴 ~/.codex/auth.json 里的 tokens.access_token」 |
| 不是 JWT / 缺 account_id 或 exp | 「这不是 Codex 的 access_token，请复制 tokens.access_token」 |
| 已过期 | 「access_token 已过期（{N 分钟/小时/天}前），请在本机运行一次 codex 后重新复制粘贴」 |

请求头只发 `Authorization: Bearer`（敏感）与 `chatgpt-account-id`；不发 `originator` / `OpenAI-Beta`，不冒充 Codex CLI（§一.7）。

**模型**：`reasoning_effort` 取值同 `openai`，不写取 `low`（后端缺省为不思考，「默认」须有确定含义）；`max_tokens` 必须为空（后端拒收 `max_output_tokens`），否则 `check_model` 报「ChatGPT 订阅不支持限制输出长度，请留空」。

**测试连接**：`GET {base}/models?client_version={CODEX_CLIENT_VERSION}`，解析 `models[].slug`（其余字段不消费）；`CODEX_CLIENT_VERSION` 放 `limits.rs`（`"0.161.0"`），列表为空时报「没有可用模型，可能需要更新 CODEX_CLIENT_VERSION」。token 的本地解析与过期判断同 `build`，失败映射为 `ProbeError::Unexpected`。

**Responses 内容来源（两预设共用，修订）**：按 `output_index` 收集 `response.output_item.done` 的条目作为唯一内容来源，转换规则不变（message / function_call / reasoning / 其它 → Protocol）；终态事件只提供结束状态、`incomplete_details`、usage 与错误，其 `output` 不读。

**失败**：沿用现有分类。401/403 → `Account`（多为 token 被吊销），文案沿用通用「key 无效…」。

## 四、调用方

| 调用方 | 影响 |
|---|---|
| `mic-core`（run / kernel / provider trait） | 不变：key 已按轮交给 `build`，`key_env = None` 即不读环境变量 |
| `mic-gateway` | 不变 |
| `mic-provider-openai` | `config` 加预设与 token 解析；`responses` 内容来源改为 `output_item.done`；`factory` 按预设加请求头 |
| Web `types.ts` / `endpoint-form.ts` / `EndpointDialog.svelte` | `Preset` 加 `chatgpt`；地址固定；推理选项同 OpenAI；该预设的最大输出输入框禁用（「不支持」）且不下发；key 占位提示 |

不新增 crate、跨 crate 依赖或 core 接口；`base64` 已是依赖。

## 五、风险

- 内部接口，无文档、可能无预告变更；失败时按现有分类报错，不做兜底。
- 条款层面灰色：借用官方客户端签发的凭据访问其内部后端，仅个人自用。
- access_token 有效期决定手贴是否可忍；不够用再起「贴整份 auth.json + 自动刷新」方案（需独立登录份，避免 refresh_token 轮换互相顶掉）。

## 六、待验证（实现前由 human 跑一次，结论回填本节）

1. ~~account_id 与有效期~~：已确认（§一.6）。
2. ~~模型列表~~：已确认（§一.5）。
3. ~~originator~~：已确认（§一.7）。不带 `originator` 时模型列表同样正常（2026-10-10 测试连接列出 `gpt-6.1-sol`）。

## 七、验收

1. 粘贴 token 后正文、推理摘要、工具往返、图片、多轮推理续轮正常；usage 五项入库。
2. 粘贴乱写的字符串 / 过期 token → 对应中文提示，不发请求。
3. OpenAI 预设回归：内容来源修订后正文与工具调用不变。
4. `cargo fmt`、`cargo clippy -- -D warnings`、`bun run check` / `build`。
