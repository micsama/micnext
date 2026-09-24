# B2: OpenAI 兼容模型实现（mic-provider-openai）

**状态**: CLOSED（2026-09-23 批准并实现于 `crates/mic-provider-openai`；§八 1～4 已用 DeepSeek `deepseek-flash` 与本地 ollama 实跑通过，含推理回传的工具往返）
**来源**: [`v0a-module-map.md`](v0a-module-map.md) M7；[`provider-port.md`](provider-port.md)（实现的契约）；
DeepSeek 协议细节参考 `../deepseek-harness/packages/llm/llm-deepseek`
**依赖不变量**: 新 crate `mic-provider-openai`，依赖 `mic-core`（port）+ `mic-message` + `mic-store`
（`Usage`）+ `mic-tool`（`ToolSpec`）；不被任何 crate 依赖，只由二进制装配。

本文只写现行契约；修订过程见 git 历史。

## 一、用户视角的效果

最常见的配置（DeepSeek 官方，只填模型名，key 从环境变量 `DEEPSEEK_API_KEY` 读）：

```toml
[models]
default = "ds"

[models.ds]
kind = "openai"
preset = "deepseek"
model = "deepseek-flash"
```

本地 ollama（不需要 key）：

```toml
[models.local]
kind = "openai"
preset = "ollama"
model = "qwen3:8b"
```

其它 OpenAI 兼容服务：

```toml
[models.other]
kind = "openai"
base_url = "https://example.com/v1"
model = "some-model"
api_key_env = "EXAMPLE_API_KEY"          # 或 api_key = "sk-..."（二选一）
headers = { "X-Title" = "micnext" }      # 可选
```

- key 缺失（没写 `api_key`，环境变量也没设）→ 启动报错并说明两种填法，而不是等第一次对话才失败。
  `ollama` 预设不需要 key，自动填一个占位值。
- 余额不足、key 无效会直接说明原因（如"DeepSeek 余额不足，请充值"）。
- 回复流式显示；DeepSeek 的思考过程作为推理单独显示。
- 连接超时、长时间没有数据、限流、服务端错误都归为"暂时不可用"，由执行主路径决定是否重试。

## 二、范围

本文定：条目字段与预设、请求/响应到 Chat Completions 协议的映射、HTTP 与流错误到四类失败的映射、
超时、方言扩展点。不定：重试（M6）、条目呈现规则（provider-port §三.5）；多模态只留扩展点（§四.5），实现在后续 B2；推理强度按条目配置（§3.1）。

## 三、公开接口

crate 对外只导出一个模块：

```rust
pub struct OpenAiModule;          // Module::name() = "openai"
```

`install` 收到 `{条目名 → 条目}`（mic-core-module §四），对每个条目解析、解出 key、建一个
Provider 实例，`Registry::provider(条目名, 实例)`。Provider 类型不公开。

### 3.1 条目字段

| 字段 | 必填 | 说明 |
|---|---|---|
| `model` | 是 | 发给上游的模型名，原样传递 |
| `preset` | 否 | `"deepseek"` 或 `"ollama"`：给出默认值并选定方言（§四.2），见下表 |
| `base_url` | 无 `preset` 时必填 | 请求发往 `{base_url}/chat/completions`；有 `preset` 时可覆盖 |
| `api_key` | 否 | 字面 key |
| `api_key_env` | 否 | 从该环境变量读 key；与 `api_key` 互斥 |
| `headers` | 否 | 额外请求头（字符串表） |
| `max_tokens` | 否 | 单次输出上限；不写则不发，由上游决定 |
| `reasoning_effort` | 否 | 推理强度 `"none"`（关闭思考）/`"low"`/`"high"`/`"max"`，仅 `preset = "deepseek"` 支持；DeepSeek 不写时取 `"low"` |

| 预设 | `base_url` 缺省 | key 缺省 |
|---|---|---|
| `deepseek` | `https://api.deepseek.com` | 环境变量 `DEEPSEEK_API_KEY` |
| `ollama` | `http://localhost:11434/v1` | 占位值 `ollama`（服务不校验） |
| 无 | 必填 | 必须写 `api_key` 或 `api_key_env` |

未知字段、非 DeepSeek 条目写了 `reasoning_effort`、`api_key` 与 `api_key_env` 同时出现、未知 `preset`、非法请求头、应有 key 却读不到 → `install`
报错（启动失败，文案说明两种填法）。key 在启动时读取一次。

### 3.2 build 后不改的旋钮（`src/limits.rs`）

| 常量 | 值 | 含义 |
|---|---|---|
| `CONNECT_TIMEOUT` | 30 s | 建连超时 |
| `STREAM_IDLE_TIMEOUT` | 300 s | 两次收到数据之间的最长间隔（含等待首字节） |
| `ERROR_BODY_CHARS` | 500 | 错误信息里保留的上游错误体长度 |

## 四、映射规则

### 4.1 请求

- `stream: true`、`stream_options.include_usage: true`；有 `max_tokens` 才发。
- `system` 非空 → 第一条 `role: system`。
- `tools` 非空 → `tools: [{type: "function", function: {name, description, parameters}}]`。
- 历史逐条取 `Message::model_view()` 映射（呈现规则不在本 crate）：
  - `User(parts)` → `role: user`，文本片段按行拼接。
  - 连续的 `Assistant` 视图合成一条 `role: assistant`：`Text` 拼成 `content`，`ToolCall` →
    `tool_calls[{id, type: "function", function: {name, arguments}}]`；`arguments`：`args` 为
    `Value::String` 时原样发回该字符串（即模型当初的原文，见 §四.3），否则发 `args` 的 JSON 文本。
    推理按 §四.2 处理。
  - `Tool { tool_call_id, output }` → `role: tool` + `tool_call_id`，文本片段按行拼接。
