# B2：会话搜索

状态：2026-10-26 human 同意方向（按活跃倒序扫描、凑够一页即停，不建索引）；实现完成，本地检查通过，待 human 验收。

## 行为

- **入口**：侧栏「新会话」旁的搜索按钮，或快捷键 ⌘K / Ctrl+K，打开居中的搜索面板。输入后稍作停顿就开始搜，Esc 关闭。
- **搜什么**：所有 Root 会话（Web、微信、CLI，包括已归档的）的标题、用户输入的文字、回复的正文。工具结果、思考过程和后台回报不搜。
- **怎么匹配**：用空白分隔多个词，同一条消息（或标题）里要同时含有全部词。不分英文大小写，中文按子串匹配。
- **顺序与分页**：按会话最近活跃时间倒序，一页 20 个会话，滚动到底时自动加载更多。会话越老扫得越晚，所以近期会话自然排在前面。
- **结果**：每个会话显示标题（命中的词高亮）、渠道和归档标记、最近活跃时间，下面列最多 3 条最新的命中片段，标明是用户说的还是回复，命中词高亮。超过 3 条时注明还有多少条。
  - 点标题打开会话。
  - 点片段打开会话，并定位到那条消息：滚到视口中间，短暂高亮。
  - 支持 ↑↓ 选择、回车打开。

## 契约

- **store**：
  - `search_sessions(terms, before: Option<SessionCursor>, limit) -> SearchPage { items: Vec<SessionHit>, next }`。
  - `SessionHit { summary: SessionSummary, title_hit: bool, hits: Vec<MessageHit>, hit_count }`，`MessageHit { id, created_at, from_user, text }`。
  - 游标沿用会话列表的 `(last_activity_at, id)`。`next` 表示还有没扫到的会话。
  - 新增 `messages_page_until(session, until, before, rounds, cap)`：返回 `[until, before)` 范围内的全部消息，再加上 `until` 之前正常的一页。定位一次取到位；`messages_page` 不变。
- **Gateway**：
  - 新增 `GET /sessions/search?q=&before_at=&before_id=`，返回 `{ items: [{ session: SessionItem, title_hit, hits: [{ message_id, created_at, from_user, snippet }], hit_count }], next }`。
  - 片段截取命中位置附近约 80 字。
  - `q` 为空、超过 5 个词或超过 100 字时返回 400。
  - `GET /sessions/{id}/messages` 增加可选参数 `until`。
- **Web**：路由 `/s/{id}?m={message_id}` 表示打开会话后定位到这条消息。
- **规模**：`limits.rs` 新增每页会话数、每个会话的片段数、词数和字数上限。

无 schema 变更。

## 验收

- **本地**：store 测试覆盖多词同条命中、标题命中、不搜工具结果和思考、跨页游标；`until` 分页的覆盖范围。再加上收尾检查。
- **human**：面板观感；用中文两字词和英文混合搜索；从片段跳到很早的消息。
