# todo 索引

活跃条目按状态列出，正文放各自的 B1/B2 文档，这里只留一行指针。收尾后的条目
挪去 [`todo-archive.md`](todo-archive.md)，不留在这里。

| 状态 | 主题 | 文档 |
|---|---|---|
| B1 余项：B（模型输入呈现）、F（工具参数）、E 余项（会话身份）待定；A 并入阶段二 | 最小正交审查余项 | [`docs/brainstorm/orthogonality-review-2026-09-24.md`](docs/brainstorm/orthogonality-review-2026-09-24.md) |
| B1 第二轮收敛中 | 产品定位、Web + 微信、定时看板、同步/异步与 v0–v3 路线；全项目待决策项见 §四 | [`docs/brainstorm/product-roadmap.md`](docs/brainstorm/product-roadmap.md) |
| 进行中（步 1～6 与存储重整完成，v0a Web 闭环打通） | v0a 模块地图：Web 闭环的模块划分、对接方式、实现顺序（v0a 总索引） | [`docs/blueprints/v0a-module-map.md`](docs/blueprints/v0a-module-map.md) |
| v0b，Web 打通后再做 | 微信配置保存后重启、凭据边界、Channel port 与投递完成判据 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §二-10～12、§四-1～3 |
| B1 待核对，再起 B2 | 定时任务看板事实来源与异步 shell/子 agent 生命周期 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-4～5 |
| 阶段二待细化再批 | 运行期设置阶段二：模型存库与 key 加密、Provider 工厂（正交审查 A2） | [`docs/blueprints/runtime-settings.md`](docs/blueprints/runtime-settings.md) §四 |
| 想法，未定 | 人设绑定 UI 主题；「理性大脑」人设定稿 | [`docs/brainstorm/runtime-settings.md`](docs/brainstorm/runtime-settings.md) §三 |
| 已知缺口，待定 | 模型条目误写成顶层段（如 `[openai] model = ...`）只报 serde 原文，未提示应写在 `[models.<名字>]`；改进需让装配识别模型模块（动 `Module` trait，走 B2） | [`mic-core-module.md`](docs/blueprints/mic-core-module.md) |
| 已知缺口，待定义验收边界 | 入站去重、投递重复；模型/工具/历史尺寸、工具授权与进程退出 | [`docs/brainstorm/cross-check-2026-09-23.md`](docs/brainstorm/cross-check-2026-09-23.md) §四-3、8～9 |
| 待运行时骨架 | 日志路径与级别约定，定稿后补进 `CLAUDE.md` | — |
