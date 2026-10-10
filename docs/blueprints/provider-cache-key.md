# B2: 模型请求携带会话缓存键

**状态**：APPROVED（2026-10-10），已实现，fmt/clippy 通过，待 §六 验收后 CLOSED
**来源**：[provider-chatgpt](provider-chatgpt.md) 验收中发现 ChatGPT 订阅缓存命中恒为 0。

## 一、已验证事实（2026-10-10，human 本机脚本，同一约 3300 token 前缀各连发 3 次）

| 组 | 请求附加 | 命中 |
|---|---|---|
| A | 无 | 3 次均 0 |
| B | `prompt_cache_key` | 3 次均 0 |
| D | `prompt_cache_key` + `session_id` 头（同值） | 第 3 次 3200 |
| E | D + `conversation_id` / `originator` / `OpenAI-Beta` | 第 3 次 3200 |
| F | E + Codex 形态 User-Agent | 第 2、3 次 3200 |

结论：Codex 后端要按 `session_id` 头路由才会缓存；不需要冒充客户端身份（D 与 E 一致，F 早一次属样本噪声）。缓存写入有延迟，第 1～2 次未命中正常。

## 二、用户视角

ChatGPT 订阅在同一会话内连续对话时，统计里出现缓存命中、首字更快。OpenAI 官方预设顺带受益（`prompt_cache_key` 是公开参数）。其它预设无变化。

## 三、契约

**mic-core `ModelRequest`** 新增：

```rust
/// 同一会话稳定、不同会话不同的不透明串，供上游做前缀缓存路由；不含用户内容。
pub cache_key: String,
```

由 `request::build` 填 `format!("micnext-s{}", session.id.0)`（core 最早知道会话）。

**mic-provider-openai**：

| 预设 | 发送 |
|---|---|
| `openai` | 请求体 `prompt_cache_key = cache_key` |
| `chatgpt` | 同上，并按请求加 `session_id: cache_key` 头（SDK `RequestOptionsBuilder::header`） |
| Chat 系 | 不发（DeepSeek 自动缓存；通用服务不保证支持该字段） |

不发 `conversation_id` / `originator` / `OpenAI-Beta` / 伪造 UA，保持 provider-chatgpt §三「不冒充」。

**失败**：无新增失败；`cache_key` 为 ASCII，头值构造不会失败。

## 四、调用方

| 调用方 | 影响 |
|---|---|
| `mic-core` `request::build` / `run.rs` | 传入会话 id 填 `cache_key` |
| `mic-provider-openai` `responses` / `factory` | 按上表发送；`chat` 忽略 |
| 其它 `Provider` 实现 | 无（当前仅 openai） |
| `provider-port.md` / `provider-openai.md` | 同步字段与映射 |

不新增 crate 或跨 crate 依赖。

## 五、隐私

发出的只有本地会话序号，不含内容或身份；上游缓存按账号隔离。

## 六、验收

ChatGPT 订阅同一会话连发 3 轮，`core_model_calls.cache_read_tokens` 第 2～3 轮起大于 0；OpenAI / DeepSeek 回归正常；fmt / clippy 通过。
