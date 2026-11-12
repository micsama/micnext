# B2：web-guide 扩为 micnext-guide（网页功能 + 能力与实现索引）

状态：已实现，待验收。

来源：human 2026-11-12。现在的 `web-guide` 只能回答「入口在哪、怎么用」。agent 拿它做功能对比、开发维护或排查问题时，缺两样东西：产品能力全景和源码入口。结果每次（包括子 agent）都要从头翻源码。修订 [builtin-skills](builtin-skills.md)。

## 结构

合成一个内置 skill `micnext-guide`，不保留 `web-guide`。只回答「是什么、在哪里」，不写操作规程和开发规范。文档放在仓库 `docs/guide/`，和源码一起维护：

```text
docs/guide/
├── SKILL.md          # 一句定位、两份索引的链接、索引所在位置
├── capabilities.md   # 能力与边界：一张表，每项能力一行
└── code-map.md       # 实现索引：分层与关键约束一小节，能力 → 关键入口文件与职责一张表
docs/web-features.md  # 网页功能地图，接在 SKILL.md 后面作为正文
```

- **正文**：`SKILL.md` 加上 `docs/web-features.md`，编译期拼接；问「在哪、怎么用」加载一次就能答。`docs/web-features.md` 原位不动，仍是网页使用的唯一来源。
- **两份索引按需读**：加载结果带上所在目录，模型用读文件工具读，和磁盘 skill 的附带文件一样。
- **每句话不是产品事实，就是实现定位**。不写阅读分流、派任务方式、维护规程；网页入口只在 web-features，工具参数看工具 schema，实现细节看源码。
- **code-map**：每项只列少数关键入口的完整路径，文件名说不清职责时才补符号；不列全部组件、存储与测试文件，不记行号。

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
- **校验**：mic-gateway 加一个测试，检查 code-map 里反引号包着的路径都存在。只校验路径，行为是否准确靠改功能时顺手核对。
- **维护**：CLAUDE.md「流程」里加「地图同步」一条：功能变化同步 web-features，能力边界变化同步 capabilities，实现入口或分层变化同步 code-map。
- 无存储变更，无 HTTP 形状变更。

## 不做

- 不按功能一篇篇拆文档，不单独写架构流程文档；索引不复述源码。
- 不写未来规划。规划留在 `docs/brainstorm/` 和 `todo.md`。

## 验收

- **本地**：`cargo fmt`、`clippy`、`cargo test`（含路径校验）。用 `-p` 确认目录里只有 `micnext-guide`，加载结果带目录和两份索引。
- **human**：
  - 问「某某功能在哪」，确认先加载了 micnext-guide 再回答。
  - 让 agent 改一个小功能或排查一个问题，看它是否先读 code-map 再定位源码。
