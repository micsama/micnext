# B2：web-guide 扩为 micnext-guide（产品能力 + 网页使用 + 架构 + 源码导航）

状态：已实现，待验收。

来源：human 2026-11-12。现在的 `web-guide` 只能回答「入口在哪、怎么用」。agent 拿它做功能对比、开发维护或排查问题时，缺三样东西：产品能力全景、跨模块流程、源码入口。结果每次（包括子 agent）都要从头翻源码。修订 [builtin-skills](builtin-skills.md)。

## 结构

合成一个内置 skill `micnext-guide`，不保留 `web-guide`。文档放在仓库 `docs/guide/`，和源码一起维护：

```text
docs/guide/
├── SKILL.md          # 定位、阅读导航、维护约定（skill 正文开头）
├── capabilities.md   # 按用户目标组织的能力地图：能做什么、行为与限制
├── architecture.md   # 模块职责与关系、3～5 条主流程、容易踩坑的不变量
└── code-map.md       # 能力 → 前端 / 服务端 / 存储 / 测试入口（路径 + 职责 + 关键符号）
docs/web-features.md  # 网页使用指南，接在 SKILL.md 后面作为正文第二部分
```

- **正文**：`SKILL.md` 加上 `docs/web-features.md`，编译期拼接。平时问「在哪、怎么用」，加载一次就能答，和现在一样。
- **`docs/web-features.md` 原位不动**：CLAUDE.md 照旧引用它，仍是网页使用的唯一来源。
- **另外三份按需读**：skill 加载结果里带上它们所在目录，模型用读文件工具去读，和磁盘 skill 的附带文件一样。
- **各份只写自己那一层**，其余引用：
  - 网页操作 → web-features
  - 能力边界 → capabilities
  - 流程约束 → architecture
  - 源码路径 → code-map
- **code-map 的写法**：不记行号、不抄代码、不列完整目录树。没核实的写「待核实」，不把计划写成已有功能。
- **首批覆盖的主线**：
  - 会话与 agent 执行
  - 工具、后台任务与子 agent
  - 定时任务
  - 推送与通知
  - 模型、人设与上下文管理
  - 渠道（Web、微信）
  - 设置与诊断

## 契约

- **mic-core**：`Registry::skill` 增加参数 `dir: Option<PathBuf>`。
  - 有目录时，加载结果和磁盘 skill 一致：正文之后带 `Skill directory:` 和文件清单。
  - 没有目录时只返回正文，和现在一样。
  - 目录行（`<skills>`）同理：有目录时带 `dir`。
- **mic-gateway**：
  - 登记 `micnext-guide`，目录为 `[gateway] update_repo` 下的 `docs/guide`。
    - 没配 `update_repo` 时不带目录：服务器上没有源码，code-map 本来也用不上。此时 `SKILL.md` 里写明「附带文档需在源码仓库中查看」。
  - 描述：`micnext 的产品能力、网页使用、系统架构与源码导航。回答功能在哪、怎么用，或做功能对比、开发维护、问题排查之前先加载，不凭印象答。`
  - Web channel prompt 里的 `web-guide` 改成 `micnext-guide`。
- **派发约定**：`SKILL.md` 里写明，把开发或排查任务派给子 agent 时，在任务里给出这份指南的路径和相关小节，不假定子 agent 已加载过它。
- **校验**：mic-gateway 加一个测试，检查 code-map 里反引号包着的路径都存在。只校验路径，行为是否准确靠改功能时顺手核对。
- **维护**：CLAUDE.md「设计」或「流程」里加一句：
  - 功能改变时，同步 web-features。
  - 能力边界或入口变了，同步 capabilities 和 code-map。
  - 跨模块流程变了，同步 architecture。
- 无存储变更，无 HTTP 形状变更。

## 不做

- 不按功能一篇篇拆文档。哪份长到加载成本明显偏高，再拆。
- 不写未来规划。规划留在 `docs/brainstorm/` 和 `todo.md`。

## 验收

- **本地**：`cargo fmt`、`clippy`、`cargo test`（含路径校验）。用 `-p` 确认目录里只有 `micnext-guide`，加载结果带目录和三份文件。
- **human**：
  - 问「某某功能在哪」，确认先加载了 micnext-guide 再回答。
  - 让 agent 改一个小功能或排查一个问题，看它是否先读 code-map 再定位源码。
