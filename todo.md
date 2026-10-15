# todo 索引

活跃条目按状态列出，正文放各自的 B1/B2 文档，这里只留一行指针。收尾后的条目
挪去 [`todo-archive.md`](todo-archive.md)，不留在这里。

| 状态 | 主题 | 文档 |
|---|---|---|
| 以后做（2026-10-15 human 定） | 服务商分时段计价：空闲时段挂服务商，模型另填空闲价，费用按调用开始时间取价；币种保持美元。现按高峰价计，DeepSeek 已按高峰价填库 | [B2](docs/blueprints/offpeak-pricing.md) |
| 已实现，待验收（2026-10-15） | 旁路调用记录输入与产出：手动标题另带最近 2 条 AI 回复（截断）；标题调用的输入指向原消息，产出存为不进上下文的 `SideOutput` 消息；以后新旁路沿用 | [B2](docs/blueprints/side-call-output.md) |
| 已实现，待验收（2026-10-15） | 流式调用长时间无数据即判挂起、直接失败不重试（起因：DeepSeek 思考模式 + `tool_choice: none` 卡死，已在 provider 规避） | [B2](docs/blueprints/model-stall.md) |
| 已实现，待真机验收（2026-10-15） | 运行中停止：Web 停止键与 Web/微信 `/stop`；半截回复保留并通知「已停止」，工具记已取消，未并入的消息照常开新一轮；run 新增「用户停止」终态 | [B2](docs/blueprints/run-stop.md)；[B1](docs/brainstorm/run-stop.md) |
| 想法，以后做（2026-10-15） | trace「复制为 curl / 重新推理」：消息按记录还原，系统提示与工具清单用当前配置；工具清单视为固定，与历史工具调用对不上则拒绝重放 | [trace B1](docs/brainstorm/run-trace.md#未来重放与复制为-curl) |
| human 明确延期（2026-10-13） | 自建人设支持设置 emoji；当前统一使用 🎭，以后独立定义契约 | [记录](docs/blueprints/web-ui.md#43-设置页与人设选择) |
| B2 已实现：DeepSeek、ChatGPT 订阅（2026-10-12）已验收，OpenAI（API key）与订阅缓存命中待验收 | 模型调用迁移 async-openai；新增 OpenAI（Responses）与 ChatGPT 订阅（Codex 后端，手贴 access_token）预设；Responses 发会话缓存键 | [SDK B2](docs/blueprints/provider-sdk-responses.md)；[ChatGPT B2](docs/blueprints/provider-chatgpt.md)；[缓存键 B2](docs/blueprints/provider-cache-key.md)；现行契约 [provider-openai](docs/blueprints/provider-openai.md) |
| B2 CLOSED（2026-10-09） | 构建脚本支持 debug 与 run：默认 release，可构建后 exec 启动，运行参数原样透传 | [build-script-run](docs/blueprints/build-script-run.md) |
| B2 CLOSED（2026-10-10） | 对话记录与用户所见大致一致：长期记录与系统解释、临时微信媒体打扁；短占位与合并通知省 token；含占位入站只记录 | [B2](docs/blueprints/conversation-parity.md)；[B1](docs/brainstorm/conversation-parity.md) |
| B2 CLOSED（2026-10-09） | 统一消息头解释与 system prompt 布局；沿用现有模型呈现所有权 | [`docs/blueprints/system-prompt-layout.md`](docs/blueprints/system-prompt-layout.md) |
| B1 余项：B（模型输入呈现）、F（工具参数）、E 余项（会话身份）待定；A 并入阶段二 | 最小正交审查余项 | [`docs/brainstorm/orthogonality-review-2026-09-24.md`](docs/brainstorm/orthogonality-review-2026-09-24.md) |
| B1 第二轮收敛中 | 产品定位、Web + 微信、定时看板、同步/异步与 v0–v3 路线；全项目待决策项见 §四 | [`docs/brainstorm/product-roadmap.md`](docs/brainstorm/product-roadmap.md) |
| 进行中（步 1～6 与存储重整完成，v0a Web 闭环打通） | v0a 模块地图：Web 闭环的模块划分、对接方式、实现顺序（v0a 总索引） | [`docs/blueprints/v0a-module-map.md`](docs/blueprints/v0a-module-map.md) |
| B2 CLOSED（2026-10-12）；fmt/clippy、Web 检查/构建与 12 项隔离链路验收通过；human 接手真实使用验收，OpenAI key 401 待补 | 上下文压缩与 `/compact`、`/clear`：每次模型返回后按其上游用量判定，默认 80% 触发，压完再接上工具结果或新输入；只读最新有效 Boundary，clear 后排队输入正常执行；快照走 read；Web 展示结果并可将当前压缩边界置为失效撤回，保留审计记录 | [B1](docs/brainstorm/context-compaction.md)；[B2](docs/blueprints/context-compaction.md) |
| B1 待核对，再起 B2 | 定时任务看板事实来源与异步 shell/子 agent 生命周期 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-4～5 |
| B2 CLOSED：服务商/模型分层与图片输入均已实现并验收；遗留：模型不支持图片时的上游报错呈现、只发图时会话预览为空 | 运行期设置阶段二：模型设置/工厂/凭据、图片输入、本轮快照与模型呈现 | [`docs/blueprints/model-settings.md`](docs/blueprints/model-settings.md)；取舍见 [B1](docs/brainstorm/model-settings.md) §九 |
| 已实现，本地验收通过；页面观感待 human（2026-10-15） | 聊天统计行（每轮一行：步数、模型/工具耗时、首字、速率、token、缓存、费用）与输入框上下文占用；后端一处计算，trace 运行概览共用 | [B2](docs/blueprints/chat-run-stats.md)；[B1](docs/brainstorm/chat-run-stats.md) |
| B1 已记录；脱敏明确延期（2026-10-10） | 模型出站脱敏、工具隔离与授权：脱敏只求减少部分意外披露，当前不实施；工具隔离既有缺口仍待定。专用用户部署 human 进行中，服务器 bwrap 已验证可运行；访问与披露各自控制，候选未批准 | [B1](docs/brainstorm/privacy-permissions.md)；合并「工具授权」缺口（cross-check §四-8～9） |
| 引用 CLOSED（2026-10-10 服务器验收）；入站图片 CLOSED（2026-10-10 服务器验收，[B2](docs/blueprints/wechat-inbound-images.md)）；入站文件/视频已实现待验收（[B2](docs/blueprints/wechat-inbound-files.md)） | 微信 V2：引用关联归微信，不新增核心来源登记接口；随后入站图片/文件/视频、出站媒体；斜杠指令跨 Channel 统一另起 B1 | [引用 B2](docs/blueprints/wechat-quotes.md)；[V2 B1](docs/brainstorm/wechat-v2.md) §二 |
| 已知 bug，低优先级；human 明确延期（2026-10-10） | 微信 Markdown/表格部分引用无法精确定位时提供完整被引消息并注明；显示文字的精确还原后续再处理，不阻塞本轮引用验收 | [引用 B2](docs/blueprints/wechat-quotes.md)；[样本事实](docs/brainstorm/wechat-v2.md) §三 |
| 想法，未定；现状够用（2026-10-12） | 日志页开关「落盘包含 DEBUG」：存 SQLite，跨重启有效，仅项目 crate，开启时页面提示。场景：新功能临时调试时复现与查看之间有重启；现状 DEBUG 只在内存实时可看，重启即丢 | [B2](docs/blueprints/developer-log-history.md) |
| 想法，未定 | 人设绑定 UI 主题；「理性大脑」人设定稿 | [`docs/brainstorm/runtime-settings.md`](docs/brainstorm/runtime-settings.md) §三 |
| 已知缺口，待定义验收边界 | 入站去重、投递重复；模型/工具/历史尺寸、工具授权与进程退出 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-3、8～9 |
