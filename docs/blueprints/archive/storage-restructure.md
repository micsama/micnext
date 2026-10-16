# B2: 存储结构重整（session / run / message / model_call）

**状态**: CLOSED（2026-09-24 批准并实现；§四 验收 1～5 通过；跨 person 并入的端到端验证随 M9）
**来源**: 2026-09-24 B1 讨论（已并入下列各文档，不再单列）；排在 M9 网关前。
**切换原则**: 推翻重写，不留兼容。旧类型、旧表、旧命名一律删除，不留别名或双轨；受影响的 blueprint
在实现的同一次改动里改写成新结构（§三）。库版本仍为 core v1，切换前手动删除已有库。
**依赖不变量**: 不新增 crate，不新增跨 crate 依赖。

契约已并入下方各文档，本文只留效果、构造与指针；为什么这样拆见 git 历史。

## 一、用户视角的效果

- 一次模型回复在历史里就是**一条**消息（推理、正文、工具调用按生成顺序在里面），Web 上一次回复
  对应一个气泡，不会被拆成几段。
- 每次调用模型都留一条记录，失败和重试也在内；用量是上游报了才有，没报就是空，不再显示成 0。
- 用户在 agent 干活时插话、多人消息交错、进程被杀后重启，行为与原来一致（mic-store §4.1、run-execution §4.6）。
- 升级需要删掉旧的数据库文件（开发期只有你一个人的库）。

## 二、四个构造与由此得到的性质

| 构造 | 一句话 |
|---|---|
| **message 闭集** | 一条消息 = 一个产出者的一次产出，种类由 `MessageBody` 变体唯一决定 |
| **run_id 归属** | 输入和产出靠一个可空外键挂到 run；输入的 `run_id` 只从空填成非空一次 |
| **model_call 事实** | 每次调用尝试一行；`Reply` 只能与它在同一事务里产生，并指向它 |
| **请求模型名** | 调用前就已知的上游模型名，由 `Provider::model()` 给出，调用记录与推理回传用同一口径 |

组合出来、不需要额外机制的性质：

1. **一个谓词，四处复用**：「未认领输入」= `kind ∈ {UserInput, Completion} ∧ run_id IS NULL`。
   认领、并入、启动补跑扫描、上下文排除都用它；`claimed_start_id`/`claimed_end_id` 区间与
   `request::order` 的 `claimed_end` 参数一起消失。
2. **认领 = 新建 run + 并入**：两者取同一段输入（未认领输入按 id 排序后开头连续同 person 的一段），
   只差并入要求该段 person 等于 run 的 person。
3. **崩溃收尾不做 id 区间运算**：取 `run_id = R` 的消息，`Reply` 里的 `ToolCall` 块减去已有
   `ToolResult`，差集补 `cancelled`。
4. **草稿与落盘一一对应**：一次调用尝试恰好以「`Reply` 的 `MessageAppended`」或「`DraftDiscarded`」
   之一结束；`DraftEnded` 删除。Gateway 回放交接只需判断一个 `Reply` 是否已回放。
5. **用量只有一处**：挂在调用行上，按会话、run、模型统计都是 `core_model_calls` 上的聚合，
   失败重试的成本也在里面；消息表不带用量。
6. **非法组合不可表示**：原来「作者 × 内容」能写出 `User + ToolCall` 这种不存在的组合，
   认领判别与 `model_view` 要按组合匹配；现在一个 `kind` 决定一切。
7. **换模型自然停回传**：`Reply.model`（请求模型名）与 `provider.model()` 比较，两端同口径，
   不再受上游带版本后缀的回报名干扰。

## 三、契约所在

| 内容 | 文档 |
|---|---|
| `MessageBody` 闭集、`ReplyBlock`、模型视图 | [`mic-message.md`](mic-message.md) |
| 类型、写入口、认领/并入、schema、不变量 | [`mic-store.md`](mic-store.md) §三～§六 |
| `Provider::model()`、`ModelResponse.blocks`、`Option` 用量 | [`provider-port.md`](provider-port.md) |
| 一条 `Reply` 一条 assistant 消息、用量不编造 | [`provider-openai.md`](provider-openai.md) |
| 事件、run 循环、请求组装、崩溃收尾、`-p` 呈现 | [`run-execution.md`](run-execution.md) |
| `append_user_input` 签名、启动收尾 | [`mic-core-module.md`](mic-core-module.md) |

## 四、验收（已通过）

1. `cargo fmt` + `cargo clippy -- -D warnings`。
2. 删库后 `micnext -p "列出当前目录"`：一条 `UserInput`、每次调用一条 `Reply` + 一行调用记录
   （`model` 为请求模型名，上游没报的用量列为空）、工具结果挂同一 run；run `completed`。
3. 工具执行中杀进程再启动：run 为 `interrupted`，缺的 `ToolResult` 补为 `cancelled`，加一条通知。
4. DeepSeek 带工具往返（推理回传）通过。
5. 错误 key：调用行 `error` 有值、用量全空，run 为 `provider_failed`。
