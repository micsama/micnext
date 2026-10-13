# B2：模型调用改回手写，移除 async-openai，统一 reqwest 0.13

**状态：已实现，本地验收通过，待 human 验收（2026-10-13）。**

方向（human 已定）：async-openai 当初只为省事引入，体积代价过大；自己实现协议，功能不变。依赖统一用最新稳定版。

## 一、动机（实测，macOS arm64，去符号）

| 版本 | 大小 |
|---|---|
| 迁移前 `a83b515^`（手写 Chat） | 15.0 MiB |
| 迁移 async-openai `a83b515` | 19.8 MiB（+4.8） |
| HEAD `aedc36b` | 21.6 MiB |

体积来源：SDK 0.42 共 3.6 万行、1,325 个可反序列化类型（仅 `responses` 模块就有 426 个）。serde 为每个类型生成访问器代码，泛型实例化又落在调用方，`mic_provider_openai` 本身因此占 1.7 MiB 机器码。另外它要求 reqwest 0.13，与微信、网页抓取使用的 0.12 并存，并带进 aws-lc（与 0.12 的 ring 是两套加密库）。

其余大依赖都是功能必需，本次不动（同次实测，release `.text`，不含常量表）：

| 依赖组 | 大小 | 用途 |
|---|---|---|
| SQLite（C 本体约 1.0 MiB + `libsqlite3_sys` + `rusqlite`） | 约 1.15 MiB | 存储 |
| regex 一族（`regex_automata`、`regex_syntax`、`aho_corasick`、`ignore`、`globset`、`grep_*`） | 约 0.9 MiB | grep/glob 工具、日志过滤 |
| image 一族（`image`、`image_webp`、`zune_jpeg`、`png`、压缩） | 约 0.45 MiB | 图片解码/缩放/编码，仅 jpeg/png/webp |
| `html5ever`、`htmd`、`markup5ever` | 约 0.25 MiB | 网页抓取转 Markdown |

现实现里 SDK 实际只承担 SSE 分帧和协议类型：鉴权配置、无重试传输、非 2xx 解析、DeepSeek/Ollama 扩展字段、Chat 结束判据、Responses 推理输入都已是自写的（见 [provider-sdk-responses](provider-sdk-responses.md) §二）。

## 二、不变的契约

- `Provider` / `ProviderFactory` / `ModelEvent` / `StopReason` / `ProviderError` / `ProbeError`、服务商与模型配置 JSON、Web 设置：全部不变。
- 协议映射、usage 对照、终态与错误分类、超时：沿用 [provider-sdk-responses](provider-sdk-responses.md) §四～§六，以及 [provider-chatgpt](provider-chatgpt.md)、[provider-cache-key](provider-cache-key.md)、[reasoning-selection](reasoning-selection.md) 的现行规则。实现完成后，这些文档里提到 SDK 的表述改为指向本文。
- 唯一有意的行为差异见 §四「未知流事件」。

## 三、依赖变更

| 位置 | 变更 |
|---|---|
| `mic-provider-openai` | 删除 `async-openai`、`tower`、`secrecy`；加回 `eventsource-stream 0.2`（SSE 分帧，迁移前就用它）；reqwest 0.13 开启 `rustls-no-provider`、`json`、`stream` |
| `mic-channel-wechat`、`mic-tool-web-fetch` | reqwest 0.12 → 0.13，特性从 `rustls-tls` 改为 `rustls-no-provider`（微信另加 `json`、`query`） |
| `bin/micnext` | 新增 `rustls`（仅 `ring` 特性）；启动时安装一次进程级加密实现；删除 `async_openai` 日志过滤 |

TLS 选择：0.13 默认的 `rustls` 特性会带 aws-lc（C 库，机器码约 0.7 MiB，另有常量表）；选 `rustls-no-provider` 加 ring（约 0.15 MiB），由装配根在任何客户端建立前调用一次 `rustls::crypto::ring::default_provider().install_default()`。忘了装会在建客户端时直接 panic，属于 fail fast。

证书校验：0.13 改用操作系统的信任库（`rustls-platform-verifier`），不再内置 Mozilla 根证书。Linux 服务器需要安装 `ca-certificates`（Ubuntu 默认已装）。

## 四、模块与实现要点

模块划分沿用现状：`client` 改为 reqwest 直发，其余模块职责不变，只把 SDK 类型换成本地类型。

