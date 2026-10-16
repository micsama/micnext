# B2：上下文构成

**状态：CLOSED（2026-10-16 批准、实现并人工验收）。**

依据：[B1](../../brainstorm/context-breakdown.md)。review 后修订：只解释上一次对话调用，不预测下一次请求；系统提示改为结构化组装，构成统计与 trace 共用这一处结构。不改 schema，查询时重算。

## 一、口径

1. 解释的调用与 `occupancy`（[chat-run-stats §二](chat-run-stats.md#二上下文占用)）相同：当前有效边界下最近一次有完整用量的成功对话调用。
2. 上游只给 `input_tokens`、`output_tokens` 总数，分项只能估算：用 `context_usage` 的字数估算得到各类权重，按比例缩放到上游总数，最大余数法取整。
   - 输入六类权重缩放到 `input_tokens`。
   - `output_tokens` 全部归「对话」，其中工具调用部分按输出内容的估算比例划出。
   - 分项合计等于 `used_tokens`；`tool_tokens` 是对话的内部划分，不超过对话。
3. 数据来源（查询时重算，不落盘）：
   - 消息：按调用记录重建该次输入（与 trace `call_input` 同一函数），精确。
   - 输出：`model_call_id` 指向该调用的回复消息，精确。
   - 摘要与文件快照：取自当前边界，与该调用相同，精确。
   - 系统提示其余部分、工具 schema、图片开关：按当前配置算。这些大多数时候不变；调用后改过人设或工具，分项按新配置估，直到下一次回复。

| `kind` | 内容 |
|---|---|
| `instructions` | system.md + 消息头说明 |
| `persona` | 人设 + 通用偏好；子 agent 只有通用偏好，为空时无此块 |
| `tools` | 工具 schema + 各工具 `prompt_hint` |
| `environment` | 工作目录、渠道提示 |
| `summary` | 压缩摘要 + 压缩时的文件快照 |
| `conversation` | 全部消息与本次输出；`tool_tokens` 为其中的工具调用与结果 |

## 二、系统提示结构（一处真相）

`request` 先产出结构化的 `SystemPrompt`，再渲染成发给模型的字符串。模型请求、trace 预览、构成统计都消费这一份结构，Web 不再用正则从字符串里拆块。

```rust
pub struct SystemPrompt { pub blocks: Vec<PromptBlock> }          // 固定 instructions → persona → context
pub struct PromptBlock { pub kind: PromptBlockKind, pub parts: Vec<PromptPart> }
pub struct PromptPart { pub kind: PromptPartKind, pub text: String } // text 为实际发送的完整段落
```

- `PromptPartKind` 即 §一 中除 `conversation` 外的五类；context 块里依次是目录、渠道、各工具 hint、摘要、各文件快照，每段一个 part。
- 渲染结果与现在的字符串逐字一致，前缀缓存不受影响。
- 估算时各 part 按自身 kind 计入；标签、分隔符这类零头不单独归属，由缩放吸收。

## 三、存储

不改 schema。`context_usage` 查询额外返回所选调用的 id，occupancy 与 breakdown 共用这一条选择规则。

## 四、接口

### `GET /api/sessions/{id}/context/breakdown`

200 返回 `ContextBreakdown` 或 `null`；会话不存在返回 404。只在打开浮层时请求，打开期间 occupancy 变化时重取。

```json
{
  "used_tokens": 51234,
  "window": 128000,
  "trigger_percent": 80,
  "parts": [
    { "kind": "instructions", "tokens": 2100, "tool_tokens": null },
    { "kind": "persona", "tokens": 800, "tool_tokens": null },
    { "kind": "tools", "tokens": 3900, "tool_tokens": null },
    { "kind": "environment", "tokens": 120, "tool_tokens": null },
    { "kind": "summary", "tokens": 0, "tool_tokens": null },
    { "kind": "conversation", "tokens": 44314, "tool_tokens": 30120 }
  ]
}
```

`parts` 固定 6 项、固定顺序，为 0 也返回；`tool_tokens` 只有 `conversation` 非 null。

### `GET /api/trace/sessions/{id}/system`（响应变更）

路径不变，响应由 `{ "text" }` 改为结构加原文：

```json
{
  "text": "<instructions>\n...\n</instructions>\n\n<persona>\n...",
  "blocks": [
    { "kind": "instructions", "parts": [{ "kind": "instructions", "text": "..." }] },
    { "kind": "persona", "parts": [{ "kind": "persona", "text": "..." }] },
    { "kind": "context", "parts": [{ "kind": "environment", "text": "Working directory: ..." }, { "kind": "tools", "text": "..." }] }
  ]
}
```

`text` 由同一结构渲染，供「原文」视图和复制使用。

## 五、调用方影响

- `mic-core`
  - `request`：新增 §二 的类型，`system_prompt` 返回 `SystemPrompt`，并新增 `render`；`build` 用渲染结果填 `ModelRequest.system`。
  - `context_usage`：新增按 part 和消息分类估算的函数，与现有 `estimate_tokens` 共用底层估算。
  - 构成计算：取所选调用 → 重建输入消息、读其回复 → 结合当前配置的系统提示与工具 → 分类估算并缩放。
  - `Kernel::system_prompt` 返回 `SystemPrompt`，gateway 序列化时附 `render()` 的 `text`；新增 `Kernel::context_breakdown(session) -> Result<Option<ContextBreakdown>, KernelError>`。
- `mic-store`：`ContextUsage` 增加 `call`；`CallRecord` 增加 `reply`（该调用的回复消息 id）。
- `mic-gateway`：新路由；trace system 路由序列化新结构。
- Web
  - `ContextMeter` 可点击，新增 `ContextBreakdown.svelte` 浮层。
  - `TraceSystem` 按 `blocks` 展示，删除正则；context 卡片内按 part kind 分小节（运行环境 / 工具说明 / 压缩摘要）。`TraceTab` 中只需原文的地方取 `view.text`。
- 文档：实现时同步 `run-trace.md`（system 接口）和 `system-prompt-layout.md`（内部组装），三块布局契约不变。

## 六、页面

浮层风格同模型、人设选择器（`picker-panel`），锚定圆圈向上弹出，宽约 26rem；窄屏与选择器一样用同一 Popover。

```
┌──────────────────────────────────────────────┐
│ 上下文                                  17%  │
│ 17.0k / 100k                                 │  大号数字 + 次要色分母
│ 上次调用 · 分项估算                           │
│                                              │
│ ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪   ■ 框架规则      329    │  左：20×20 方格，约 192px
│ ▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪▪   ■ 人设与偏好     97    │  右：图例
│ ▪▪▪▪▪▪▪▫▫▫▫▫▫▫▫▫▫▫▫▫   ■ 工具        1347    │
│ ▫▫▫▫▫▫▫▫▫▫▫▫▫▫▫▫▫▫▫▫   ■ 运行环境      24    │
│ ...                    ■ 压缩摘要    4029    │
│ ▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨   ■ 对话       11.1k    │
│                          ◘ 其中工具调用 6244  │  缩进、muted，带点色块
│ ──────────────────────────────────────────── │
│ 距自动压缩 63.0k                              │  达到阈值时 warning 色「已达自动压缩阈值」
└──────────────────────────────────────────────┘
```

- **格子**：圆角 1.5px 的小方块，间距 1.5px，整体是规整的正方形色块。先按总占用算占用格数（四舍五入，最多 400），再按色段比例用最大余数法分格；格数够时非零色段至少一格。不足一格的色段占一格，按比例自下而上填一截（至少 20%），像一根小柱。对话拆成消息与工具调用两段，工具段同色、格内加一个底色小点，紧接在消息段之后。超出窗口时填满，百分比照实显示。
- **配色**：在 `app.css` 定义 6 个分类 token，明暗各一套，取 Apple 系统色。相邻分类色相拉开，一眼能看出边界：
  - 框架规则 灰 `#8e8e93 / #98989d`
  - 人设 紫 `#af52de / #bf5af2`
  - 工具 绿 `#34c759 / #30d158`
  - 运行环境 粉 `#ff2d55 / #ff375f`
  - 压缩摘要 橙 `#ff9500 / #ff9f0a`
  - 对话 蓝 `--primary`
- **图例**：只写名称和 token，为 0 的分类不列；百分比放进行 hover 的 title，避免一行两个数字。
- **联动**：悬停图例行时其余格子降到约 25% 透明度（悬停对话连同工具段一起亮）；格子本身无悬停效果。
- **动效**：打开时格子按顺序快速填入（总长约 300ms，`ease-apple`）；减弱动态效果时直接显示。

## 七、验收

1. 功能测试：渲染后的系统提示与改动前逐字一致；分项之和等于 `used_tokens`，`tool_tokens` 不超过对话；无 occupancy 时返回 null。
2. 隔离实例 curl：有工具调用的会话 `tool_tokens > 0`；`/clear` 后返回 null；trace system 返回三块且 `text` 与原先一致。
3. 页面观感交 human；`cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。
