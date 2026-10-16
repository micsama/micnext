# B1：按具体模型提供思考档位预设

**状态：方向已确认（2026-10-13）；human 修订为供应商保守默认 + 已知模型覆盖，[B2](../blueprints/archive/reasoning-model-capabilities.md) 已批准实施。现行契约以 B2 为准。**

## 问题

前端从本项目 `GET /models` 获取思考滑条节点，但后端目前按服务商预设返回固定列表：OpenAI/ChatGPT 都返回七档，工厂虽然接收模型名，实际忽略它。上游模型目录查询目前只提取名称。这把「所有可能的枚举值」误当成「具体模型支持的档位」。

已出现的错误：标题模型 `gpt-6-luna` 选择 `minimal`，本地校验通过，上游返回 HTTP 400 `unsupported_value`；它支持 `none/low/medium/high/xhigh/max`，不支持 `minimal`。标题和聊天共用该能力来源，都会受影响。

## 已确定方向

- 当前实际使用 DeepSeek 与 ChatGPT 订阅（Codex）。按供应商保守默认 + 已知模型覆盖组织能力；模型覆盖优先，未登记模型取所属供应商默认。供应商默认取常用模型的保守共同档位，避免把七档枚举当成支持列表；这是预设，不保证任意未来模型都支持。
- 后端维护按服务商预设及具体模型名匹配的能力表，记录支持档位和默认值；首批 Codex 覆盖 `gpt-6-luna` 与 `gpt-6.1-sol`。AI 建议直接写 Rust 代码：表很小且很少修改，强类型数据无需额外解析；JSON 打包会多出字段校验与加载环节，当前无明确消费者。
- 已知模型的 `GET /models` 返回其真实档位，聊天与标题滑条共用这一处来源；用户选择继续按用途分别存 SQLite，不新增浏览器持久化。
- 本次方向替代早先「未登记模型返回 reasoning: null、不发思考参数」：这两家供应商的未知模型也返回供应商默认节点；用户不选档位时按该预设默认调用。
- 新模型不需要先补表才能使用；补表只用于开放准确的手动选择。新模型名出现时按需补一条，已有模型仅在能力变化时修正；接受这部分人工维护成本。
- 具体模型默认值按对应通道资料确认；供应商默认值是产品预设，不声称是每个未知模型的上游默认。B2 已批准 DeepSeek 项目默认 low，Codex Sol 默认 low、Luna 默认 medium，未知 Codex 默认 low。
- 已保存选择不再受支持时明确报错，让用户重选，不自动改档。切换模型时继续沿用现有重置为 `null` 的行为。
- 不增加自动试探、失败重试降级、兼容分支或「上游优先、本地兜底」机制。

## 数据来源核对（2026-10-13）

- DeepSeek [`GET /models`](https://api-docs.deepseek.com/api/list-models/) 已提供 `effort.supported_levels` 与 `default_level`；当前官方示例为 low/high/max、默认 high。文档明确 supported_levels 不包含关闭思考的 none；本项目完整可选列表应单独包含 none。首版以此作为本地表的依据，不增加运行期联网同步链路。
- DeepSeek [Chat 文档](https://api-docs.deepseek.com/api/create-chat-completion/)确认关闭开关为 `thinking={type:disabled}`，也可单独用 `reasoning_effort=none`。现有 micnext 发送后者，默认显式 low；上游不传参数时则默认启用思考且 high。
- OpenAI 公开 [`GET /v1/models`](https://developers.openai.com/api/reference/resources/models/methods/list) 只提供基础模型信息，没有思考档位及默认值字段。档位和默认值按具体模型不同，以[官方推理说明](https://developers.openai.com/api/docs/guides/reasoning)和模型页核对。
- [`gpt-6-luna` 模型页](https://developers.openai.com/api/docs/models/gpt-6-luna) 明确列出上述六档，默认 `medium`。
- [`gpt-6.1-sol` 模型页](https://developers.openai.com/api/docs/models/gpt-6.1-sol) 与[推理说明](https://developers.openai.com/api/docs/guides/reasoning)确认不支持 none/minimal，默认 medium；2026-10-13 本机 Codex 实测 none 被拒绝，错误列出 low/medium/high/xhigh/max，low 调用成功。
- 上一条的 medium 是公开 API 默认，不能用于订阅通道。2026-10-13 本机 Codex 模型缓存确认 Sol 的 default_reasoning_level=low，Luna=medium；B2 已修正。目录 Luna 的支持节点未列 none，和既有报错提供的六档不同，需在实施验收时核对其接受情况。
- Codex 并非没有能力资料：[App Server 的 `model/list`](https://learn.chatgpt.com/docs/app-server#models) 返回 `supportedReasoningEfforts` 与 `defaultReasoningEffort`；本机 Codex 缓存也存在相应字段。不过这不证明 micnext 当前直连的 HTTP 模型目录具有相同字段。本次不接入 Codex App Server，也不增加自动发现。

## 后续 B2 范围

明确供应商默认与模型覆盖、各自默认值；复用现有 `ReasoningOptions` 和 nullable `reasoning`，同步 GET、选择校验及 Provider 建实例，避免前后端各维护一份表。

验收以必要的模块联动和整体调用为主：已知模型排除不支持档位；未知模型取供应商默认；聊天与标题共用能力但选择独立；旧选择失效显式报错；未登记的新模型仍可使用。页面观感交给 human。
