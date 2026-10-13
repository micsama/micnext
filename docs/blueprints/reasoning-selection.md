# B2：按会话选择思考强度

**状态：CLOSED（2026-10-13）。human 已确认功能可用，完成模型/人设入口观感调整后回到标题主题；本地验收通过。切模型档位置为 null；标题进入独立评审。**

Human 已确定方向：模型注册阶段不固定思考强度；聊天页通过节点滑条选择，档位由模型 GET 返回。本 B2 独立实现和验收；[会话标题](session-title.md)依赖本契约，反向没有依赖。

## 一、用户看到什么

- 设置 → 模型删除思考强度输入，保留服务商、模型名、最大输出和上下文窗口。
- 新会话及已有 Root 会话提供「模型名 · 强度」单一胶囊入口（2026-10-13 human 观感修正）。点击浮层展开分组模型列表及思考强度；模型列表用勾选与浅底标出当前项，滑条在浮层内，蓝色填充轨道、离散节点与白色圆形滑块。支持点击、拖动或键盘选择，拖动结束才提交；不支持该参数时隐藏，单一档位只显示标签。
- 滑条使用 `/models` 返回的节点与默认值，Web 不维护第二份按预设划分的名单。可复用于未来的标题用途配置。
- 选择保存在会话上。执行中修改从下一轮生效；本轮重试、工具往返、插话和压缩均不受影响。
- 切换模型时档位重置为 `null`（滑条显示新模型默认档位），不沿用旧值；保存失败显示错误，不伪装成成功。
- 未指定档位（`null`）用模型默认；`none` 是显式关闭思考，两者不同。同一个模型可在不同会话使用不同档位。
- 未指定模型时只允许档位为 `null`。沿用现有语义：认领时选当时默认模型并写回会话，不把它改为永久跟随全局默认。Web 修改仍须选择具体模型。

## 二、公开契约变更

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| `mic-store` 类型 | 增加 `ReasoningEffort` 七档枚举，serde 小写；`Session`、`NewSession`、`RunSettings`、`ClaimedModel::Selected` 增加 `reasoning_effort: Option<ReasoningEffort>` | 所有会话创建与认领初始化点补字段；run 持有读定的选择 |
| `mic-core` 类型 | 增加 `ReasoningOptions { choices: Vec<ReasoningEffort>, default: ReasoningEffort }`，默认必在有序 choices 内 | Gateway 模型目录与调用选择验证消费 |
| `ProviderFactory` | 新增 `reasoning_options(endpoint_json: &str, name: &str) -> Result<Option<ReasoningOptions>, ConfigError>`；`build` 末尾增加 `reasoning_effort: Option<ReasoningEffort>` | Provider 配置不再携带推理选择，core 建实例时显式传入 |
| 模型静态配置 | `check_model` 和 `config_json` 不再接收 `reasoning_effort`；其它字段不变，未知字段仍拒绝 | Provider parser、模型设置表单与 API body 同步删字段 |
| Kernel / Store | `set_session_model(session, model: ModelId, reasoning_effort: Option<ReasoningEffort>)` | 校验组合并原子保存两者；签名替换现有方法 |
| 模型选择与能力读取 | Store `selectable_model(id)` 区分不存在/已删除；Kernel `model_reasoning(&ModelView)` 经已注册工厂取能力 | 前者用于创建/选择校验，后者供 Gateway GET 与同一校验流程消费 |
| 会话创建 | `NewSession` / Web 创建 body 增加可缺省档位字段；Kernel 创建边界验证 `model=None ⇒ effort=None` 及模型/档位组合 | Web 发送所选值；CLI/微信缺省 `None`；Store 保证空模型不能有具体档位 |
| 会话响应 | HTTP `SessionItem` 增加 nullable string 字段 `reasoning_effort` | TS 类型、decoder、会话状态和聊天页读取 |
| `GET /models` | 每项增加 nullable 对象 `reasoning: { choices: [...], default: "low" }`；`config` 去掉思考字段 | 模型选择与滑条共用该目录；工厂计算能力，不把能力重复存库 |
| `PUT /sessions/{id}/model` | body 改为 `model_id`（number）与 `reasoning_effort`（nullable string），均必填；成功 204 | Root 会话沿用现有入口；模型/档位一并提交，无独立竞争写入口 |

