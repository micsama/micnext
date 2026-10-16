# B2：上下文构成

**状态：草案，待 human 批准（2026-10-16）。**

依据：[B1](../brainstorm/context-breakdown.md)。总量沿用 [chat-run-stats §二](chat-run-stats.md#二上下文占用) 的 `occupancy`，不改 schema，不落盘估算结果。

## 一、口径

1. 重新组装当前会话的下一次对话请求，与对话调用、压缩走同一组装入口（见 §三）。组装受模型「支持图片」开关影响，关闭时图片按文字占位计。
2. 用现有 `context_usage` 的字数估算逐部分估 token。
3. 各部分按 `used_tokens / 估算总和` 等比分配，取整用最大余数法，使分项之和严格等于 `used_tokens`。
4. `occupancy` 为 null 时整体为 null，不估算、不展示。

| 分类 `kind` | 内容 |
|---|---|
| `instructions` | `instructions` 块：system.md + 消息头说明 |
| `persona` | `persona` 块：人设 + 通用偏好 |
| `tools` | 工具 schema（名称、描述、参数）+ 各工具 `prompt_hint` |
| `environment` | 工作目录、渠道提示 |
| `summary` | 压缩摘要 + 压缩时的文件快照 |
| `conversation` | 全部消息，含推理回传、工具调用与结果、图片 |

`conversation` 另给 `tool_tokens`：其中工具调用参数与工具结果之和，同样按比例分配后的值。

## 二、接口

| 方法与路径 | 成功 |
|---|---|
| `GET /api/sessions/{id}/context/breakdown` | 200 `ContextBreakdown` 或 `null` |

```json
{
  "used_tokens": 51234,
  "window": 128000,
  "trigger_percent": 80,
  "parts": [
    { "kind": "instructions", "tokens": 2100 },
    { "kind": "persona", "tokens": 800 },
    { "kind": "tools", "tokens": 3900 },
    { "kind": "environment", "tokens": 120 },
    { "kind": "summary", "tokens": 0 },
    { "kind": "conversation", "tokens": 44314, "tool_tokens": 30120 }
  ]
}
```

- `parts` 固定 6 项、固定顺序，为 0 也返回；顺序即方格着色顺序（基线在前，对话在后）。
- 会话不存在 404，与 `context` 接口一致。
- 只在用户点开时请求，不随回复刷新；浮层打开期间收到 `run_finished` 时重取一次。

## 三、调用方影响

- `mic-core`
  - `request::system_prompt` 改为先产出带分类的段落，再拼成字符串；构成估算消费同一组段落。拼接结果逐字不变，前缀缓存不受影响。
  - 从 `Exec::history_request` 抽出不依赖 run 的组装入口（会话 + 设置 + 工具 + 渠道提示 + 图片开关），对话调用、压缩、构成三处共用。
  - 新增 `Kernel::context_breakdown(session) -> Result<Option<ContextBreakdown>, KernelError>`。
- `mic-gateway`：新路由。
- Web：`ContextMeter` 可点击（`occupancy` 为 null 时圆圈本就不显示）；新增 `ContextBreakdown.svelte` 浮层。

## 四、页面

浮层，宽屏锚定圆圈向上弹出，窄屏底部弹出；风格与模型、人设选择浮层一致（`picker-panel`）。

```
上下文                                  51.2k / 128k · 40%
┌──────────────────────────────────────┐
│ ■■■■■■■■■■■■■■■■■■■■                 │  20 × 10 格，每格 0.5% 窗口
│ ■■■■■■■■■■■■■■■■■■■■                 │  按 parts 顺序逐格填色
│ ■■■■■■■■■■■■■■■■■■■■                 │
│ ■■■■■■■■□□□□□□□□□□□□                 │  空余：淡底色
│ □□□□□□□□□□□□□□□□□□□□                 │
│ ...                                  │
│ ▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨▨                 │  压缩阈值之后：斜纹
└──────────────────────────────────────┘
● 框架规则     2.1k   1.6%
● 人设与偏好    800   0.6%
● 工具         3.9k   3.0%
● 运行环境      120   0.1%
● 对话        44.3k  34.6%
    其中工具调用与结果 30.1k
○ 剩余        76.8k   距自动压缩 51.2k
```

- 格子：圆角小方块，格间 2px；占用格用分类色，空余格用 `--muted`，阈值后的格子在空余色上加细斜纹。
- 配色：6 个分类色在 `app.css` 定义为 token，明暗两套；色相分开、饱和度克制，取系统色系（蓝、靛、紫、青、橙、灰）。
- 有占用但不足半格的分类至少占 1 格，保证每个出现的分类都能看到；多出的格从最大的分类里扣。
- 联动：悬停图例行，其余分类的格子变淡；悬停格子，对应图例行高亮。
- 打开时格子按顺序快速逐格填入（总时长约 300ms，`ease-apple`）；减弱动态效果时直接显示。
- 为 0 的分类不进图例（如无摘要）。

## 五、验收

1. core 测试：分项之和等于 `used_tokens`；`occupancy` 为 null 时返回 null；`system_prompt` 拼接结果与改动前逐字相同。
2. 隔离实例 curl：有工具调用的会话 `tool_tokens > 0`；`/clear` 后返回 null。
3. 页面观感交 human；`cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。
