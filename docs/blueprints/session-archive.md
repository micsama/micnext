# B2：会话归档

**状态：human 已批准精简方案；已实现，本地验收通过，待 human 验收（2026-10-13）。只标记隐藏，新消息不自动恢复，归档成功整页刷新。与标题独立实现、验收。**

依据：[B1](../brainstorm/session-archive.md)。

## 用户看到什么

侧栏会话条目提供一个「归档」动作。成功后从列表移除，归档当前会话时回到新会话页。Web、微信、CLI 的 Root 会话统一处理。暂不提供永久删除、归档列表或恢复按钮。

归档只影响列表可见性。消息、模型调用、设置都保留；不取消正文或标题任务，收到新消息仍保持隐藏。已知会话 URL 可继续查看。手工把数据库中的 `core_sessions.archived_at` 清为 NULL 后刷新即可恢复。

## 公开契约

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| Store | `archive_session(session: SessionId, at: i64) -> Result<(), StoreError>`，保存首次时间；重复调用成功 | Kernel 先校验存在且为 Root；不触碰入站或执行路径 |
| Kernel | `archive_session(session: SessionId) -> Result<(), KernelError>`；新增 `KernelError::SessionNotRoot` | 不存在 404、非 Root 403；只增加一个业务错误，不另建归档错误类型 |
| HTTP | `POST /sessions/{id}/archive`，无 body；成功 204 | 前端成功后整页刷新；归档当前会话时先将地址改为 `/`；请求失败保留条目并显示错误 |
| 列表 | 现有分页查询加 `s.archived_at IS NULL`，排序及 cursor 不变 | 只过滤可见性；现有列表与详情 JSON、`SessionSummary`、`Session` 类型均不增加字段 |

归档成功直接整页刷新，由后端列表过滤呈现结果；归档当前会话时先用现有路由将地址改为 `/`，再刷新。保留浏览器已存的访问凭据。`SessionList` 不变，普通 focus/切会话刷新继续沿用现状。

不新增事件流或定时轮询。其它窗口在现有刷新时机更新第一页；已加载旧页不承诺跨窗口即时同步，整页刷新后重新取到。已知 ID 的详情与发消息路径照常，不新增归档提示或访问限制。

后端仅现有会话表的一列、一次 UPDATE 和列表过滤，不建立归档模块、状态机、任务、历史表或额外索引。仅归档命令经 Kernel 校验存在且为 Root；内部 Store 写入互信。归档后的消息接收、正文调度、标题生成和渠道绑定均沿用现有路径，不检查或清除归档标记。

## 迁移

`core_sessions` 增加 nullable `archived_at`（Unix 毫秒），旧会话全部 NULL；用 `COALESCE(archived_at, at)` 保留首次归档时间，重复归档幂等。不删记录、不迁移关联表、不自动恢复。手工恢复方式：`UPDATE core_sessions SET archived_at = NULL WHERE id = <会话编号>;`（使用外部 SQLite 工具，Web SQL 页仍只读）。

## 验收

以整体功能验收及必要的模块联动检查为主，不新增纯 SQL/字段读写单元测试：不存在/非 Root 的错误、重复归档幂等；分页排除标记；详情、消息与模型调用保留；执行中归档不取消正文或标题；新消息不恢复；手工清标记后重新出现；归档成功刷新页面后列表不出现该条目，当前会话归档后停在新会话页。前端已加载旧页的跨窗口同步边界按上文验收，不另加同步机制。

完成 fmt、clippy 和 Web check；human 验收菜单观感后关闭。

### 本地验收记录（2026-10-13）

`cargo fmt`、`cargo clippy --all-targets -- -D warnings`、`cargo test --workspace`、`web/` 下 `bun run check` 与 `bun run build` 通过。本项未新增单元测试。

[整体功能脚本](../../scripts/check-session-archive.py) 的 4 组隔离 HTTP/SQLite/模拟 Provider 验收通过：执行中的正文与标题不取消、数据及 DTO 保持；新输入不清归档标记且重复归档保留首次时间；不存在/非 Root 的错误、Web/微信/CLI 归档及多页过滤；重启后标记保持、手工清标记恢复且历史保留。

侧栏菜单观感、归档当前/非当前会话后的整页刷新由 human 验收，暂不标 CLOSED。
