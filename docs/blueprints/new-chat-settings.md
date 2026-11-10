# B2：新会话发送前可关闭凭据脱敏

状态：已实现，待验收。

来源：[B1](../brainstorm/new-chat-reuse.md)，方向 A。

## 行为

- 新会话页工具栏加「临时关闭凭据脱敏」开关，和会话页同一组件，默认不选中；选中时输入框同样显示警示色。
- 选中后发送，从第一条消息起不脱敏；进入会话后开关保持选中，和在会话页打开开关完全一样。

## 实现

- Gateway 建会话后、写入首条消息前，按 `plaintext` 调用现有的 `set_session_plain(id, true)`。此时会话还没有消息，水位按现有规则落在 `0`，即从头不脱敏。水位规则仍只在 `set_session_plain` 一处。
- 中途失败留下空会话，和现在首条消息写入失败的情况相同，不另做处理。

## 契约

- **Gateway**：`POST /sessions` 的请求体新增 `plaintext`，缺省为 `false`。
- **Web**：会话页的开关（图标、提示、开关）抽成 `PlaintextToggle`（`checked` + `onchange`），两页共用。`createSession` 改为接收一个对象 `{ parts, persona_id, model_id, reasoning_effort, plaintext }`，不再继续加位置参数。
- mic-store、mic-core 不变。

## 验收

- 本地：`cargo fmt`、`clippy`、`bun run check`。
- human：在新会话页选中开关，发一条带示例凭据的消息，到 Trace 里确认模型看到的是原文。
