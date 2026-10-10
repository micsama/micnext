# B2: 模型调用统一迁移 async-openai，接入 Responses

**状态**：APPROVED（2026-10-10），步 1～4 已实现，fmt/clippy/Web check 通过，待 §十 实跑验收后 CLOSED。ChatGPT 订阅 Token 待验证后另行补入（§七）。
**方向来源**：human 选择 async-openai 完整替换手写模型调用，以终为始：只要能完成 LLM 接入与用量统计；彻底重构，不留兼容代码与历史路径；SDK 不锁版本。
**替代范围**：[provider-openai](provider-openai.md) 的传输、协议类型与响应映射；[model-settings](model-settings.md) 的服务商预设新增 `openai`；[provider-port](provider-port.md) 不变。

## 一、职责

- micnext：上下文、系统提示、工具执行、重试、取消、历史与用量落盘、Web/Channel 事件。
- async-openai：协议类型、请求发送、SSE 分帧、JSON 解析、流生命周期。
- provider：配置边界、领域消息与 SDK 类型互转、服务商扩展字段、结束语义、失败分类。

## 二、SDK 事实（以 0.42.2 源码为准）

1. 默认 HTTP executor 带重试层；本项目用 `with_http_service` 装自己的无重试 service，只有 core 重试。
2. `OpenAIConfig` 默认读 `OPENAI_API_KEY/ADMIN_KEY/ORG_ID/PROJECT_ID/BASE_URL`，且无 key 也发 `Bearer `、坏字符 panic。本项目自己实现 `Config`，地址与鉴权头全部来自已解析配置，不读环境变量。
3. Chat 原生类型缺 DeepSeek `reasoning_content`、Ollama `reasoning`、DeepSeek `prompt_cache_hit_tokens`、`insufficient_system_resource`。用 BYOT：请求/分片/usage 自定义命名类型，标准子结构 `flatten` 复用 SDK 类型。
4. Chat 流吞掉 `[DONE]`：完成判据改为「SDK 正常 EOF + 已收到 finish_reason」。
5. SDK 对非 2xx 不保留 `Retry-After`，5xx 原文进 `tracing::warn!`。改为：自写 service 在非 2xx 时自己读体、解析错误、返回 `OpenAIError::Boxed(HttpFailure)`，携带 status、Retry-After、结构化 code/message；SDK 的错误读取路径不再执行。
6. 分片反序列化失败时 SDK 以 `tracing::error!` 打印原文；二进制在全局过滤掉 `async_openai` target，对终端与 DeveloperLogs 同时生效。
7. Responses 流事件是封闭枚举，新事件类型会解析失败报 Protocol；处理方式是升级 SDK，不自定义兜底。

## 三、模块

不新增 crate、不新增 kind、不新增跨 crate 依赖。`lib.rs` 只 re-export `OpenAiModule`。

| 模块 | 职责 |
|---|---|
| `factory` | `ProviderFactory` 实现，按预设构造对应协议 Provider |
| `config` | 服务商/模型配置解析、规范化、`Resolved` |
| `client` | SDK `Config`、无重试 service、`HttpFailure` |
| `error` | SDK 错误 → `ProviderError` / `ProbeError` |
| `stream` | 两协议共用的流驱动：打开、逐事件、超时、EOF、空回复判定、drop 即取消 |
| `content` | 两协议共用的内容转换：文本拼接、图片 data URL、工具参数 |
| `chat` | Chat 请求映射、BYOT 类型、增量累积 |
| `responses` | Responses 请求映射（请求体自定义：SDK 的 reasoning input 会把 id 序列化为 null）、事件处理、终态 output 转换 |
| `probe` | `/models` 探测 |
| `limits` | 构建期旋钮 |

依赖：`async-openai`（`default-features=false`，`rustls`/`chat-completion`/`responses`/`byot`/`middleware`/`model`）、`reqwest 0.13`、`tower`、`secrecy`；删除 `eventsource-stream`。

## 四、配置与公开契约

`Provider`/`ProviderFactory`/`ModelEvent`/`StopReason`/`ProviderError` 不变。

**服务商**：kind 仍为 `openai`，预设新增 `openai`（Responses，地址固定 `https://api.openai.com/v1`，不接受 `base_url`，key 回退 `OPENAI_API_KEY`）。预设即协议选择：`deepseek|ollama|generic` → Chat，`openai` → Responses，工厂内以枚举区分，前端只多一个预设选项。

**模型**：形状不变 `{max_tokens, reasoning_effort}`。`max_tokens` 在 Responses 映射为 `max_output_tokens`。`reasoning_effort` 取值扩为 `none|minimal|low|medium|high|xhigh|max`，按预设限定：DeepSeek `none|low|high|max`（缺省 low，沿用）；OpenAI 全部取值或 null（null 不发送）；Ollama/通用不接受。

