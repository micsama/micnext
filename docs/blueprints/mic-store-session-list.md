# B2: mic-store 增量——按 Channel 列举会话

**状态**: 待批准
**来源**: [`product-roadmap.md`](../brainstorm/product-roadmap.md) §2.2、§三 v0a、§四-2
**基线**: [`mic-store.md`](mic-store.md)（CLOSED）。批准并实现后并入该文 §三/§五/§九，本文删除。
**依赖不变量**: 不变，`mic-store` 只依赖 `mic-message`。

## 一、用户视角的效果

- Web 左侧是会话列表，**最近有动静的排最前**；往下滚动再加载更早的会话。
- 每个会话显示**第一条用户文字消息的开头**作为标题；只发了附件的会话没有预览，
  由 Web 显示占位名（如"新会话"+时间）。v0 不支持改名。
- 列表按 Channel 取：v0 只列 Web 会话；微信接入后同一接口列微信会话（只读查看）。
- 列表只含已有用户输入的直接对话会话（Root）。子 agent、定时运行等派生会话不进列表，
  各自从父会话或定时看板进入。
- v0 不做删除/归档。

## 二、公开类型

```rust
/// 分页游标：上一页最后一项的排序键。Gateway 在边界把查询参数解析成本类型。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SessionCursor {
    pub last_activity_at: i64,
    pub session_id: SessionId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionSummary {
    pub session: Session,
    /// 最新 entry 的 `created_at`。
    pub last_activity_at: i64,
    /// 第一条 `User` + `Text` 的前 `PREVIEW_CHARS` 个字符；没有则 `None`。
    pub preview: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionPage {
    pub items: Vec<SessionSummary>,
    /// 还有更早的会话时给出，原样传回取下一页。
    pub next: Option<SessionCursor>,
}
```

## 三、公开签名

```rust
impl Store {
    /// 列出 `channel` 下的 Root 会话，按 `(last_activity_at, id)` 降序。
    /// `before = None` 取第一页；`limit` 由调用方在边界限定上界。
    pub async fn list_root_sessions(
        &self,
        channel: &str,
        before: Option<SessionCursor>,
        limit: u32,
    ) -> Result<SessionPage, StoreError>;
}
```

- 不新增错误变体。游标是强类型、来自已解析的外部输入：任意值都只是一个排序
  位置，不存在"非法游标"，结果可能为空页。
- `PREVIEW_CHARS = 80`，store 内部常量。预览是展示用摘要，不作为会话标识；
  截断在 SQL 侧做，避免把长消息整条读出。

## 四、实现要点

```sql
SELECT <session cols>, act, preview FROM (
  SELECT s.*,
    (SELECT e.created_at FROM session_entries e
      WHERE e.session_id = s.id ORDER BY e.id DESC LIMIT 1) AS act,
    (SELECT substr(json_extract(e.payload, '$.Text.content'), 1, 80)
       FROM session_entries e
      WHERE e.session_id = s.id AND e.author_kind = 'user' AND e.content_kind = 'text'
      ORDER BY e.id LIMIT 1) AS preview
  FROM sessions s
  WHERE s.kind = 'root' AND s.channel = ?1
    AND EXISTS (SELECT 1 FROM session_entries e
                 WHERE e.session_id = s.id AND e.author_kind = 'user')
)
WHERE ?2 IS NULL OR (act, id) < (?2, ?3)
ORDER BY act DESC, id DESC
LIMIT ?4 + 1;
```

- 多取一行判断是否有下一页；`next` 取第 `limit` 项的排序键。
- **活跃时间算出来、不落列**：一处真相是 entry 本身；取"按 id 最新一条 entry 的
  `created_at`"，走已有索引 `idx_entries_session(session_id, id)`，单会话 O(log n)。
  个人规模（会话数百到数千）全量排序可接受；出现性能摩擦再加索引或冗余列。
- **预览依赖 `MessageContent` 的 serde 外部标签形状**（`{"Text":{"content":..}}`）。
  序列化本来就由本 crate 独占，形状变化时这里与写入一起改。
- 翻页期间某个会话有了新消息，它会跳到最前，可能在后续页里缺席或重复出现一次。
  列表以 Web 刷新/实时事件为准，不为此做快照游标。
- 不改 schema，不需要迁移。

## 五、副作用

只读，无副作用。

## 六、调用方

| 调用方 | 用法 | 兼容性 |
|---|---|---|
| `mic-gateway`（Web 会话列表 API） | 解析 `before`/`limit` 查询参数后调用；按 Channel 分组展示 | 新增 API |
| Web 新建会话 | 不经本接口：由 Gateway 生成 `chat` 后调 `Kernel::resolve_root_session`，首条消息后即进入列表 | 无影响 |
| 微信（v0b 之后） | Web 查看微信会话时以 `channel = "wechat"` 调用；查看授权随微信 B2 定 | 无影响 |
| `mic-core`、模块 | 不使用 | 无影响 |

## 七、未纳入

- 会话改名/模型生成标题、删除与归档、置顶。
- 列表项上的"执行中"标记（执行状态随 Gateway 实时事件显示）。
- Task/Triggered 会话列举（随异步子 agent 与定时看板的 B2）。
- 微信投递完成判据：随微信接入 B2，排在 Web 打通之后。
