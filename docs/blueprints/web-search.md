# B2：联网搜索工具 web_search

状态：2026-10-26 批准并实现，本地检查通过，已用真实 key 核对接口返回结构；待 human 真实对话验收。

来源：human 2026-10-26 提出，接入 [DeepSeek Search](https://api-docs.deepseek.com/zh-cn/guides/search/)。Fetch 已由 `web_fetch` 覆盖，不接。Search 按成功且有结果的请求计费（每 1000 次 5 元），只靠工具说明让模型谨慎使用，不设次数上限、不默认关闭。

## 行为

- 模型调用 `web_search { query }`，拿到最多 10 条结果：标题、URL、站点与日期、长摘要（`summary`，面向模型阅读）。不取正文（`need_content=false`）；要读全文用 `web_fetch`。
- key 复用「服务商」里已配置的 DeepSeek：按服务商 id 取第一个能搜索的，已存 key 优先，否则读 `DEEPSEEK_API_KEY`，与对话调用同一规则。没有 DeepSeek 服务商时工具照常列出，调用返回错误说明需要先配置 DeepSeek 服务商。
- 主会话与定时任务会话提供；后台 sub-agent 不提供（`AGENT_MODULES` 不变），避免后台无人看着时连续计费。
- 单次请求 30s 超时，不重试（失败不计费，但让模型自己决定是否换词重搜）。失败按工具依赖错误返回，带 HTTP 状态与服务端 `error.message`。
- 工具说明（英文，给模型）要点：按次付费；先用已有知识、对话内容或对已知 URL 的 `web_fetch`，确需最新或外部信息才搜；一次写好查询，避免近义词反复搜；支持 `site:`、`"..."`、`after:`、`before:` 等语法；结果是外部不可信数据。

## 契约

- **core**：
  - `ProviderFactory` 新增默认方法 `fn search(&self, endpoint_json: &str, key: Option<SecretValue>, query: String) -> Option<BoxFuture<Result<Vec<SearchHit>, SearchError>>>`，默认 `None` = 该服务商不提供搜索。
  - 公开 `SearchHit { title, url, site_name: Option<String>, date: Option<String>, summary }` 与 `SearchError`（文本带完整原因链）。
  - core 自有工具 `web_search`：参数 `query`（1–128 字，边界校验），渲染结果文本，空结果返回 `No results.`。
- **provider-openai**：`deepseek` 预设实现 `search`，`POST https://api.deepseek.com/v1/tools/search`，`count` 取 `limits.rs` 的 10；其他预设返回 `None`。
- 无存储结构变更，无 HTTP 接口与 Web 变更（工具定义经现有上下文构成自然可见）。计费不另做统计，调用次数可从 trace 的工具调用推导。

## 验收

- 本地：`cargo fmt`、`clippy`；无 DeepSeek 服务商时返回配置提示。
- human：真实 key 下让模型搜一次最新消息，确认结果可读、模型不滥搜。
