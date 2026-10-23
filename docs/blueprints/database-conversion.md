# B2：数据库转换移出 Rust 二进制

状态：已批准方向（human 2026-10-23：Rust 二进制保持洁净，转换使用独立脚本，由 build.sh 与网页更新调用）；已实现，待真实网页更新验收。

## 边界

Rust 只包含当前完整 schema、初始化与精确版本校验。删除历史升级链、旧消息字段的 serde 默认兼容、旧配置专用分支，以及不匹配时自动备份换库/删除日志库的路径。不做降级、数据猜测、自动删用户数据。后续格式变更直接改当前结构；有保留旧数据的需求时另写独立转换脚本，不把升级代码加回应用。

业务库仍用 schema_migrations 保存每个模块的当前整数版本，不再代表应用可执行迁移链。core 与微信各自的 schema/version 文本为版本号唯一来源，Rust 与外部脚本共同读取。日志库继续用自己的 user_version，版本不符返回错误，沿已有的可见日志存储失败路径处理，不自动重建。

## 当前公开契约与调用方

- mic-store：Migration 替换为 Schema，字段为 module、version（静态版本文本）、sql（当前完整建表 SQL）、secret_columns。每个模块只声明一份当前 schema；Store::open / open_in_memory 接收 &[Schema]。SchemaMismatch { module, db, expected } 替代 SchemaAhead / SchemaRebuild，已有版本必须完全相等。新库或新启用的模块直接建当前结构，不经过历史版本。
- mic-core：Registry::migrations 替换为 schemas；Assembly 不再自动换库重建。仓库内微信注册与测试同步更新，旧入口删除。
- mic-gateway：RestartRequest 新增 arguments: Vec<OsString>，executable 从二进制改为仓库 build.sh；更新仍统计实际 release 二进制大小、提交版本。main 只负责停止服务后 exec 该命令，携带实际业务库路径与原始启动参数，保留原工作目录，不执行或解析转换 SQL。
- Compaction.elided / quoted 和 ExportPreferences.push_channels 缺字段不再自动按旧格式补默认；当前格式正常读取。
- HTTP、会话执行与 Channel 业务不变。

## 构建、转换与重启

独立工具：scripts/db/convert.py（Python 3 标准库）与按模块/目标版本命名的 SQL。脚本不嵌入二进制。目标版本从当前 schema/version 读取，逐个核对从库版本到目标版本的步骤，打印版本差与步数。缺步骤、库超前或 SQL 失败直接报错，不猜、不跳版本、不删库。

- ./build.sh [debug]：先构建，再检查实际库；未运行时执行转换，服务持锁时明确输出仅检查、推迟执行。
- ./build.sh [debug] run [程序参数]：构建成功后转换，再启动程序。
- ./build.sh [debug] prepare [程序参数]：构建并只读检查，供网页更新使用。配置路径从原参数解析，支持显式 --config、XDG 与 core.data_dir。
- ./build.sh [debug] start --db <实际库路径> -- [程序参数]：不再构建，服务已停止后转换并 exec 新二进制。
- 网页更新：fetch/reset → prepare → 通知 main 停止 → 等 Assembly 退出释放锁 → exec build.sh start → 外部脚本转换 → exec 新二进制。构建/检查失败时旧服务继续运行；停止后的转换失败则命令非零退出，库回滚，由终端显示原因，人工处理，不自动启动不兼容的旧/新程序。

转换脚本使用同一个 micnext.lock 排斥运行中的应用；锁内重读版本，整次计划在同一 SQLite 事务中执行，提交前 foreign_key_check。没有旧库时不创建库，由新应用初始化。重复执行已经完成的版本无副作用。转换过程中退出时事务回滚；不自动恢复、重试或删除历史数据。

本次只保留实际旧库需要的 core v22 → v26 四步转换，位于 scripts/db/；更早的历史升级链删除，微信当前已是 v5，无旧版转换脚本。v23 任务来源匹配只使用同发起 run、同 wire id 的 Dispatched 结果，避免其他轮次引用同一个 job 时重复写入；同一来源的相同证据去重，真正歧义仍失败。

## 验证

- Store、Message、Core、微信模块测试：66 项通过，覆盖当前结构初始化/重开、版本不符拒绝且原数据不变、当前业务执行。
- 外部脚本：只读检查、转换后重复执行、运行中排斥、缺步骤/超前拒绝、跨步骤失败整体回滚，4 项通过。
- 实际 v22 业务库只读副本：4 步转换到 v26，微信 v5 不变，165 条消息与 6 个任务保留，外键检查通过；原库未改。
- cargo fmt、cargo clippy -- -D warnings、Web check、完整 debug prepare 构建通过；start 入口创建新库、转换后表结构与直接创建一致、直接启动旧版本库拒绝且版本不变均通过。
- debug 二进制扫描未发现历史转换 SQL 或 origin_tool_call_id 旧字段。
- 真实网页更新流程和停止后的转换失败终端呈现交 human 验收。
