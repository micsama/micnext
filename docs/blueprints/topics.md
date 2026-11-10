# B2：专题

状态：待批准。

来源：[B1](../brainstorm/topics.md)。本篇只做第 3 节的方案 A（索引加 `topic` 工具）和沉淀方案 a（对话中随写）。压缩时派出 fork 整理见 B1 第 4 节，另立 B2；专题页和托管网页不在本篇范围内。

## 行为

- **来源**：`<data_dir>/topics/<name>/TOPIC.md`，一个目录一个专题，数据、脚本、网页等文件放在同一目录。
  - frontmatter 只读 `description`，其余字段忽略。
  - 正文是「现状」：当前结论、已做决定、未决问题，以及目录内资料的导读。
  - `name` 即目录名，命名规则和 skill 相同，描述上限 512 字符。
  - 不合法的专题不进索引。和 skill 一样，`topics/` 不存在视为没有专题；其他 I/O 错误如实让 run 失败。
- **新建与维护**：模型用现有文件工具创建和修改专题。约定写在索引的说明文字里（不进 system.md）：
  - human 提出长期话题时，可以建议在 `topics/` 下新建专题；
  - 已打开某个专题后，出现新结论或新数据就更新该专题的现状或文件（对应方案 a）。
- **索引**：run 开始时扫描一次，放进 system prompt 的 `context` 块，紧跟在 Skill 目录之后。

  ```
  <topics>
  <topic name="house" dir="/…/topics/house">description</topic>
  </topics>
  ```

  没有专题时只保留说明文字和 `topics/` 路径，否则模型无从建议第一个专题；文件没变时文本不变，不破坏缓存。说明文字还写明：话题与某个专题相符时，先用 `topic` 打开再聊。
- **`topic { name }` 工具**：主会话、定时任务会话和子 agent 都提供。返回内容依次是：
  1. 现状正文；
  2. 目录路径；
  3. 目录内其余文件清单，最多 50 个。

  名字不存在时返回输入错误，并列出可用的专题。微信和 Web 没有专门入口，human 说清楚话题即可。
- **占位与压缩**：`topic` 结果按普通工具输出处理：冷整理和压缩保留段照常占位，压缩摘要照常提取任务需要的结论。调用记录里留着专题名，需要原文时重新打开即可。本篇不在压缩时更新专题文件；压缩时如何整理专题，见 B1 第 4 节，另立 B2。
- **会话关联**：不新建表，「打开过哪些专题」从 `topic` 调用推导，供 B1 第 4 节使用。

## 契约

- **core**：
  - 从 `skills` 抽出私有模块 `docdir`：「一个目录一个条目、入口文件带 frontmatter」的共同规则，即命名校验、frontmatter 解析、description 规整（上限由调用方传入）、目录遍历和文件清单、I/O 错误。命名规则因此只有一处。`SkillsError` 随之改名 `DocDirError`，`KernelError::Skills` 改为 `DocDir`，Gateway 映射不变，专题不新增错误类型。
  - `skills` 和新增的 `topics` 只保留各自的语义：skill 有 `/name`、压缩后重载、内置 skill、与内置指令重名检查；专题有现状正文和 `topic` 工具。
  - 新增 core 工具 `topic`。run 开始时扫描失败，以 `ProviderFailed` 结束；`topic` 读取失败，返回 dependency 错误，均同 skill。
  - `PromptPartKind`、`ContextKind` 各新增 `Topics`，上下文构成里单列「专题索引」。
  - `limits.rs` 新增专题描述上限 512（skill 仍是 1024）；`SKILL_FILES_LISTED` 改名为两者共用的 `DOCDIR_FILES_LISTED`。
- **Web**：上下文构成新增「专题索引」一类；`topic` 工具行沿用通用工具展示。不新增页面和接口。
- **存储**：无结构变更。

## 验收

- 本地：`cargo fmt`、`clippy`、`bun run check`。手写一个专题，用 `-p` 验证：模型能看到索引并调用 `topic`；模型按约定把新结论写进 `TOPIC.md`；冷整理后 `topic` 结果换成占位。
- human：在 Web 聊一个专题，再到微信里提起同一话题，看能否自动打开并接上。
