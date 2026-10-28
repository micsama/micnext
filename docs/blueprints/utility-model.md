# B2：标题模型改名为杂活模型

状态：已批准（2026-10-28），实现完成，待验收。

来源：[B1](../brainstorm/input-prediction.md) 第一节。

## 行为

- 「标题模型」这份配置现在要服务三个用途：标题与标签生成、SQL 写入把关，以及之后的输入预测。为此把它改名为「杂活模型」（`utility_model`）。
- 用户能看到的变化只有设置页：
  - 分组标题改为「杂活模型」；
  - 说明里列出它目前负责哪些事；
  - 已选的模型和思考强度原样保留。
- 标题生成本身不改名，调用用途、`TitleCall`、`TitleOutput` 和 `title` 模块都还叫 title，因为它只是杂活模型的一个用途。

## 契约

- **数据库**：
  - core schema 升到 v30；
  - `core_settings.title_model_id` 改名为 `utility_model_id`，`title_reasoning_effort` 改名为 `utility_reasoning_effort`；
  - 转换脚本是 `scripts/db/core-30.sql`，用 `ALTER TABLE … RENAME COLUMN`，CHECK 约束会跟着改。
- **mic-store**：
  - `TitleModelSelection` 改为 `UtilityModelSelection`；
  - 方法 `title_model` 改为 `utility_model`，`set_title_model` 改为 `set_utility_model`，`title_model_snapshot` 改为 `utility_model_snapshot`；
  - 这几个方法从 `title.rs` 移到 `utility_model.rs`。
- **备份**：
  - `ExportPreferences.title_model` 改为 `utility_model`，`title_reasoning_effort` 改为 `utility_reasoning_effort`；
  - 不兼容旧字段名。旧备份文件需要手动把这两个字段改名后才能导入，导入时会提示是哪个字段不认识。
- **mic-core**：
  - kernel 的 `title_model` 改为 `utility_model`，`set_title_model` 改为 `set_utility_model`；
  - `title::build_provider` 移到新模块 `utility.rs`，标题生成和 SQL 把关都从这里取 provider。
- **HTTP**：`/settings/title-model` 改为 `/settings/utility-model`，请求体和响应格式不变。
- **Web**：
  - `TitleModelSection` 改为 `UtilityModelSection`；
  - `getTitleModel` 改为 `getUtilityModel`，`putTitleModel` 改为 `putUtilityModel`；
  - `settingsStore.titleModel` 改为 `settingsStore.utilityModel`。
- **脚本**：`scripts/check-session-title.py` 里的接口路径跟着改。

## 验收

- 自动：
  - `scripts/db/tests.py` 覆盖 v29 到 v30 的转换，确认已选的模型和思考强度不丢；
  - 现有的标题和备份测试随改名更新后通过。
- 人工：服务器更新后打开设置页，确认杂活模型仍是原来选的模型；生成一次标题、做一次 SQL 把关，都能正常完成。
