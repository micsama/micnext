# B2: Trace 展示工具清单，删除工具提示

**状态：CLOSED（human 2026-10-20 批准并验收通过）。**

## 动机

- Trace 的「系统提示」只有系统提示文本，看不到实际加载给模型的工具与参数说明。
- 工具的 `prompt_hint` 拼进系统提示，与工具 description 重复且多为说教，删除。工具说明只写能力范围；跑出问题再按需补。

## 契约

| 位置 | 变化 |
|---|---|
| mic-tool | `Tool::prompt_hint`、`ToolHandle::prompt_hint` 删除；`ToolSpec` 可序列化 |
| mic-core | 系统提示 `context` 块不再含工具段；`PromptPartKind::Tools` 删除（`ContextKind::Tools` 仍统计工具定义）；新增 `Kernel::session_tools(session) -> Vec<ToolSpec>` |
| `GET /api/trace/sessions/{id}/system` | 增加 `tools: [{ name, description, parameters }]`，即该会话当前配置下发给模型的工具定义，顺序同请求 |
| Web Trace「系统提示」 | tab：分块 / 原文 / 工具。工具 tab 每个工具一张卡：名字、描述、参数（名、类型、必填、说明），可看原始 schema |

与系统提示一样按当前配置计算，不存快照（见 CLAUDE.md 版本规则）。逐次调用已存模型、推理强度与用途；`tool_choice` 由用途决定（标题、压缩不调工具），无需另存。

## 验收

- 系统提示原文里没有工具段。
- Trace「工具」tab 列出的工具与会话范围一致：子 agent 没有 `agent`。
