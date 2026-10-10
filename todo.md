# todo 索引

活跃条目按状态列出，正文放各自的 B1/B2 文档，这里只留一行指针。收尾后的条目
挪去 [`todo-archive.md`](todo-archive.md)，不留在这里。

| 状态 | 主题 | 文档 |
|---|---|---|
| B2 已实现，待实跑验收 | 模型调用迁移 async-openai，新增 OpenAI（Responses）预设；ChatGPT 订阅 Token 待 human 实测后另补 | [B2](docs/blueprints/provider-sdk-responses.md)；现行契约 [provider-openai](docs/blueprints/provider-openai.md) |
| B2 CLOSED（2026-10-09） | 构建脚本支持 debug 与 run：默认 release，可构建后 exec 启动，运行参数原样透传 | [build-script-run](docs/blueprints/build-script-run.md) |
| B2 CLOSED（2026-10-10） | 对话记录与用户所见大致一致：长期记录与系统解释、临时微信媒体打扁；短占位与合并通知省 token；含占位入站只记录 | [B2](docs/blueprints/conversation-parity.md)；[B1](docs/brainstorm/conversation-parity.md) |
| B2 CLOSED（2026-10-09） | 统一消息头解释与 system prompt 布局；沿用现有模型呈现所有权 | [`docs/blueprints/system-prompt-layout.md`](docs/blueprints/system-prompt-layout.md) |
| B1 余项：B（模型输入呈现）、F（工具参数）、E 余项（会话身份）待定；A 并入阶段二 | 最小正交审查余项 | [`docs/brainstorm/orthogonality-review-2026-09-24.md`](docs/brainstorm/orthogonality-review-2026-09-24.md) |
| B1 第二轮收敛中 | 产品定位、Web + 微信、定时看板、同步/异步与 v0–v3 路线；全项目待决策项见 §四 | [`docs/brainstorm/product-roadmap.md`](docs/brainstorm/product-roadmap.md) |
| 进行中（步 1～6 与存储重整完成，v0a Web 闭环打通） | v0a 模块地图：Web 闭环的模块划分、对接方式、实现顺序（v0a 总索引） | [`docs/blueprints/v0a-module-map.md`](docs/blueprints/v0a-module-map.md) |
| B1 方向已定，契约待起草 | 统一自动上下文压缩：全部 Channel 共用 core 能力，触发与执行协调待定 | [`docs/brainstorm/wechat-channel.md`](docs/brainstorm/wechat-channel.md) §四-D；后续单独起 B1/B2 |
| B1 待核对，再起 B2 | 定时任务看板事实来源与异步 shell/子 agent 生命周期 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-4～5 |
| B2 CLOSED：服务商/模型分层与图片输入均已实现并验收；遗留：模型不支持图片时的上游报错呈现、只发图时会话预览为空 | 运行期设置阶段二：模型设置/工厂/凭据、图片输入、本轮快照与模型呈现 | [`docs/blueprints/model-settings.md`](docs/blueprints/model-settings.md)；取舍见 [B1](docs/brainstorm/model-settings.md) §九 |
| 想法，待起 B1 | 会话/轮次基本属性随 session、turn 下发：输入框下统计行（轮·步·LLM/工具耗时·首 token·tok/s·缓存命中·token）与上下文占用圆环（悬停看用量）。现状：用量与调用起止已落库；工具耗时、首 token 未记录；模型无上下文窗口大小配置 | — |
| 待起 B1（2026-10-09 服务器实测触发） | 工具执行隔离：模型以 root 经 bash 读了 micnext.db 与 `master.key` 所在目录。分两层：① 部署改用专用用户 `micnext`（human 进行中）；② bash 子进程套 bwrap，不挂 micnext 数据目录、私有 /tmp、按需 `--unshare-pid`；fs 工具在进程内，须另设路径边界；数据目录清单由 micnext 启动侧下发，不在两工具各写一份。待定：模型对服务器的权限定位（运维助手 vs 受限）、是否断网。环境事实：Ubuntu 24.04 `apparmor_restrict_unprivileged_userns=1`，已加 `/etc/apparmor.d/bwrap` 单程序放行并以 micnext 用户验证隔离生效 | 与「工具授权」缺口（cross-check §四-8～9）合并讨论 |
| 引用 CLOSED（2026-10-10 服务器验收）；下一步入站图片，待起 B2 | 微信 V2：引用关联归微信，不新增核心来源登记接口；随后入站图片/文件/视频、出站媒体；斜杠指令跨 Channel 统一另起 B1 | [引用 B2](docs/blueprints/wechat-quotes.md)；[V2 B1](docs/brainstorm/wechat-v2.md) |
| 已知 bug，低优先级；human 明确延期（2026-10-10） | 微信 Markdown/表格部分引用无法精确定位时提供完整被引消息并注明；显示文字的精确还原后续再处理，不阻塞本轮引用验收 | [引用 B2](docs/blueprints/wechat-quotes.md)；[样本事实](docs/brainstorm/wechat-v2.md) §三 |
| 想法，未定 | 人设绑定 UI 主题；「理性大脑」人设定稿 | [`docs/brainstorm/runtime-settings.md`](docs/brainstorm/runtime-settings.md) §三 |
| 已知缺口，待定义验收边界 | 入站去重、投递重复；模型/工具/历史尺寸、工具授权与进程退出 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-3、8～9 |
