# B2：供应商默认思考档位与已知模型覆盖

**状态：human 已批准并已实现，本地验收通过（2026-10-13），待服务器人工验收。**

依据：[B1](../brainstorm/reasoning-model-capabilities.md)。Human 已确定供应商保守默认 + 已知模型覆盖；本文将首批值与调用方影响具体化。

## 一、能力表与匹配

只修订当前实际使用的 `deepseek`、`chatgpt` 预设。能力表直接写在 `mic-provider-openai` 的 Rust 实现里，由现有 `config` 模块承载；无需新增文件格式、依赖或运行期配置入口。匹配先查预设下的完整模型名，再取该预设默认。覆盖是完整 choices/default 对，不做字段级合并，不按名称前缀推断。

首批已批准值：

| 预设 / 模型 | choices | default |
|---|---|---|
| DeepSeek 供应商默认 | none / low / high / max | low |
| Codex 供应商默认 | low / medium / high | low |
| Codex `gpt-6-luna` | none / low / medium / high / xhigh / max | medium |
| Codex `gpt-6.1-sol` | low / medium / high / xhigh / max | low |

Codex 两个首批模型的完整交集是 low/medium/high/xhigh/max；供应商默认进一步收窄到 low/medium/high，用于更广的未登记模型。它是产品预设，不能保证未来任意模型兼容。未知模型的上游拒绝仍明确报错；不试探或自动改档。后续补表只开放准确选择，不阻塞新模型注册。

DeepSeek [模型目录](https://api-docs.deepseek.com/api/list-models/) 的 supported_levels 为开启思考后的档位，不包含合法关闭值 none；上游默认 high。本项目预设继续取 low，两者分开记录。[Chat 接口文档](https://api-docs.deepseek.com/api/create-chat-completion/)明确 `reasoning_effort=none` 关闭思考，也接受 `thinking={type:disabled}`；现有协议映射继续只发送 reasoning_effort，不增加重复开关。本次将资料落实为本地预设，不增加模型目录能力同步、缓存或落库链路。

Codex 默认以订阅目录为依据：2026-10-13 09:21 UTC 本机 `~/.codex/models_cache.json` 记录 Sol 默认 low、Luna 默认 medium；公开 API 的 Sol 默认 medium 不可直接用于订阅通道。能力范围参考 [Luna 模型页](https://developers.openai.com/api/docs/models/gpt-6-luna)、[Sol 模型页](https://developers.openai.com/api/docs/models/gpt-6.1-sol)及既有上游报错记录。实际 Codex 调用确认 Sol 拒绝 none、支持 low；本机目录的 Luna 可选列表不含 none，和此前报错记录给出的六档有差异，实施验收需核对 none 的真实接受情况，不把目录显示节点等同于完整协议能力。Sol 目录另含 ultra（带自动委派语义），不纳入本项目现有七档枚举。

## 二、契约与调用方影响

不改公开 Rust 签名或 HTTP JSON 形状；改变现有能力方法与 GET 的返回值语义：

| 边界 | 变化 | 调用方影响 |
|---|---|---|
| `ProviderFactory::reasoning_options(endpoint_json, model_name)` | `chatgpt` 使用模型名选择覆盖；否则返回供应商默认。DeepSeek 取本次预设 | core 现有查询、设置校验、建实例路径共用同一处能力 |
| `GET /models` 的 reasoning | choices/default 改为上表 | Web 聊天、新会话、标题滑条沿现有字段显示，无前端模型表 |
| 会话与标题用途的空档位 | DeepSeek、Codex Sol 与 Codex 供应商默认保持 low，Luna 改 medium | 下次新建 Provider 使用对应默认；正在执行的 Provider/快照不变 |
| 显式已保存档位 | 必须在当前匹配结果的 choices 内 | 失效选择在保存/建实例边界明确报错，不清理数据库、不转换值 |

供应商预设默认是内置能力数据，区别于用户在 SQLite 中选择的默认模型或用途档位。`null` 仍表示使用本项目解析后的默认档位，`none` 仍是显式关闭；未登记 Codex 模型现在也有供应商节点与默认值，不按早先 B1 返回 reasoning:null。

其它已有预设行为本次保持现状，不补完整全供应商目录。无数据库迁移；同一能力来源用于会话创建、模型切换、标题选择与实际调用。压缩继续复用本轮 Provider，不产生独立能力规则。

## 三、验收与关闭

必要的模块联动和模拟上游验收：

- GET、设置校验与出站参数使用相同能力；Luna 排除 minimal，Sol 排除 none/minimal。
- 未登记 Codex 模型显示三档，null 出站 low；未知 DeepSeek 模型取其预设，null 出站 low。
- 两个已知模型显式档位、默认档位正确；聊天与标题选择独立；旧失效选择明确报错。
- 执行中更改用途选择仍只影响下一次调用；模型切换继续重置档位 null。

完成 cargo fmt、cargo clippy -- -D warnings；涉及前端改动则运行 web/bun run check。Human 验收服务器上的 DeepSeek、Codex 常用模型与滑条节点，通过后 CLOSED、todo 移至归档。

### 本地验收记录（2026-10-13）

- 能力直接放在现有 config 方法的强类型匹配表中；工厂开始消费模型名，core/Gateway/Web 继续使用现有路径，无依赖、数据库或前端变更。
- cargo fmt、cargo clippy --all-targets -- -D warnings、cargo build -p micnext 通过。
- cargo test -p mic-provider-openai -p mic-core 共 11 项通过。新增一条模拟上游联动用例，验证 DeepSeek、Codex 两个已知模型及未知模型的默认值实际进入 Chat/Responses 请求；已有 core 用例继续验证重试与压缩使用本轮档位快照。
- 隔离 Gateway/SQLite 的 11 组验收通过：GET 精确匹配与供应商默认、模型字段边界、创建/切模型/标题共用校验、用途选择独立、执行中切换不改 run 快照、下一轮反映新选择、旧失效档位在标题与普通调用建 Provider 前明确失败、重启持久化、模型删除错误区分。报告：`/private/tmp/micnext-reasoning-acceptance/run-vm22vzyy/result.json`；脚本只调用本机模拟服务。Human 将未知 Codex 默认修订为 low 后已重跑测试、隔离验收及 fmt/clippy/build，全部通过。
- 真实供应商与页面观感交 human；特别核对 Codex Luna 的 none（订阅目录显示节点与既有协议报错资料存在差异），暂不关闭。
