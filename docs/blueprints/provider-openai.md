# B2: OpenAI 协议族模型实现（mic-provider-openai）

**状态**: CLOSED（2026-09-23 首版）；2026-10-10 按 [`provider-sdk-responses.md`](provider-sdk-responses.md) 整体改为 async-openai 实现并接入 Responses
**来源**: [`v0a-module-map.md`](v0a-module-map.md) M7；[`provider-port.md`](provider-port.md)（实现的契约）；配置分层与工厂契约见 [`model-settings.md`](model-settings.md) §四
**依赖不变量**: 依赖 `mic-core`（port）+ `mic-message` + `mic-store`（`Usage`）+ `mic-tool`（`ToolSpec`）；不被任何 crate 依赖，只由二进制装配。

本文只写现行契约；修订过程见 git 历史。

## 一、用户视角的效果

- Web 设置页新建服务商，选预设：OpenAI、DeepSeek、Ollama、通用兼容服务。OpenAI / DeepSeek 地址固定不用填；Ollama 留空取本机；通用必须填地址。
- key 可在 Web 填（加密入库），不填则读环境变量（OpenAI/通用 `OPENAI_API_KEY`，DeepSeek `DEEPSEEK_API_KEY`，Ollama 不需要）。
- 回复流式显示；DeepSeek / Ollama 的思考过程、OpenAI 的推理摘要作为推理单独显示。
- key 无效、余额不足直接说明原因；限流、服务端错误、断流、长时间无数据、空回复归为"暂时不可用"，由 core 决定是否重试。
- 每次调用的用量（输入、输出、缓存读写、推理）入库，供统计。

## 二、公开接口

crate 只导出 `OpenAiModule`（`name() = "openai"`，`Activation::Always`），`install` 登记 `openai` 工厂。
Provider 与工厂类型不公开；工厂方法（`check_endpoint` / `check_model` / `key_env` / `build` / `list_models`）语义见 model-settings §四。

### 2.1 配置（JSON，`deny_unknown_fields`）

服务商 `{preset, base_url?}`：

| 预设 | 协议 | 地址 | key 环境变量 |
|---|---|---|---|
| `openai` | Responses | 固定 `https://api.openai.com/v1`，不接受 `base_url` | `OPENAI_API_KEY` |
| `deepseek` | Chat（DeepSeek 方言） | 固定 `https://api.deepseek.com`，不接受 `base_url` | `DEEPSEEK_API_KEY` |
| `ollama` | Chat（Ollama 方言） | 缺省 `http://localhost:11434/v1` | 无 |
| `generic` | Chat（通用） | 必填 | `OPENAI_API_KEY` |

`base_url` 只接受 http(s)，不得含账号密码、查询参数或锚点，末尾 `/` 去掉。有 key 时发 `Authorization: Bearer`（敏感头），无 key 不发。

模型 `{max_tokens?, reasoning_effort?}`，模型名单列：

| 预设 | `reasoning_effort` |
|---|---|
| `openai` | `none|minimal|low|medium|high|xhigh|max` 或不写（不写不发） |
| `deepseek` | `none|low|high|max`，不写取 `low` |
| 其它 | 只能不写 |

### 2.2 构建期旋钮（`src/limits.rs`）

| 常量 | 值 | 含义 |
|---|---|---|
| `CONNECT_TIMEOUT` | 30 s | 建连超时 |
| `STREAM_IDLE_TIMEOUT` | 300 s | 两次收到数据之间的最长间隔（含等待首字节） |
| `PROBE_TIMEOUT` | 15 s | 测试连接整体超时 |

## 三、传输

- async-openai 自定义 `Config`：地址与请求头全部来自已解析配置，不读任何环境变量。
- 自写无重试 HTTP service：重试只在 core。非 2xx 时自己读体，解析 `error.{code,message}` 与秒数形式的 `Retry-After`。
- 请求与分片用 BYOT 自定义类型，标准子结构 `flatten` 复用 SDK 类型；服务商扩展字段完整接收。
- 两协议共用一个流驱动：每次等待都受空闲超时约束；丢弃流即取消（SDK 读流任务随接收端关闭退出）。
- SDK 解析失败时会把上游原文打进 `async_openai` 日志，二进制全局丢弃该 target。

## 四、映射规则

### 4.1 请求公共部分

历史逐条取 `Message::model_view()`（呈现规则不在本 crate），`None` 跳过。

- user：文本片段按行拼接；图片转 data URL。
- 工具调用参数：`args` 为 `Value::String` 时原样发回（模型当初的原文，见 4.4），否则发 JSON 文本。
- 视图里有本协议不支持的 `File` 片段 → `Rejected`。
- `stream: true`；有 `max_tokens` 才发。

### 4.2 Chat（DeepSeek / Ollama / 通用）

- `system` 非空 → 第一条 `role: system`；工具 → `tools[{type: function, ...}]`。
- assistant 一条：正文拼成 `content`，工具调用 → `tool_calls`；tool 结果 → `role: tool`。
- `stream_options.include_usage: true`；`reasoning_effort` 有值时发顶层字段（`none` 即关闭思考）。
- 方言：

| | DeepSeek | Ollama | 通用 |
|---|---|---|---|
| 推理来源 | delta `reasoning_content` | delta `reasoning` | 不解析 |
| 推理回传 | 仅同模型、含工具调用、推理非空的轮次发 `reasoning_content` | 不回传 | 不回传 |
| `cache_read_tokens` | `prompt_cache_hit_tokens` | None | `prompt_tokens_details.cached_tokens` |

