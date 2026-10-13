# B1：按具体模型提供思考档位预设

**状态：方向已确认，human 要求先记录（2026-10-13）；暂不实现，实施前另起 B2。**

## 问题

前端从本项目 `GET /models` 获取思考滑条节点，但后端目前按服务商预设返回固定列表：OpenAI/ChatGPT 都返回七档，工厂虽然接收模型名，实际忽略它。上游模型目录查询目前只提取名称。这把「所有可能的枚举值」误当成「具体模型支持的档位」。

已出现的错误：标题模型 `gpt-6-luna` 选择 `minimal`，本地校验通过，上游返回 HTTP 400 `unsupported_value`；它支持 `none/low/medium/high/xhigh/max`，不支持 `minimal`。标题和聊天共用该能力来源，都会受影响。

## 已确定方向

- 针对能力明确的供应商，后端代码维护按服务商预设及具体模型名匹配的能力表，记录支持档位和默认值；只维护实际使用、资料可确认的模型，不另加 JSON/YAML 配置文件。
- 已知模型的 `GET /models` 返回其真实档位，聊天与标题滑条共用这一处来源；用户选择继续按用途分别存 SQLite，不新增浏览器持久化。
- 未登记的模型返回现有 `reasoning: null`，界面显示使用模型默认，不提供档位选择；调用不发送思考参数。未知不等于上游不支持思考，只表示本项目没有明确能力资料。
- 新模型不需要先补表才能使用；补表只用于开放准确的手动选择。新模型名出现时按需补一条，已有模型仅在能力变化时修正；接受这部分人工维护成本。
- 默认档位必须有依据，不把 `medium` 或 `low` 当作未知模型的安全默认，也不把全量枚举当成支持列表。
- 已保存选择不再受支持时明确报错，让用户重选，不自动改档。切换模型时继续沿用现有重置为 `null` 的行为。
- 不增加自动试探、失败重试降级、兼容分支或「上游优先、本地兜底」机制。

## 数据来源核对（2026-10-13）

- OpenAI 公开 [`GET /v1/models`](https://developers.openai.com/api/reference/resources/models/methods/list) 只提供基础模型信息，没有思考档位及默认值字段。档位和默认值按具体模型不同，以[官方推理说明](https://developers.openai.com/api/docs/guides/reasoning)和模型页核对。
- [`gpt-6-luna` 模型页](https://developers.openai.com/api/docs/models/gpt-6-luna) 明确列出上述六档，默认 `medium`。
- Codex 并非没有能力资料：[App Server 的 `model/list`](https://learn.chatgpt.com/docs/app-server#models) 返回 `supportedReasoningEfforts` 与 `defaultReasoningEffort`；本机 Codex 缓存也存在相应字段。不过这不证明 micnext 当前直连的 HTTP 模型目录具有相同字段。本次不接入 Codex App Server，也不增加自动发现。

## 后续 B2 范围

明确能力表首批供应商与模型、各自默认值；复用现有 `ReasoningOptions` 和 nullable `reasoning`，同步 GET、选择校验、Provider 建实例及界面说明，避免前后端各维护一份表。不得把七档枚举重新用作未知模型能力。

验收以必要的模块联动和整体调用为主：已知模型排除不支持档位；未知模型省略思考参数；聊天与标题共用能力但选择独立；旧选择失效显式报错；未登记的新模型仍可使用。页面观感交给 human。