- **`client`**：持有 `reqwest::Client`、base URL 和已解析的请求头；`post(path, body, extra_headers)` 和 `get(path, query)`。非 2xx 时读取响应体，解析 `{"error":{"code","message"}}`，生成现有的 `HttpFailure`，日志字段不变。
- **协议类型**：放在 `chat` / `responses` 各自模块内（未另起 `wire`，类型只被本协议用），**只定义用到的字段**，未知字段由 serde 忽略。
  - Chat：messages（system/user/assistant/tool，user 支持 text/image_url 分片）、function tools、`stream_options.include_usage`、DeepSeek `reasoning_content` 回传；分片中的 content/tool_calls/`reasoning_content`/`reasoning`/finish_reason，usage 中的标准缓存与推理字段以及 DeepSeek `prompt_cache_hit_tokens`。
  - Responses：输入项 message（含 phase）、function_call、function_call_output、reasoning（不带 id）；function tool；reasoning 参数；流事件和输出项见下。
- **`stream`**：`bytes_stream` → `eventsource-stream` → 按协议解析每个 `data`。Chat 遇到 `[DONE]` 或 EOF 都交给 `eof()`，仍以 finish_reason 为准；空闲超时、drop 即取消、终态后停止，这些都不变。
- **`error`**：输入换成本地的 `HttpFailure` / reqwest 错误 / SSE 错误 / JSON 错误，分类表不变。

**未知流事件（有意变更）**：SDK 的事件枚举是封闭的，上游新增一种事件类型就会报 Protocol 错误。手写版按 `type` 只解析下列事件，其余忽略：`response.output_text.delta`、`response.refusal.delta`、`response.reasoning_summary_text.delta`、`response.output_item.done`、`response.completed`、`response.incomplete`、`response.failed`、`error`。协议里已知但用不到的事件（created、in_progress、各类 `.added` / `.done` 等）静默跳过；真正未知的类型记 warn 日志（只记类型名，不记原文）。会影响结果的地方仍然严格：未知输出项类型和未知 finish_reason 继续报 Protocol 错误。

## 五、调用方影响

| 调用方 | 影响 |
|---|---|
| `mic-core` / `mic-gateway` / Web | 无 |
| `mic-channel-wechat`、`mic-tool-web-fetch` | 只改 Cargo 特性名；代码若有 0.13 不兼容的调用就地修改，行为不变 |
| `bin/micnext` | 安装加密实现、删除日志过滤 |

无新 crate，公开类型和签名不变。

## 六、验收与关闭

本地：
1. `cargo tree -d` 中不再有重复的 reqwest，`cargo tree -i aws-lc-rs` 为空。
2. 用模拟上游覆盖：Chat（DeepSeek 推理与回传、Ollama 推理、Generic 缓存统计、工具往返、图片、`[DONE]` 与无 `[DONE]`、缺少 finish_reason）、Responses（正文、推理摘要与密文续轮、phase、拒答、incomplete、failed/error 事件、未知事件被忽略、未知输出项报错）、非 2xx（401/402/429 + Retry-After/5xx 及 code 分类）、首字节前与流中取消、空闲超时。
3. `./build.sh` 后用 `scripts/size.sh` 记录体积，目标去符号后 ≤ 16.5 MiB（同机同配置对比）。
4. `cargo fmt`、`cargo clippy -- -D warnings`。

Human：DeepSeek、ChatGPT 订阅、OpenAI key 实际对话各一轮；微信收发、网页抓取 HTTPS 正常；服务器上一键更新后同样验证。通过后 CLOSED，todo 条目移到归档。

实测结果（2026-10-13，同机同配置，release 去符号）：

| 项 | 结果 |
|---|---|
| 体积 | HEAD `4d929ba` 21.91 MiB → 17.33 MiB（−4.58）；`.text` 13.9 → 10.7 MiB，其中 `mic_provider_openai` 1.7 MiB → 298 KiB、`reqwest` 716 → 363 KiB、aws-lc 消失 |
| 目标 ≤ 16.5 MiB | **未达，差约 0.8 MiB**；15.0 MiB 基线之后陆续加了 Responses、ChatGPT 订阅、压缩、标题等功能，未逐项拆账。是否接受或另行压缩交 human 决定 |
| 依赖 | 只剩一份 reqwest 0.13.5；`cargo tree -i aws-lc-rs` 为空；async-openai、secrecy 移除（tower 仍经 axum/reqwest 引入） |
| 测试 | `factory/tests.rs` 单连接模拟上游 7 组用例 + 原有 config 测试全过：Chat 方言与终止、Responses Codex 往返与终态、HTTP 失败分类、丢弃流断连、模型列表探测 |
| 收尾 | `cargo fmt`、`cargo clippy --all-targets -- -D warnings` 通过 |

与验收项 2 的差异：建连失败的用例已删除——本机 `http_proxy` 会把对本地关闭端口的请求转成代理 503，结果随环境变化；空闲超时（300 s）未做自动用例。

编译选项（如 `opt-level="s"`）不在本 B2 范围，手写完成后按新体积另行决定。
