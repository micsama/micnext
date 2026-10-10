# todo 索引

活跃条目按状态列出，正文放各自的 B1/B2 文档，这里只留一行指针。收尾后的条目
挪去 [`todo-archive.md`](todo-archive.md)，不留在这里。

| 状态 | 主题 | 文档 |
|---|---|---|
| B2 CLOSED（2026-10-09） | 构建脚本支持 debug 与 run：默认 release，可构建后 exec 启动，运行参数原样透传 | [build-script-run](docs/blueprints/build-script-run.md) |
| B2 已批准并实现，fmt/clippy 通过，待服务器验收后 CLOSED（2026-10-09） | 渠道提示：仅 Root，简短描述界面与表达适配；告知复用 Reply 投递 | [B2](docs/blueprints/channel-prompt.md)；[审查依据](docs/brainstorm/channel-prompt-review.md) |
| B2 CLOSED（2026-10-10） | 对话记录与用户所见大致一致：长期记录与系统解释、临时微信媒体打扁；短占位与合并通知省 token；含占位入站只记录 | [B2](docs/blueprints/conversation-parity.md)；[B1](docs/brainstorm/conversation-parity.md) |
| B2 CLOSED（2026-10-09） | 统一消息头解释与 system prompt 布局；沿用现有模型呈现所有权 | [`docs/blueprints/system-prompt-layout.md`](docs/blueprints/system-prompt-layout.md) |
| B1 余项：B（模型输入呈现）、F（工具参数）、E 余项（会话身份）待定；A 并入阶段二 | 最小正交审查余项 | [`docs/brainstorm/orthogonality-review-2026-09-24.md`](docs/brainstorm/orthogonality-review-2026-09-24.md) |
| B1 第二轮收敛中 | 产品定位、Web + 微信、定时看板、同步/异步与 v0–v3 路线；全项目待决策项见 §四 | [`docs/brainstorm/product-roadmap.md`](docs/brainstorm/product-roadmap.md) |
| 进行中（步 1～6 与存储重整完成，v0a Web 闭环打通） | v0a 模块地图：Web 闭环的模块划分、对接方式、实现顺序（v0a 总索引） | [`docs/blueprints/v0a-module-map.md`](docs/blueprints/v0a-module-map.md) |
| v0b：Phase 1 CLOSED；Phase 2–5 已实现，真实扫码/收发已在服务器跑通；Phase 6 typing 已实现待验收，整体验收后 CLOSED | 微信 Channel：启动待命 → Kernel 能力 → Web 登录 → 入站 → 回复投递 → typing/整体验收；Bun 构建已本地验证并已推送 | [`docs/blueprints/wechat-channel.md`](docs/blueprints/wechat-channel.md) §十一（当前交接见 §11.5）；决定来源 [`B1`](docs/brainstorm/wechat-channel.md)、协议事实 [`wechat-protocol.md`](docs/brainstorm/wechat-protocol.md) |
| B1 方向已定，契约待起草 | 统一自动上下文压缩：全部 Channel 共用 core 能力，触发与执行协调待定 | [`docs/brainstorm/wechat-channel.md`](docs/brainstorm/wechat-channel.md) §四-D；后续单独起 B1/B2 |
| B1 待核对，再起 B2 | 定时任务看板事实来源与异步 shell/子 agent 生命周期 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-4～5 |
| B2 CLOSED：服务商/模型分层与图片输入均已实现并验收；遗留：模型不支持图片时的上游报错呈现、只发图时会话预览为空 | 运行期设置阶段二：模型设置/工厂/凭据、图片输入、本轮快照与模型呈现 | [`docs/blueprints/model-settings.md`](docs/blueprints/model-settings.md)；取舍见 [B1](docs/brainstorm/model-settings.md) §九 |
| 想法，待起 B1 | 会话/轮次基本属性随 session、turn 下发：输入框下统计行（轮·步·LLM/工具耗时·首 token·tok/s·缓存命中·token）与上下文占用圆环（悬停看用量）。现状：用量与调用起止已落库；工具耗时、首 token 未记录；模型无上下文窗口大小配置 | — |
| 待起 B1（2026-10-09 服务器实测触发） | 工具执行隔离：模型以 root 经 bash 读了 micnext.db 与 `master.key` 所在目录。分两层：① 部署改用专用用户 `micnext`（human 进行中）；② bash 子进程套 bwrap，不挂 micnext 数据目录、私有 /tmp、按需 `--unshare-pid`；fs 工具在进程内，须另设路径边界；数据目录清单由 micnext 启动侧下发，不在两工具各写一份。待定：模型对服务器的权限定位（运维助手 vs 受限）、是否断网。环境事实：Ubuntu 24.04 `apparmor_restrict_unprivileged_userns=1`，已加 `/etc/apparmor.d/bwrap` 单程序放行并以 micnext 用户验证隔离生效 | 与「工具授权」缺口（cross-check §四-8～9）合并讨论 |
| V2，待起 B1（方向 2026-10-10） | 微信媒体接入：图片走 CDN 下载 + AES 解密后作图片输入，表情包/GIF 等非 PNG/JPEG/WebP 内部转换；文件与视频先存入按日期分目录的临时目录，再落占位（含路径）供模型用文件工具读取；无转写语音维持占位。出站：bot 向微信发文件、表情包（协议已有 getuploadurl + CDN 上传，待评估）。待定：临时目录位置与清理、尺寸上限、转换失败处置 | [`docs/blueprints/wechat-channel.md`](docs/blueprints/wechat-channel.md)；协议见 [`wechat-protocol.md`](docs/brainstorm/wechat-protocol.md) |
| 待查（2026-10-09） | 微信引用回复：`ref_msg` 仅校验，被引用内容未进入记录，模型不知用户引用了哪句 | [`docs/blueprints/wechat-channel.md`](docs/blueprints/wechat-channel.md) |
| 想法，未定 | 人设绑定 UI 主题；「理性大脑」人设定稿 | [`docs/brainstorm/runtime-settings.md`](docs/brainstorm/runtime-settings.md) §三 |
| 已知缺口，待定义验收边界 | 入站去重、投递重复；模型/工具/历史尺寸、工具授权与进程退出 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-3、8～9 |
| 待运行时骨架 | 日志路径与级别约定，定稿后补进 `CLAUDE.md` | — |
