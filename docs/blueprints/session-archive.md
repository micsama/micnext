# B2：会话归档

**状态：待 human 批准；方向已定（2026-10-13）：从简，只标记隐藏，新消息不自动恢复。与标题独立实现、验收。**

依据：[B1](../brainstorm/session-archive.md)。

## 用户看到什么

侧栏会话条目提供一个「归档」动作。成功后从列表移除，归档当前会话时回到新会话页。Web、微信、CLI 的 Root 会话统一处理。暂不提供永久删除、归档列表或恢复按钮。

归档只影响列表可见性。消息、模型调用、设置都保留；不取消正文或标题任务，收到新消息仍保持隐藏。已知会话 URL 可继续查看。手工把数据库中的 `core_sessions.archived_at` 清为 NULL 后刷新即可恢复。

## 公开契约

| 边界 | 变更 | 调用方影响 |
|---|---|---|
| Store | `archive_session(session: SessionId, at: i64) -> Result<(), StoreError>`，保存首次时间；重复调用成功 | Kernel 先校验存在且为 Root；不触碰入站或执行路径 |
| Kernel | `archive_session(session: SessionId) -> Result<(), KernelError>` | 不存在 404、非 Root 403 |
| HTTP | `POST /sessions/{id}/archive`，无 body；成功 204 | 前端成功后移除缓存中的条目；当前会话导航到 `/` |
| 摘要与详情 | `SessionSummary` 与对应 JSON 增加 `archived_at: Option<i64>`（nullable Unix 毫秒） | 已知 ID 的详情照常返回，显示归档提示；普通列表只返回未归档 Root |
| 列表 | 现有分页查询加 `archived_at IS NULL`，排序及 cursor 不变 | 归档后清掉该条目的本地缓存；正在进行的列表响应不能重新插入它 |

不新增事件流；其它窗口按现有刷新时机获知归档。归档后的已加载旧页也由下一次列表刷新清空重取，避免尾页保留已归档条目。前端不另存长期归档状态。

## 迁移

`core_sessions` 增加 nullable `archived_at`，旧会话全部 NULL。不删记录、不迁移关联表、不自动恢复。手工恢复方式：`UPDATE core_sessions SET archived_at = NULL WHERE id = <会话编号>;`（使用外部 SQLite 工具，Web SQL 页仍只读）。

## 验收

本地验证：不存在/非 Root 的错误、重复归档幂等；分页排除标记；详情、消息与模型调用保留；执行中归档不取消正文或标题；新消息不恢复；手工清标记后重新出现；列表刷新和归档响应竞态不会重插条目。

完成 fmt、clippy 和 Web check；human 验收菜单观感后关闭。
