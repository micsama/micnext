# B2：会话归档

**状态：按已批准的 [async-jobs](async-jobs.md) P3 更新实现（2026-10-16），待页面及真实环境验收。**

依据：[B1](../../brainstorm/session-archive.md)；2026-10-16 human 确认归档后停止整树，并拒绝继续执行。以下契约替代此前“只隐藏、不取消、仍可发送”的行为。

## 行为

- 侧栏只提供「归档」，保留会话、消息、调用与任务输出，不提供永久删除。
- 仅 Root 可归档，Web、微信、CLI 统一；不存在 404、非 Root 403，成功 204。重复归档成功，保留首次时间。
- 归档前确认会停止当前执行及后台任务；用任务列表与当前会话执行状态展示已知运行数量。成功后整页刷新，归档当前会话先回到 `/`。
- Kernel 先落根会话 `archived_at`，再取消标题、主/子 run 与全部后台任务，把未认领输入标 held。完成事实照常落盘，不唤醒。清理失败报错，归档标记不回滚。
- 标记只存根会话；后代执行入口沿祖先查询。入站消息/命令、派出/追问在写入边界拒绝；调度器不认领归档树的输入。无需额外归档状态机。
- 已知 URL 仍可读历史、trace、任务输出；执行接口返回 409。会话列表及详情增加 `archived_at: number | null`；聊天页展示已归档并隐藏输入框，禁用生成标题及任务停止按钮。
- 微信拒收归档会话的新消息或命令时写一条通知「会话已归档，不能继续发送消息。」，不进入 agent，不把拒收作为收发服务故障。

## 存储与调用方

- 沿用已有 nullable `core_sessions.archived_at`，不改 schema；`COALESCE(archived_at, at)` 保留首次时间，侧栏仍按 `IS NULL` 过滤。
- `Session` 增 `archived_at: Option<i64>`，会话列表/详情复用同一投影。子 session 本身可为空，是否允许执行按祖先标记判断。
- Store 增 `ensure_session_active(session)` 和 `subtree_sessions(root)` 供 core 执行入口与整树收尾使用；归档拒绝统一为 `StoreError::SessionArchived`，Gateway 映射 409。
- 普通停止键与 `/stop` 仍只停当前 run，未归档时不清理后台任务，排队输入可继续执行。归档整树收尾与普通停止语义分开。
- 手工恢复：`UPDATE core_sessions SET archived_at = NULL WHERE id = <根会话编号>;`。只清标记，held 输入不补跑；新输入可正常执行。Web 七天内恢复另起主题，当前不实现。

## 验收与边界

- 本期不新增常驻测试（human 2026-10-16 要求先不写测试）。实现过程中三项临时模块验收已通过后移除：整树取消/拒收/重复归档/手工恢复、进行中标题取消并记录调用、递归用量汇总；不代表页面或真实微信已验收。
- 本地收尾执行 fmt、clippy、前端检查。页面确认框、已知 URL 只读展示、真实微信拒收交给 human。
- 运行中归档的人工验收明确延期（human 2026-10-16），不阻塞本轮验收；后台 bash / 子 agent 的复现例子保留在 [async-jobs §P3.4](async-jobs.md#p34-延期人工验收human-2026-10-16-确认)，以后实际遇到或主动测试时再看，当前不算人工通过。
- 其它窗口仍沿用现有刷新时机，不新增实时归档事件或轮询；未刷新窗口的写请求由后端 409 拒绝。
- 下列记录属于旧版归档行为的历史验收，不再作为 P3 行为的验收依据。

### 本地验收记录（2026-10-13）

`cargo fmt`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace`、`web/` 下 `bun run check` 与 `bun run build` 通过。本项未新增单元测试。

[整体功能脚本](../../../scripts/check-session-archive.py) 的 4 组隔离 HTTP/SQLite/模拟 Provider 验收通过：执行中的正文与标题不取消、数据及 DTO 保持；新输入不清归档标记且重复归档保留首次时间；不存在/非 Root 的错误、Web/微信/CLI 归档及多页过滤；重启后标记保持、手工清标记恢复且历史保留。

侧栏菜单观感、归档当前/非当前会话后的整页刷新由 human 验收，暂不标 CLOSED。