非法/不支持档位 400，不存在模型 404，已删除模型 409，非 Root 沿用 ReadOnly。请求验证与认领建实例都结合当前 Provider 的能力；协议编辑后若旧选择失效，明确失败，不转换档位。

档位来源沿用现行本地 Provider 规则：DeepSeek none/low/high/max，OpenAI/ChatGPT 现有七档，默认 low；generic/Ollama 无档位。工厂接口接收模型名以表达能力所属对象；本次不增加联网能力发现，也不声称本地枚举能保证每个上游模型都支持。上游拒绝保持明确错误。

认领事务把档位选择与模型配置、凭据一起读定，写入 run 快照。core 建本轮 Provider 时一次性把 `null` 解析为该模型默认值（不支持时仍不发）；实例持有有效值至本轮结束。`ModelRequest` 不新增覆盖字段，压缩复用本轮 Provider。

实现影响限定为 Store 会话/认领、core 工厂与建实例、Provider 参数映射、Gateway DTO/设置接口、Web 模型设置与聊天选择。标题接口、worker、调用用途字段不在本 B2。无新 crate 或依赖。

## 三、迁移

增加会话与 run 的 nullable 档位列，既有值全部 `NULL`；删除现存模型 config JSON 中的 `reasoning_effort` 字段。不把旧模型档位搬到会话，不回填历史 run，不保留旧配置读取分支。迁移仍按现有 core schema 顺序执行，不要求重建整库。

可见变化：旧会话以后用 Provider 默认档位（现为 low），旧模型原来选的 none/high/max 不再自动保留，需在聊天页重新选择。历史记录、凭据、静态模型参数不变。此行为变更随本 B2 批准。

## 四、验收与关闭

本地验收：模型设置无档位、GET 节点与滑条一致；同模型两个会话档位独立；null/none 区别；空模型+具体档位拒绝；不支持组合拒绝；切模型默认选择；执行中修改下一轮生效；微信 Root 同入口；旧库清理字段且新档位为空。模拟上游检查普通调用、重试及压缩实际出站参数。

完成 fmt、clippy、Web check/构建。Human 验收滑条观感与真实模型参数行为，通过后 CLOSED、todo 移至归档。此项可单独批准、实施、验收，不等待标题功能。

### 本地结果（2026-10-13）

- `cargo fmt`、`cargo clippy -- -D warnings`、`web/` 的 `bun run check`（0 errors / 0 warnings）与 `bun run build` 通过。
- workspace 测试与新增 core 执行验收共 9 项通过：旧库字段清理不搬迁、会话独立与 run 快照、两个协议的出站参数，以及执行中变更不影响重试、下一轮压缩用 none、未选档位解析为 low。
- 隔离 Gateway + localhost 模拟服务的 8 组 HTTP 验收通过：模型 GET 与静态字段拒绝；空模型/具体档位拒绝；缺字段、未知及不支持档位拒绝；执行中原子切换及快照不变；微信 Root 同入口且发消息仍只读；下轮/列表反映 null；跨重启持久化；不存在 404 与已删除 409 区分。报告见 `/private/tmp/micnext-reasoning-acceptance/run-walz0xg8/result.json`，临时服务已停止。
- 当前无可用浏览器，未做页面观感验收；真实上游模型未调用。Human 验收完成前不标 CLOSED，不进入标题实施。

观感修订（2026-10-13）：按 human 提供截图，将输入框旁的常驻细滑条改为「模型名 · 强度」合并入口；分组模型选择和宽轨道滑条放入圆角浮层。前端 check/构建、fmt/clippy 通过；human 明确自行验收页面，本轮不做浏览器操作。

Human 已确认功能可用，比例继续调整：浮层宽度 320 → 288px，模型行高 44 → 36px；当前档位与「思考强度」标签合为一行，减少强度区高度；滑轨 24 → 16px、滑块 32 → 24px，缩小节点和留白。检查/构建通过，待 human 复验观感。
