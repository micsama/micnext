# B2：内置 skill（首个：web-guide）

状态：待批准。

## 动机

Web 功能地图（`docs/web-features.md`）现在作为 Web channel prompt 整段放进每次请求，有两个问题：大多数对话用不到，却一直占着上下文；微信会话里根本没有这份地图，用户在微信里问「定时任务在哪改」，模型只能凭印象回答。改成随二进制发布的内置 skill 后，平时只在目录里占一行，需要时再加载，所有渠道共用这一份。

## 行为

- **来源**：模块在 `install` 里登记内置 skill，提供名称、描述和正文，正文用 `include_str!` 嵌入。内置 skill 没有目录，用户不能修改或删除。
- **目录**：和磁盘 skill 一起按名称排序进入 `<skills>`。内置 skill 的那一行不带 `dir`：`<skill name="web-guide">…</skill>`。
- **加载**：`skill` 工具、`/name` 点名、压缩后按 `required_skills` 重新加载，都和磁盘 skill 走同一条路径。加载结果只有正文，没有 `Skill directory` 行和文件清单。
- **重名**：两个内置 skill 重名，启动时报错，和 `channel_prompt` 重复声明的处理一样。磁盘 skill 和内置 skill 重名时，磁盘那个计入非法列表，原因写「与内置 skill 重名」，和现在跟内置指令重名的处理一致。
- **`GET /api/skills`**：内置 skill 也放在 `skills` 里返回，补全列表照常显示，不新增字段。

## web-guide

- 由 Gateway 登记，正文就是 `docs/web-features.md`，仍是唯一来源，CLAUDE.md 照旧引用这份文件。
- 描述：`micnext 网页功能地图。回答功能在哪、怎么用之前先加载，不凭印象答。`
- Web channel prompt 不再带整份地图，只保留页面索引：

  ```
  Web：支持 Markdown，工具活动实时可见。页面有会话、定时任务、设置、Trace、开发者诊断；功能在哪、怎么用先加载 `web-guide`。
  ```

- 微信 channel prompt 不变。目录里有 web-guide 的描述就够了。

## 契约

- **mic-core**：新增 `Registry::skill(name: &'static str, description: &'static str, body: &'static str)`。名称和描述沿用磁盘 skill 的校验规则；不合法时在启动阶段报错，因为这属于开发错误。
- **mic-core 私有**：`Skills` 持有内置 skill 列表；`list`、`load`、`catalog`、`expand` 合并两类来源。
- **mic-gateway**：web channel prompt 换成上面的索引，并登记 `web-guide`。
- 无存储变更，无 HTTP 形状变更。

## 不做

- micnext 自运维 skill（数据目录、日志库、安全改自身数据库、更新）：等写出具体内容、有了明确场景再另行登记。本篇只把机制和 web-guide 做完。

## 验收

- 本地：`cargo fmt`、`clippy`、`cargo test -p mic-core`。用 `-p` 确认目录里有 web-guide，且模型能加载。
- human：在 Web 和微信各问一次「某某功能在哪」，到 Trace 里看是否先加载了 web-guide 再回答。
