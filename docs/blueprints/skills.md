# B2：Skill 加载

状态：已批准（2026-10-28），实现完成，待验收。

来源：[B1](../brainstorm/skills.md) 第一、二节。缓存冷后整理历史另立 B2；关键词/正则自动加载暂不做。

## 行为

- **来源**：`<data_dir>/skills/<name>/SKILL.md`，一个目录一个 skill，附带的脚本和资料放在同一目录里。以后要支持会话工作目录下的 `.agents/skills`，只需多扫一个目录，本篇不做。agent 可以用现有 fs 工具自己创建或修改 skill。
- **格式**：遵循开放规范，frontmatter 只读 `name` 和 `description`，其余字段忽略；frontmatter 之后是正文。
  - `name` 必须与目录名一致：1–64 个字符，只含小写字母、数字和连字符，不以连字符开头或结尾，不含连续连字符（同规范）；且不能与内置指令（`compact`、`clear`、`stop`）重名。
  - `description` 不能为空，最长 1024 字符。
  - 不合法的 skill 不进目录。原因通过 `GET /api/skills` 返回，Web 补全里灰色显示，悬停可看原因。
  - description 里的换行（如 YAML 多行字符串）折叠成空格；正文保持原文。
- **扫描结果**：按名称排序，文件没变时目录文本不变，不破坏缓存。`skills/` 不存在视为没有 skill；单个 skill 缺 `SKILL.md`、frontmatter 解析失败或字段不合法，计入非法列表；权限等其他 I/O 错误如实返回，不当成空目录：组装请求时让这次 run 失败并显示原因，`GET /api/skills` 返回错误。
- **目录**：run 开始时按磁盘现状扫描一次，本 run 内各次请求共用；放进 system prompt 的 `context` 块，位置在环境信息之后、摘要之前。用 XML 列出，每个 skill 一行，带绝对目录，模型用 read 或 bash（`workdir`）时可以直接用：

  ```
  <skills>
  <skill name="pdf" dir="/…/skills/pdf">description</skill>
  </skills>
  ```

  没有 skill 时整段省略。skill 有增删或描述有改动时，前缀会失效一次，这一点接受。
- **加载结果**：工具加载和用户点名共用同一份加载结果，内容依次为：
  1. 正文原文；
  2. `Skill directory: <绝对路径>`；
  3. 目录内其余文件的相对路径清单，按路径排序，最多 50 个，超出部分注明省略了多少。

  正文里的相对路径以该目录为基准。
- **模型加载**：新增 core 工具 `skill { name }`，主会话、定时任务会话和子 agent 都提供，返回加载结果。子 agent 在自己的上下文里加载，不继承父会话已加载的正文。工具不做去重：说明里写明「已加载的 skill 在上下文里一直可见，不用重复加载；正文已不在当前上下文时（被替换成占位或压缩进摘要）再重新加载」。skill 不存在或不合法时，返回输入错误并列出可用的名称。
- **用户点名**：用户输入的第一段文字以 `/name` 开头，且 `name` 是 skill 时，core 生成加载结果，把这条输入存成 `[Skill { name, text }, 其余文字, 附件…]`，`text` 即完整加载结果（正文、目录、文件清单）。
  - `name` 取到第一个空白为止，整个词须是 skill 名，`/foo-bar` 不会匹配 `foo`；只认开头一个点名。
  - skill 目录存在但读不出（不合法或 I/O 错误）时，输入原样收下，同一事务附一条通知，写明失败原因，交给 agent 处理（例如修好 SKILL.md）。
  - 微信端同样可用。Web 输入框把 skill 和内置指令放在同一个 `/` 补全列表里，界面上区分两类。
  - 记录存的是模型当时看到的内容，之后磁盘上的文件改了，历史保持不变。
  - 名字不是 skill，也不是内置指令时，按普通文本发送，和现在一样。
- **压缩**：skill 工具结果照现有规则处理，保留段内超过阈值的旧结果换成占位。压缩时带回的用户原话里，`Skill` 部分只写成 `[skill：name]`，不带正文。

## 契约

- **mic-message**：`ContentPart` 新增 `Skill { name: String, text: String }`。
  - 模型视图渲染为 `<skill name="…">\n加载结果\n</skill>`。
  - Web 显示为可折叠的「已加载 skill name」。
- **core**：
  - 新增私有 `skills` 模块，负责扫描、解析、校验和生成加载结果；扫描结果分为合法列表与非法列表（附原因），按名称排序，I/O 错误单独返回。
  - `PromptPartKind` 新增 `Skills`；`ContextKind` 新增 `Skills`，上下文构成里单列「Skill 目录」。通过工具或点名加载的正文，仍分别计入对话里的工具部分和用户部分。
  - 新增 core 工具 `skill`，以及输入解析中的 `/name` 分支。
  - `KernelError` 新增 `Skills`（扫描 I/O 错误）。
  - `Kernel` 新增 `skills()`，供 Gateway 调用。
  - `limits.rs` 新增：文件清单上限 50、描述上限 1024。
- **依赖**：新增一个 YAML 解析库，用于 frontmatter。倾向 `serde_yaml_ng`，因为 `serde_yaml` 已停止维护。
- **Gateway**：新增 `GET /api/skills`，返回 `{ skills: [{ name, description }], invalid: [{ dir, reason }] }`；扫描出 I/O 错误时返回 500 和原因。
- **Web**：输入框补全加入 skill；消息里的 `Skill` 部分渲染成折叠块；上下文构成新增一类。
- **无存储结构变更**：`ContentPart` 本来就以 JSON 存在消息里，新增变体不涉及迁移。

## 验收

- 本地：`cargo fmt`、`clippy`、`bun run check`；手写一个 skill，用 `-p` 验证模型能看到目录并调用 `skill`。
- human：Web 和微信各试一次 `/name`；上下文构成里能看到「Skill 目录」；压缩后 skill 结果按规则变成占位。
