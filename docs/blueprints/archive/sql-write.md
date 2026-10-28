# B2：开发者 SQL 写入模式与 AI 把关

状态：CLOSED（2026-10-28），已在线上改会话目录时验收。

来源：换系统用户后，需要批量改会话目录 `core_sessions.pwd`。线上 SQL 页只读，只能停服务后用 sqlite3 改。

## 行为

- **写入模式开关**：SQL 页新增「写入模式」开关，默认关，刷新页面后回到关。
  - 关：行为与现在一致，只读。
  - 开：编辑区加红色粗边框，工具栏变红，执行按钮变成红色「把关」。切换只换颜色、不增加行，布局不跳动。完整提醒放在把关卡片里：「确认后先备份整个数据库，再直接改主库。改动立即生效，已打开的页面不会自动刷新，部分设置可能要重启才生效。改错时，停服务后用备份文件替换 micnext.db。」
- **每次执行先备份**：写入模式下每次执行，先用 `VACUUM INTO` 把数据库备份到 `<数据目录>/sql-backups/micnext-<时间>.db`，备份成功才执行。只保留最近 `SQL_BACKUP_KEEP = 10` 份（mic-store `limits.rs`），更旧的自动删掉。备份失败就不执行，直接报错。
- **能做什么**：
  - 允许：读、`INSERT`、`UPDATE`、`DELETE`，以及 `RETURNING`。
  - 不允许：建表、删表、改表、`PRAGMA`、`ATTACH`、事务控制。
  - 仍然一次只执行一条语句，超时规则不变。
  - 凭据列照旧读作 NULL，也不允许写入。
- **在哪执行**：在业务连接上执行，和业务写入排队，不会冲突。整条语句放在一个事务里，出错或超时就回滚，不会留下改了一半的数据。
- **结果显示**：显示「影响 N 行 · 已备份 micnext-xxx.db」。带 `RETURNING` 时照常显示返回的行。
- **执行前把关**：写入模式下点「执行」不会直接执行，而是先把关，再由你确认。
  - **试运行**：在业务连接上开事务执行这条语句，记下影响行数和涉及的表，然后回滚。表名从授权回调里取，不靠解析 SQL。试运行同样不允许建表、`PRAGMA` 等语句，不允许的语句在这一步就被拒。
  - **AI 评估**：把以下内容交给标题模型，返回风险等级（低 / 中 / 高）和一两句理由：
    - 这条 SQL；
    - 试运行影响的行数；
    - 涉及的表的 DDL（取自 `sqlite_schema`，含这些表上的索引和触发器）和表说明。

    不发送任何数据行。
  - **确认执行**：页面显示把关卡片，内容是影响行数、风险等级和理由，等级越高颜色越红，再点「确认执行」才真正备份并执行。确认时会再执行一次，实际影响行数以那次为准。
  - **超时与记账**：AI 评估最多等 `SQL_REVIEW_TIMEOUT = 60` 秒。这次调用不属于任何会话，不写入模型调用记录。
  - **没有标题模型或 AI 调用失败**：卡片只显示试运行行数，并写明「未经 AI 评估」和原因，仍可确认执行。这是可见的降级，需要你同意。
- **暂不做**：
  - 在页面上恢复备份。
  - 一次执行多条语句。
  - 改表结构。
  - 只读查询的 AI 解读。
  - 用 AI 生成 SQL。

## 契约

- **HTTP**
  - `POST /developer/sql` 的请求体新增 `write: bool`，缺省为 `false`。写入模式的响应新增 `changes`（影响行数）和 `backup`（备份文件名），只读模式下两者都是 `null`。
  - 新增 `POST /developer/sql/review`：请求 `{ sql }`，返回 `{ changes, tables: string[], risk: "low" | "medium" | "high" | null, reason: string | null, ai_error: string | null }`。`risk` 为 `null` 时，`ai_error` 写明没评估的原因。
- **mic-store**
  - 新增 `diagnostic_write(sql, at) -> WriteResult { result: QueryResult, changes: u64, backup: String }`，在业务连接上先备份再执行。授权规则在只读白名单的基础上，只多放开 `INSERT`、`UPDATE`、`DELETE`，凭据列不能写。
  - 新增 `diagnostic_dry_run(sql) -> DryRun { changes: u64, tables: Vec<String>, ddl: String }`，授权规则同上，执行后回滚。
  - `DiagnosticError` 新增 `Backup(String)`。
- **mic-core**
  - 新增 `Kernel::diagnostic_write`，直接透传给 store。
  - 新增 `Kernel::review_sql(sql) -> SqlReview`：先试运行，再用标题模型评估。复用标题模型的选择和调用路径，提示词放在 core 里。
  - `limits.rs` 新增 `SQL_REVIEW_TIMEOUT`。
  - 新增 `SqlReview { changes, tables, verdict: Result<(SqlRisk, String), String> }` 和 `SqlRisk`。
- **Web**
  - `SqlTab.svelte`：新增写入开关、红框、把关卡片（含完整提醒）和「确认执行」按钮，结果区显示影响行数和备份文件名。

## 验收

- 自动：在 store 里写一个模块测试，覆盖四点：
  - 写入模式执行 `UPDATE` 后，生成备份文件且数据已改；
  - 建表、`PRAGMA`、写凭据列被拒；
  - 试运行返回正确的行数和涉及的表，并且数据没有改动；
  - 超过保留份数时，删掉最旧的备份。
- 人工：
  - 线上打开写入模式，执行会话目录迁移 SQL，看把关卡片后确认执行；
  - 再试一条不带 `WHERE` 的 `DELETE`，看是否被评为高风险；
  - 确认数据目录下生成了备份文件，会话能正常进入新目录。
