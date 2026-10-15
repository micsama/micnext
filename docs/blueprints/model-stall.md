# B2：流式无数据视为挂起，不重试

**状态：已实现（2026-10-15），待 human 真实环境验收。**

## 一、问题

2026-10-15 `/compact` 卡在「处理中…」：DeepSeek 思考模式收到 `tool_choice: "none"` 后只回 keep-alive。空闲超时 300 秒后按 `Transient` 重试，最多 3 次，约 15 分钟才失败，期间页面无任何进展提示。触发原因已在 provider 修复（思考时不发该字段），本 B2 处理超时本身。

human 已定：流式调用以「两次收到数据的间隔」判定，长时间没有数据就当上游挂了，直接失败、不重试；不做端到端总超时。取消/打断另行处理（见 todo）。

## 二、契约变更

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| `mic-core` `ProviderError` | 新增 `Stalled { message }`，显示为「模型无响应：{message}」；不重试 | core 重试判定、错误呈现；trace 错误文字随之变化 |
| `mic-provider-openai` | 空闲超时（含等待首字节）由 `Transient` 改报 `Stalled` | 无 |
| `mic-core` 标题 | 删除标题 60 秒总超时（`TITLE_TIMEOUT`），与正文同样只靠空闲判定；改写[标题 B2](session-title.md) 对应条款 | 标题挂起时报「模型无响应」，持续出字的慢模型不再被截断；挂起期间「生成中」最长保持一个空闲上限 |

连接失败、5xx、限流、流中途断开仍是 `Transient`，照旧重试。空闲上限保持 `STREAM_IDLE_TIMEOUT = 300s`：推理模型可能长时间只思考不输出，且 keep-alive 注释不算数据。如要缩短，由 human 另定数值。

## 三、验收

- 标题调用不再有总超时，挂起时 title_error 为「模型无响应：…」。
- provider 测试：上游只发 keep-alive，超时后得到 `Stalled`；core 测试：`Stalled` 只尝试一次，run 失败并落通知。
- `cargo fmt`、`cargo clippy -- -D warnings`。
