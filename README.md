# micnext

单进程个人 Rust agent：常驻 Gateway（内嵌 Web）与 Channel，也可用 `-p` 一次性调试。

## 启动

需要 Rust 和 Bun，在仓库根目录运行：

```sh
./build.sh                         # 构建前端与 release 二进制
./target/release/micnext            # 常驻，终端打印带 token 的 Web 地址
./target/release/micnext -p '你好'   # 一次性调试
```

首次启动自动生成 `~/.config/micnext/config.toml`（支持 `XDG_CONFIG_HOME`）；也可用
`--config <路径>` 指定已有配置。默认监听 `127.0.0.1:7878`。
在 Web「设置 → 服务商与模型」添加服务商、凭据和模型后开始对话。

## 基础功能与入口

| 功能 | 入口与用途 |
|---|---|
| 对话 | Web 新建会话，选择模型、人设和推理强度；支持图片、流式回复、工具结果、历史分页与会话归档 |
| 上下文 | 自动压缩；`/compact [文字]` 手动压缩并处理附带文字，`/clear` 清空模型上下文但保留聊天记录，`/stop` 停止当前回答 |
| 工具与后台任务 | 文件读写、编辑、搜索，Bash、网页抓取与子 agent；后台任务条查看输出、停止任务或继续向子 agent 发消息 |
| 模型 | OpenAI / ChatGPT 订阅走 Responses；DeepSeek、Ollama 与通用兼容服务走 Chat Completions |
| 微信 | 「设置 → 微信」扫码配置；默认构建启用，支持文字、图片和文件入站 |
| 设置 | Web 管理对话偏好、人设、服务商与模型；「设置备份」导出、导入加密设置，包含 Channel 账号，不是聊天数据库备份 |
| Trace 与看板 | 侧栏切换「Trace」，查看会话树、运行、模型调用、工具执行和请求消息重建；「看板」查看用量、费用与耗时统计 |
| 日志 | 侧栏底部「开发者诊断 → 日志」，查看框架运行信息与错误 |
| SQL | 「开发者诊断 → SQL」（`/developer/sql`），直接查询业务 SQLite，查看表结构，复制结果为 Markdown 或下载 CSV |
| 更新 | 配置 `gateway.update_repo` 后，在「开发者诊断 → 更新」检查更新，以 upstream 覆盖部署仓库后构建并重启 |

## SQL 排查约定

**排查消息、任务、模型调用和压缩事实，优先使用现有 Web SQL 页面。**
每次执行一条完整的只读 SQLite SQL，可用 `WITH … SELECT …`；不要混入
`.headers`、`.mode`、`.parameter` 等 sqlite3 命令行指令，也不要依赖前一条语句设置变量。
使用 Ctrl/⌘ + Enter 执行，结果最多返回 200 行 / 1 MiB，查询超时为 2 秒，凭据列读作 `NULL`。

会话时间线按 `(created_at, id)` 排；`id` 表示写入先后，不能代替时间线顺序。
例如查看某个会话的最近 30 条聊天消息（把 `1` 换成会话 id）：

```sql
SELECT id, kind, created_at, payload
FROM (
    SELECT id, kind, created_at, payload
    FROM core_messages
    WHERE session_id = 1 AND kind <> 'SideOutput'
    ORDER BY id DESC LIMIT 30
)
ORDER BY created_at, id;
```

## 配置与数据

- `config.toml` 只放启动项；日常用户设置存 SQLite，经 Web 修改。
- 业务库默认 `~/.local/share/micnext/micnext.db`，可用 `core.data_dir` 调整；框架日志使用数据目录下独立的 `logs/` 数据库。
- API key 加密存库，主密钥默认在配置文件旁的 `master.key`；迁移数据时需一并保留。

## 开发

`./build.sh debug run` 构建并启动调试版。收尾运行 `cargo fmt`、`cargo clippy -- -D warnings`；
改前端再到 `web/` 下运行 `bun run check`。按变化选必要的功能测试，页面观感和真实环境由人工验收。

公开契约变更先写蓝图，批准后实现，流程见 [CLAUDE.md](CLAUDE.md)；
未关闭事项见 [todo.md](todo.md)，已完成事项见 [todo-archive.md](todo-archive.md)。
构建后可用 `./scripts/size.sh` 查看 release 体积（仅支持 macOS 单架构 64 位 Mach-O）；
`--bloat` 额外分析 crate 和大函数，需先安装 `cargo-bloat`。
