# micnext 指南

micnext 是单进程的个人 Rust agent：
- 常驻 Gateway，内嵌 Web 页面。
- Channel 目前接入微信。
- 支持定时任务和主动推送。
- 一个用户（person），多端共用同一套会话与设置。

本指南只做导航，实现以源码为准。指南与源码不符时，以源码为准，并顺手修正指南。

## 先读哪里

| 要做的事 | 先读 |
|---|---|
| 回答功能在哪、怎么用 | 下文「Web 功能地图」，已在本正文里 |
| 回答能不能做到、边界在哪，或与其他 agent 做功能对比 | `capabilities.md` |
| 改功能、修 bug | 先在 `capabilities.md` 找到所属能力，再按 `code-map.md` 对应小节去读源码 |
| 排查跨模块问题（消息没出现、推送没到、run 卡住） | 先读 `architecture.md` 的相关流程，再按 `code-map.md` 定位 |

- 附带文档在 Skill directory 下。源码仓库根目录是它的上两级（`../..`），`code-map.md` 里的路径都相对仓库根目录。
- 加载结果里没有 Skill directory 时，说明当前部署没有源码仓库，只能用本正文作答。
- 设计决定与契约在仓库的 `docs/blueprints/`（已关闭的在 `archive/`），未定方向在 `docs/brainstorm/`，开发约定在 `CLAUDE.md`。

## 派给子 agent

子 agent 不会继承你已加载的内容。把开发或排查任务派给子 agent 时，在任务里写明：
- 源码根目录；
- 任务属于哪项能力；
- 要先读的指南文件及小节（写绝对路径）；
- 已知的代码入口；
- 怎么验证。

子 agent 沿着索引读源码；只有索引缺失或与实现不符时，才扩大搜索。

## 维护

各份文档只写自己这一层，其余地方只引用：
- 网页怎么操作：`docs/web-features.md`，即下文「Web 功能地图」。
- 能力与边界：`capabilities.md`。
- 模块关系、主流程与不变量：`architecture.md`。
- 源码与测试入口：`code-map.md`。写路径、职责和关键符号，不写行号，不抄代码。

功能改动时同步对应的那一份：用户行为变了改前两份，入口变了改 code-map，跨模块流程变了改 architecture。没有核实的内容写「待核实」，不把计划写成已有功能。

---
