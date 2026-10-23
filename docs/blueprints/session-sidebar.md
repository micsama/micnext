# B2：侧栏状态点、后台更新提醒与置顶

**状态：已批准（human 2026-10-23），已实现，本地检查通过，待 human 验收。同日按外部审查补齐跨端刷新、旧页刷新与展示范围；「其它渠道」改为不折叠。**

依据：[B1](../brainstorm/session-sidebar.md)。2026-10-23 human 已定方向：单行加左侧状态点，未读只给 Web 会话，显示运行中转圈。标签另写 B2，本篇不涉及。

## 行为

### 行布局

每行仍是单行：标题左侧预留置顶按钮，右侧悬停时出现「⋯」（仅归档）。

- 运行态优先展示，覆盖整个 run，包括中间回复、多轮模型调用和工具执行：转圈配轻微呼吸边框。
- run 结束后，有未读回复才显示静态主色细边框和加粗标题；运行期间不显示未读提醒，窄屏菜单提醒也排除运行中的会话。
- 未置顶时，标题左侧按钮在行悬停或键盘聚焦时出现；窄屏直接显示。已置顶时常显，再次点击取消置顶。
- 每组列表使用淡斑马纹，选中和悬停背景优先。
- 沿用现有 `running`、`unread` 字段，未读水位和 HTTP 契约不变；这里区分存储未读事实与执行结束后的提醒展示。

### 未读

- 只有 **Web 渠道的 Root 会话**计算未读。如果它的 `Reply` 或 `Notification` 的 id 大于 `seen_message_id`，就算未读。用户输入、工具过程和子会话的消息都不算。
- 已读水位存在服务端，多端共用。水位只往前推：写入时取 `MAX(旧值, 新值)`。
- **标为已读**：会话页打开，并且 `document.visibilityState === "visible"` 时，Web 把当前已加载的最新消息 id 上报。时机有三个：首屏加载完成；页面可见时有新消息落地；标签页从后台切回前台。上报做节流；只有上报成功后，才记下「已上报到的 id」，并把本页列表里这条会话的 `unread` 置为 false。上报失败不记，下次时机到了用同一个 id 重试。
- 迁移时把现有会话的水位设成当前最大消息 id，升级后不会一下子亮起一片。新建会话的水位为空，等于 0。

### 运行中

Root 会话有 `Executing` 状态的 run，就算运行中。这里不看子会话：子任务运行时，父会话的 run 通常也在执行。

### 置顶

- `core_sessions.pinned_at` 可空。只有 Root 会话能置顶，置顶跨渠道，Web 和微信的都可以。
- 已经置顶的会话再次置顶，保留原来的置顶时间（`COALESCE`），排序不变。
- 侧栏最上面是「置顶」分组：一次全部列出，不分页，按 `pinned_at` 降序。置顶的会话不再出现在日期分组和「其它渠道」折叠里。
- 归档时同一条 UPDATE 清掉 `pinned_at`。已归档的会话不能置顶，返回 409，和其他写操作一致。
- 不做拖拽排序。

### 实时更新

**会话行变化事件**：现有的 Kernel 事件 `SessionTitleChanged` 改名为 `SessionChanged`，含义扩大为「会话行的列表投影变了」，覆盖标题、置顶、归档和已读。发布方都是 Kernel，在写入提交后发出：

- 标题：沿用现有的三个发布点，包括标题生成成功或失败、定时任务改名；
- 置顶、取消置顶：`set_session_pinned` 之后；
- 归档：`archive_session` 之后；
- 已读：`mark_session_seen` 只有水位真的往前推了才发。store 返回「是否推进」。

会话流里原来的 `session_title_changed` 同步改名为 `session_changed`，聊天页收到后照旧重拉会话详情。这样另一个标签页归档或置顶时，当前页也能更新。

**全局 SSE** 新增 `session_activity { id }` 事件，由 Gateway 从下面这些 Kernel 事件映射过来：

- `MessageAppended`，且消息是 `Reply` 或 `Notification`；
- `RunStarted`、`RunFinished`；
- `SessionChanged`。

`id` 取事件里的 `session_id`。如果是子会话的事件，客户端多拉一次也没有坏处，不在 Gateway 查树根。

**Web 收到事件后**：
- 节流约 500 ms，合并成一次刷新：重拉置顶列表、Web 列表和各渠道列表。
- 同一时间只跑一次刷新。刷新进行中又来事件，就记一个标记，结束后再补一轮。
- SSE 断线重连成功后，也刷新一次。
- 删除现有的「当前会话跑完时重拉」，以及「窗口聚焦时」重拉会话列表；聚焦时重拉设置的逻辑保留。

**列表刷新改为重拉已加载的范围**：`SessionList.refresh()` 现在只重拉首页，后面已加载的页原样保留。那几页里的会话如果被置顶、归档，或者已读、运行状态变了，就会残留或过期。改成：按当前已加载的页数，从首页开始逐页重拉，到原来的游标为止，然后整体替换列表。通常只有一两页。`loadMore` 不变。

### 折叠与窄屏

- 「其它渠道」**不再折叠**（human 2026-10-23）：渠道很少，在日期分组下方，每个渠道一个分组标题（如「微信」），样式和日期分组标题一致，用一条分隔线和 Web 部分隔开。启动时就加载首页，没有会话的渠道不显示；各渠道仍可「加载更多」。每行照常显示状态槽，所以不需要汇总到渠道级别的运行状态。
- 窄屏下侧栏是抽屉。**已加载的会话有未读时**，顶栏菜单按钮上显示一个小红点。「已加载」包括置顶分组，以及 Web 列表里已经加载的所有页，不额外查询。还没加载的旧页里有未读时，这个红点不会亮。