- 视图里有 `File` 片段 → `Rejected`（v0a 不支持多模态，§四.5）。

### 4.2 方言

预设决定方言（内部 `enum Dialect { DeepSeek, Ollama, Generic }`），协议差异只在方言处分支。

**DeepSeek**：

- 响应 delta 的 `reasoning_content` → `ReasoningDelta`，最终为 `Reasoning::Visible { signature: None }`。
  首个空串分片不产出事件。
- 推理回传：只对**含工具调用**、且作者 `model` 等于本条目 `model` 的 assistant 轮次，把其推理作为
  `reasoning_content` 发回（思考模式下工具往返必需）；其余推理不发，省 token。
- 用量：`cache_read_tokens` ← `prompt_cache_hit_tokens`。
- 推理强度：条目取值原样发为顶层 `reasoning_effort`（`none` 即关闭思考），不用 `thinking` 字段。

**Ollama**：delta 的 `reasoning` 字段 → `ReasoningDelta`（思考模型）；不回传推理；
`cache_read_tokens` = 0。

**Generic**：不解析、不回传推理（通用服务的推理字段不统一，出现真实需求再加预设）；
`cache_read_tokens` ← `prompt_tokens_details.cached_tokens`。

### 4.3 响应

- SSE 逐行解析为强类型分片（上游字段完整接收）；`data: [DONE]` 结束。
- `content` 增量 → `TextDelta`；`tool_calls` 增量按 `index` 累积 `id`、`name`、`arguments`。
- 结束时组装 `ModelResponse`：`model` 取上游报告值；`content` 依次为推理、正文、工具调用（空的不放）；
  `arguments` 解析为 JSON **对象**才存为 `Value::Object`；解析失败或不是对象（协议要求对象）则原文存为
  `Value::String`，由 `mic-tool` 边界报参数错误给模型（模型输出错误应回给模型自纠，而不是终止整轮）。
  于是存下的 `Value::String` 一定是原文，回传时（§四.1）逐字还原，不产生二次转义。
- `finish_reason`：`stop` → `EndTurn`，`tool_calls` → `ToolUse`，`length` → `MaxTokens`，
  `content_filter` → `ContentFilter`；`insufficient_system_resource` → `Transient`；其它或缺失 → `Protocol`。
- 用量：`input_tokens` ← `prompt_tokens`，`output_tokens` ← `completion_tokens`，
  `reasoning_tokens` ← `completion_tokens_details.reasoning_tokens`，`cache_write_tokens` = 0。
  `usage` 可能附在最后一个分片或单独尾随，统一等到 `[DONE]` 再产出 `Finished`。

### 4.4 失败

| 情况 | 分类 |
|---|---|
| 401、403 | `Account`，文案"key 无效或无权限" |
| 402（DeepSeek 余额不足） | `Account`，文案"余额不足，请充值" |
| 429 | `Transient`，带 `Retry-After` |
| 其它 4xx | `Rejected` |
| 5xx | `Transient`，带 `Retry-After` |
| 建连失败、超时、流空闲超时、未收到 `[DONE]` 就断开 | `Transient` |
| `stop` 结束但没有任何内容 | `Transient` |
| 分片 JSON 不合法、事件顺序错、未知 `finish_reason` | `Protocol` |

`message` 先写上表的中文说明（有的话），再附状态码与上游错误体中的 `error.message`（无则截取原文，
至多 `ERROR_BODY_CHARS` 字），不含 key。

### 4.5 扩展点（推理强度、多模态）

- **推理强度**：已按条目配置实现（§3.1、§4.2，目前仅 DeepSeek）。其它方言要支持时在方言分支加映射；
  若要 Web 按次切换，届时走 M5 `ModelRequest` 加字段。
- **多模态**：`File` 片段已随视图传到本 crate；加能力后在请求映射处把图片转成 `image_url` 片段，
  能力来源（配置声明或探针）随能力 B2 定。

## 五、副作用与依赖

- 每次 `stream` 发一个 HTTPS 请求；丢弃流即中止请求。无落盘。
- 外部依赖：`reqwest`（rustls、stream）、`eventsource-stream`（SSE 分帧）、`futures-util`、`serde`、
  `serde_json`、`tokio`（超时）。
- `bin/micnext` 模块列表加入 `OpenAiModule`，并依赖本 crate。

## 六、调用方

| 调用方 | 用途 | 兼容性 |
|---|---|---|
| `bin/micnext` | 模块列表加 `OpenAiModule` | 新增依赖 |
| `mic-core` 执行主路径（M6） | 经 `Provider` trait 调用，不认识本 crate | — |

## 七、已知演进

- 多模态按 §四.5 的扩展点实现；推理强度扩到其它方言。
- 其它服务的推理字段方言出现真实需求再加预设。

## 八、验收（步 3'）

临时工程装配 `OpenAiModule`，用 `DEEPSEEK_API_KEY` 发一轮流式请求：

1. 纯文本：打印 `TextDelta`/`ReasoningDelta` 增量与最终 `usage`。
2. 带一个工具说明：确认得到 `StopReason::ToolUse` 与解析后的 `args`；把工具结果拼回再发一轮，
   确认推理回传后上游不报错。
3. 错误 key → `Account`；不存在的模型名 → `Rejected`；缺 key 的配置 → 启动报错文案可读。
4. 框架条目：历史里放一条 `HarnessNote`，确认请求体里是带 `[runtime-note]` 头的 user 消息。
