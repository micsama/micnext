# B2：SQL 页可切换到白名单里的其他 SQLite 库

状态：已批准（2026-11-16），实现完成，待验收。

## 为什么要做

同机的其他服务（目前是 micjz）也用 SQLite，想直接用现成的「开发者诊断 → SQL」查改，省得各自再写诊断页。micnext 只认路径，不了解对方的业务。

## 行为

- `config.toml` 的 `[gateway.sql_databases]` 按「名字 = 路径」列出额外的库，网页上不能改。没配就和现在完全一样。
- SQL 页在配置了额外库时显示库选择器，默认是 micnext 主库；切换后表结构、执行、把关都作用于所选库，编辑器内容保留。
- 额外库与主库同一套规则：单条语句、200 行 / 1 MiB、2 秒超时，复制 Markdown、下载 CSV；不能 ATTACH。
- 写入同样先 AI 把关、确认后备份再执行。备份放在 micnext 自己的 `sql-backups/`，文件名 `<库名>-<时间>.db`，与主库一样每个库保留最近 10 份。
- 额外库没有凭据列，不做遮蔽；表结构平铺，不分组、无说明。
- 全局同一时间只能有一个查询，不分库。
- 额外库每次操作时才打开，用完关闭，不常驻：只读查询用 `SQLITE_OPEN_READ_ONLY`，写入用读写方式但不带 CREATE；`busy_timeout` 取 2 秒，对方正在写时最多等这么久。
- 路径不存在或打不开，只有这次操作报错「打不开数据库 <名字>：<原因>」，不影响启动和其他库。启动时只检查配置：名字只能用字母、数字、下划线，不能是 `micnext`；路径必须是绝对路径或以 `~/` 开头。

## 契约

- **配置**：`[gateway.sql_databases]`，值为路径字符串，解析进 gateway 的 `Config`。
  ```toml
  [gateway.sql_databases]
  micjz = "/abs/path/micjz.db"
  ```
- **HTTP**：
  - `POST /developer/sql`、`POST /developer/sql/review` 请求体加可选 `db: string`，`GET /developer/sql/schema` 加可选查询参数 `?db=`。不传就是主库，名字不在白名单里返回 400。
  - 新增 `GET /developer/sql/databases`，返回额外库的名字列表 `string[]`，前端据此决定是否显示选择器。
- **mic-store**：诊断相关的查询、写入、试运行、表结构方法都加参数 `db: SqlDb`，`pub enum SqlDb { Main, External { name: String, path: PathBuf } }`；`DiagnosticError` 加 `Open(String)`。执行、授权、截断、超时和备份逻辑两种库共用，只有连接来源不同。
- **mic-core**：`review_sql` 与诊断转发方法透传 `SqlDb`。
- **Web**：`runSql`、`reviewSql`、`getSqlSchema` 加 `db` 参数，`SqlTab` 加选择器。
- 不改数据库结构，不加依赖。

## 实现记录

- 全局互斥复用主库只读连接的锁：查询、试运行、写入都先 `try_lock`，占用时返回「已有查询在执行」。主库的写入与试运行也纳入互斥，之前它们不和只读查询互斥。
- 额外库打开后先读一次 `sqlite_schema`，让「不是数据库」「文件不存在」都在打开阶段报 `Open`；网关映射为本地路径类错误。
- 把关提示按目标库换开头：主库仍介绍 micnext，额外库只说明是同机另一个服务的库。
- 执行中禁用库选择器，避免旧库结果落到新库页面；换库清空结果、把关和错误，编辑器保留。

## 不做

- 不跨库 JOIN，不读对方日志，不做迁移，不涉及对方业务（不为它登记表说明）。
- 不常驻额外库的连接，不在启动时探测库是否可用。

## 验收

- 本地：fmt、clippy、`bun run check`；用临时 WAL 库试只读查询、写入备份与按库保留份数、库文件挪走后报错、白名单外的名字返回 400。
- human：服务器配上 micjz 后，`SELECT * FROM entries LIMIT 10` 和表结构正常；写入走把关和备份；挪走库文件后报错，其他功能正常。