- 结束：SDK 吞掉 `[DONE]`，完成判据为「正常 EOF + 已收到 `finish_reason`」。
  `stop` → `EndTurn`，`tool_calls` → `ToolUse`，`length` → `MaxTokens`，`content_filter` → `ContentFilter`；
  `insufficient_system_resource` → `Transient`；legacy `function_call` → `Protocol`；EOF 时缺 `finish_reason` → `Transient`。
- 只接受单候选；`finish_reason` 之后再有内容增量 → `Protocol`。块顺序：推理、正文、工具调用。

### 4.3 Responses（OpenAI）

- 无状态：`store=false`、`include=[reasoning.encrypted_content]`；有 effort 时 `reasoning={effort, summary: auto}`；`instructions` = system；工具 `strict=false`；`max_tokens` → `max_output_tokens`。
- assistant 块按原顺序回传：正文 → assistant 消息（带 `phase`），工具调用 → `function_call`，工具结果 → `function_call_output`。
- 推理回传：仅同模型、且有密文的推理，作为不带 id 的 reasoning item 发回（SDK 类型会把 id 序列化为 null，所以请求体自定义）。
- 流中：正文与 refusal 增量 → `TextDelta`，推理摘要增量 → `ReasoningDelta`。
- 终态 `response.output` 是唯一内容来源：
  - message → `Text { phase }`；refusal 转为可见正文，并记为 `ContentFilter`。
  - function_call → `ToolCall`，id 取 `call_id`。
  - reasoning → 有摘要：`Visible { 摘要段落以空行拼接, signature: 密文 }`；只有密文：`Redacted { data }`。
  - 其它输出项（内置工具等）→ `Protocol`。
- 终态事件：completed → 按内容定 `EndTurn` / `ToolUse`；incomplete 为 `max_output_tokens` → `MaxTokens`、`content_filter` → `ContentFilter`，其它原因 → `Protocol`；failed / error → 按 code 分类（4.5）。EOF 前没有终态 → `Transient`。

### 4.4 公共响应规则

- 工具参数解析为 JSON **对象**才存 `Value::Object`；否则原文存 `Value::String`，由 `mic-tool` 边界报参数错误给模型自纠，回传时逐字还原。
- `EndTurn` 但没有任何内容块 → `Transient`（"上游返回了空回复"）。
- 用量在终态一次转换；上游没给的项为 `None`，整个 usage 缺失则 `ModelResponse.usage = None`，不编造 0：

| 领域字段 | Chat | Responses |
|---|---|---|
| input_tokens | prompt_tokens | input_tokens |
| output_tokens | completion_tokens | output_tokens |
| cache_read_tokens | 见方言表 | input_tokens_details.cached_tokens |
| cache_write_tokens | None | input_tokens_details.cache_write_tokens（负数 → Protocol） |
| reasoning_tokens | completion_tokens_details.reasoning_tokens | output_tokens_details.reasoning_tokens |

### 4.5 失败

| 情况 | 分类 |
|---|---|
| code `insufficient_quota` / `invalid_api_key`；无已知 code 时 HTTP 401～403 | `Account`（402 文案"余额或额度不足，请充值"） |
| code `rate_limit_exceeded` / `server_error`；无已知 code 时 429、5xx | `Transient`，带 `Retry-After` |
| 其它 4xx、其它上游业务错误 | `Rejected` |
| 网络错误、建连超时、流空闲超时、流中途断开、缺终态 | `Transient` |
| 分片无法解析、未知语义 | `Protocol`（不带上游原文） |

文案为"中文说明（HTTP 状态，code）：上游 `error.message`"，只用状态、结构化 code 与 message，不拼原始响应体，不含 key。

测试连接（`GET {base}/models`，解析 `data[].id`）：401/403 → `Auth`；超时、建连失败 → `Network`；其它状态或无法解析 → `Unexpected`。

## 五、副作用与依赖

- 每次 `stream` 发一个 HTTPS 请求，丢弃流即中止；无落盘。
- 外部依赖：`async-openai`（`default-features=false`；`rustls`/`chat-completion`/`responses`/`byot`/`middleware`/`model`，不锁小版本）、`reqwest`、`tower`、`secrecy`、`base64`、`futures-util`、`serde`、`serde_json`、`tokio`。

## 六、模块

| 模块 | 职责 |
|---|---|
| `factory` | `ProviderFactory` 实现；按协议驱动对应请求与累积 |
| `config` | 配置解析、规范化、`Resolved` |
| `client` | SDK `Config`、无重试 service、`HttpFailure` |
| `error` | SDK / HTTP / 流内错误 → `ProviderError` / `ProbeError` |
| `stream` | 共用流驱动：打开、逐事件、空闲超时、EOF、空回复判定 |
| `content` | 两协议共用的内容转换：文本拼接、图片 data URL、工具参数 |
| `chat` / `responses` | 各自的请求映射、BYOT 类型、增量累积与终态转换 |
| `probe` | `/models` 探测 |
| `limits` | 构建期旋钮 |

## 七、已知演进

- ChatGPT 订阅 Token：待 human 实测后另行增加预设，复用 Responses 路径（provider-sdk-responses §七）。
- Responses 出现新流事件导致解析失败时升级 SDK，不加兜底。