**消息**：`ReplyBlock::Text` 追加 `phase: Option<AssistantPhase>`（`commentary|final_answer`）。生产者 Responses 输出；消费者 Responses 历史输入（SDK 要求回传）。旧记录缺字段即 None（serde 对 `Option` 的固有行为），无兼容代码。

**推理**：`Reasoning` 不变。Responses reasoning item 有摘要 → `Visible { text: 摘要段落以空行拼接, signature: encrypted_content }`；无摘要只有密文 → `Redacted { data }`。同模型续轮回传为 SDK reasoning input（不带 id）；无密文的 Visible 不回传。

## 五、协议映射

**Chat**：系统提示、user 文本/图片、assistant 文本+工具调用、tool 结果按原规则映射；DeepSeek 同模型工具续轮回传 `reasoning_content`。不发 legacy functions。usage：标准 `cached_tokens`（Generic）/ `prompt_cache_hit_tokens`（DeepSeek）/ Ollama 无；`reasoning_tokens` 取标准字段。

**Responses**：`instructions` + typed input；`store=false`、`stream=true`、`include=[reasoning.encrypted_content]`、`reasoning.summary=auto`（有 effort 时）；工具 `strict=false`。正文/摘要 delta → TextDelta/ReasoningDelta。终态 `response.output` 是唯一内容来源：message → Text（含 phase，refusal 转可见正文并 ContentFilter），function_call → ToolCall（`call_id` 为 id），reasoning → 见 §四；其它输出项 → Protocol。

usage（终态一次转换）：

| 领域字段 | Chat | Responses |
|---|---|---|
| input_tokens | prompt_tokens | input_tokens |
| output_tokens | completion_tokens | output_tokens |
| cache_read_tokens | cached_tokens / prompt_cache_hit_tokens | input_tokens_details.cached_tokens |
| cache_write_tokens | None | input_tokens_details.cache_write_tokens（负数 → Protocol） |
| reasoning_tokens | completion_tokens_details.reasoning_tokens | output_tokens_details.reasoning_tokens |

## 六、终态与错误

Chat：单候选；finish_reason 之后的内容增量 → Protocol；EOF 无 finish_reason → Transient；`insufficient_system_resource` → Transient。
Responses：completed → EndTurn/ToolUse；incomplete `max_output_tokens` → MaxTokens、`content_filter` → ContentFilter、其它 → Protocol；failed / error 事件按 code 分类；无终态 EOF → Transient。收到终态即停止并 drop 流。
空正文的合法终态不再猜测重试。

| 输入 | 领域结果 |
|---|---|
| 401/403/402，code `insufficient_quota`/`invalid_api_key` | Account |
| 429（非额度）、5xx、网络、超时、缺终态，code `rate_limit_exceeded`/`server_error` | Transient（带 Retry-After 秒数） |
| 其它 4xx 与其它上游业务错误 | Rejected |
| 解析失败、未知输出语义 | Protocol |

错误文案只用状态、结构化 code 与上游 `error.message`，不拼原始响应体；删除 `ERROR_BODY_CHARS`。超时沿用 CONNECT 30s / IDLE 300s / PROBE 15s。

## 七、ChatGPT 订阅（待验证）

先由 human 用手动 Access Token 对 `POST /v1/responses`（`store=false, stream=true`）与 `GET /v1/models` 实测，确认权限与目录格式；通过后新增预设 `chatgpt`（凭据为 Token、不读环境变量），复用 Responses 全部路径。验证前不写任何订阅代码。

## 八、实现步骤

直接重写，不走 parallel change（provider 内部私有，公开改动仅 `Text.phase` 一次改完调用方）：
1. `mic-message` 加 `phase`，全部调用方与 Web 解码器同步。
2. 重写 `mic-provider-openai` 为 SDK 实现（Chat + Responses），删除 `provider/request/response/wire` 旧文件；二进制加日志过滤。
3. Web 预设与推理选项按预设取值。
4. 更新 provider-openai / model-settings 文档为最终契约。

## 九、调用方

| 调用方 | 影响 |
|---|---|
| `mic-message` 及 `model_view` | Text 多一字段，视图透传 |
| `mic-core` run/request/recovery | 不变 |
| 微信 `delivery.rs` | 模式匹配加 `..` |
| `mic-gateway` | 配置 JSON 交给工厂解析，不变 |
| Web types/decoder/settings | Text 解码加 phase；预设加 `openai`；推理选项按预设 |
| `bin/micnext` | 全局过滤 `async_openai` 日志 |

## 十、验收

1. DeepSeek：正文/推理/工具往返/缓存统计；Ollama 无 key；Generic 标准文本/图/工具与 usage。
2. Responses（API key）：正文、工具往返、图片、推理摘要与密文续轮、phase 回传；usage 五项入库。
3. 失败：401、429+Retry-After、额度错误、5xx；确认 SDK 不重试。取消：首字节前与流中取消均释放连接。
4. `cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`/`build`。