## 契约

### 存储（mic-store）

迁移 core v26：

```sql
ALTER TABLE core_sessions ADD COLUMN pinned_at INTEGER;
ALTER TABLE core_sessions ADD COLUMN seen_message_id INTEGER;
UPDATE core_sessions SET seen_message_id =
  (SELECT MAX(id) FROM core_messages m WHERE m.session_id = core_sessions.id);
```

- `SessionSummary` 新增三个字段：`pinned_at: Option<i64>`、`unread: bool`、`running: bool`，都在 `summaries()` 里算出来。`unread` 只对 `channel = 'web'` 的 Root 会话计算，其余会话恒为 `false`。
- `list_root_sessions` 增加条件 `pinned_at IS NULL`。
- 新增 `list_pinned_sessions() -> Vec<SessionSummary>`：返回所有未归档、已置顶的 Root 会话，按 `pinned_at DESC, id DESC` 排序。
- 新增 `set_session_pinned(id, at: Option<i64>)`：执行前调用 `ensure_active`。置顶时写 `COALESCE(pinned_at, ?)`，取消置顶时写 NULL。
- 新增 `mark_session_seen(id, message_id) -> bool`：执行 `SET seen_message_id = ? WHERE id = ? AND COALESCE(seen_message_id, 0) < ?`；返回值表示水位是否推进。
- `archive_session` 同时把 `pinned_at` 置为 NULL。

Kernel 为上面这几个方法提供同名入口：
- 置顶和已读：先检查会话存在并且是 Root，否则返回 `SessionNotFound` 或 `SessionNotRoot`；
- 置顶、已读推进、归档：写入后发布 `SessionChanged`；
- 两个列表查询：直接转发。

`KernelEventKind::SessionTitleChanged` 改名为 `SessionChanged`，同时更新现有的三个发布点。

### HTTP（Gateway）

| 接口 | 变化 |
|---|---|
| `GET /sessions?channel=` | 不再返回置顶会话；每条新增 `pinned_at`、`unread`、`running` |
| `GET /sessions/pinned` | 新增：`{ items: SessionItem[] }`，跨渠道，不分页 |
| `GET /sessions/{id}` | 新增同样的三个字段 |
| `PUT /sessions/{id}/pin` | 新增：请求体 `{ pinned: bool }`，返回 204；会话不存在返回 404，不是 Root 返回 403（同现有 `SessionNotRoot`），已归档返回 409 |
| `POST /sessions/{id}/seen` | 新增：请求体 `{ message_id }`，返回 204；会话不存在返回 404，不是 Root 返回 403；message_id 不属于该会话返回 400 |
| `GET /stream` | 新增 `session_activity { id }` 事件 |
| `GET /sessions/{id}/stream` | `session_title_changed` 改名为 `session_changed` |

### Web

- `api/types.ts` 和 `decode` 同步上面的字段与接口，继续严格解码。
- `Sidebar.svelte`：
  - 新增置顶分组；
  - 每行加运行/未读边框与左侧置顶按钮；
  - 「⋯」菜单仅保留归档；
  - 订阅 `globalStream` 新增的 `sessions` 计数，计数变化时重拉。
- `SessionList.refresh()`：改为重拉已加载的范围，整体替换列表。
- `App.svelte`：删除跑完时重拉，以及聚焦时重拉会话列表；聚焦时重拉设置保留。
- `ChatView`：负责上报已读；会话流的事件名改为 `session_changed`。
- `Header`：窄屏菜单按钮上的未读红点。

## 调用方影响

| 位置 | 变化 |
|---|---|
| `mic-store` sessions、schema | 新增两列、v26 迁移、两个查询和两个写入方法；`summaries()` 多算三个字段 |
| `mic-core` event、kernel、title、schedule | `SessionTitleChanged` 改名为 `SessionChanged`；新增置顶和已读入口；归档后发布事件 |
| `mic-gateway` api、service、global_stream、stream | 三条新路由（`GET /sessions/pinned`、`PUT /sessions/{id}/pin`、`POST /sessions/{id}/seen`）；`SessionItem` 加字段；全局流映射新事件；会话流事件改名 |
| `web` | 见上文 Web 一节 |
| 诊断 SQL、备份 | 只是多了两列，不用改；备份只带设置，不带会话 |

不影响微信 Channel、定时任务、Trace 和标题生成。

## 验收

本地自动：
- store 测试：未读只算 Web Root，且只算 Reply 或 Notification；水位单调，并且返回值反映是否推进；置顶会话不出现在普通列表里；重复置顶时保留原来的置顶时间；归档后清掉置顶。
- `cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`。

交 human：
1. 两个标签页各开一个会话，在 A 里发消息后切到 B，A 跑完后亮起未读点；切回 A 后点消失。电脑上读过以后，手机上的点也跟着消失。
2. 在一个标签页里置顶或归档，另一个标签页的侧栏随之更新，包括后面已加载的页。
3. 后台会话运行时显示转圈。
4. 置顶和取消置顶，包括置顶微信会话；归档已置顶的会话后，它从置顶分组里消失。
5. 手机抽屉收起时，菜单按钮上的红点。
6. 整体观感。
