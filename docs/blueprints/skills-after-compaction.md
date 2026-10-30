# B2：压缩后重新加载仍在用的 skill

状态：实现完成，待验收（2026-10-30）。方向见 [B1](../brainstorm/skills-after-compaction.md)，human 选 B（自动加载）。

## 行为

- **判断标准**：只看当前有没有未完成的任务正在依赖某个 skill。有就重新加载；没有就丢弃，拿不准时也丢弃。之后真要用，模型会自己从目录里加载。
- **总结提示**（对话和定时任务两套都改）：
  - skill 正文不写进摘要，只记用了哪个 skill、做了什么。
  - 末尾可选 `<required_skills>` 块，一行一个名字，只列当前未完成任务依赖的 skill。
  - 两个清单块都放在摘要末尾，顺序不限。
- **重新加载**：压缩提交时，在边界之后、压缩通知之前，每个 skill 追加一条新消息，内容和 `/name` 点名的加载结果相同（正文、目录、文件清单）。
  - 压缩边界本身不变，只有摘要、保留段和所需文件，压缩结果保持幂等。
  - 这些消息和普通历史一样：下次压缩或冷时整理时按 skill 正文规则换成占位，再由当时的总结决定还要不要。
  - 名字不在目录里、不合法或读取出错的，跳过，在压缩通知里写明「skill x 未带上：原因」。不让压缩失败。
- **压缩通知**：带上了 skill 时，补一句「重新加载 N 个 skill」。
- **撤销压缩**：重新加载的消息留在原处。撤销后窗口里会同时有原正文和这份重载，多占一份；下次压缩或整理时会被换成占位。接受这点，不为撤销另做关联。

## 契约

- **mic-message**：`MessageBody` 新增 `SkillLoad { name: String, text: String }`，表示框架代为加载的 skill，不投递。
  - 模型视图和 `ContentPart::Skill` 一样，渲染成 `<skill name="…">\n加载结果\n</skill>`。
  - `elide_output` 对它生效，占位文字和其他 skill 正文相同。
- **mic-store**：`Compaction`（边界内容）不变；`commit_compaction` 增加参数 `skills: SkillReload { loaded: Vec<(String, String)>, skipped: Vec<(String, String)> }`，分别是（名字，加载结果）和（名字，失败原因）；`session` 参数与 `call.session_id` 重复，一并去掉。同一事务里按顺序写入边界、各条 `SkillLoad`、压缩通知；通知仍由 store 生成，补上重新加载的数量和逐项失败原因。
- **mic-core**：
  - `compaction.rs`：两套总结提示改写；`parse_summary` 解析末尾的两个清单块；提交前用 `skills` 模块生成加载结果；`elidable` 把 `SkillLoad` 归为 skill 正文。
  - 上下文构成：`SkillLoad` 在模型视图里是用户侧消息，计入「对话」，和点名加载一致；工具加载的仍计入对话及其工具子项。「Skill」一类仍只统计 system prompt 里的目录，不改口径。
- **Web**：`SkillLoad` 显示成可折叠的「已重新加载 skill name」，复用点名 skill 的折叠块。
- **无数据转换**：新增的消息种类不影响已有数据。

## 验收

- **自动**：
  - `parse_summary`：两个块各自出现、同时出现且顺序不同、都不出现。
  - 压缩模块测试：清单里有一个已加载的 skill 和一个不存在的名字。压缩后边界之后有前者的 `SkillLoad`，下一次请求带着它的正文；通知里写明后者没带上。
- **人工**：在 Web 上 `/name` 加载一个 skill，任务做到一半时 `/compact`，看到「已重新加载」并且模型能接着做；换一个已经做完的任务再压缩，不重新加载。
