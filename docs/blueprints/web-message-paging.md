# B2：Web 会话分段加载

**状态：APPROVED（2026-10-12）。**

依据：[B1](../brainstorm/web-message-paging.md)，human 已选方案 B（按轮数加条数上限）、滚到顶部自动加载。目标是打开长会话时只加载、只渲染最近一段，往上滚再接着取更早的。本文修订 [gateway](gateway.md) §五 的回放起点和 [web-ui](web-ui.md) §五，其余条款不变。

## 一、用户行为

- 打开会话：只显示最近一页，规则见 §二。显示后照常贴底。
- 往上滚到距顶部约三屏时，自动加载更早的一页，插在列表最上方。插入时视口不跳，用户眼前的内容保持不动。
- 加载状态有两处提示：
  - **浮动提示**：消息区顶部浮出一个胶囊，不占布局。预取通常发生在视口外，靠它告诉用户发生了什么。三种状态：
    - 正在加载：转圈图标加「正在加载更早的消息」；
    - 加载完成：「✓ 已加载 N 条更早的消息」，到开头时加上「已到会话开头」，2.5 秒后收起；
    - 加载失败：「加载失败：<原因>」加「重试」，不自动消失。
  - **列表内**：列表最上方，加载中显示转圈和「正在加载更早的消息」，到开头后显示「会话开始 · <首条时间>」分隔线。每次加载完成，在新旧消息交界处插入「↑ 以上为新加载的 N 条」分隔线：淡入、停留约 3 秒后变淡，只改透明度、不改高度，所以视口不动。N 不含不显示的 HarnessNote。
- 已经加载的消息不会回收。切换会话或刷新页面后，从最近一页重新开始。
- 压缩卡片（Boundary）和其他消息一样，滚到哪里就显示在哪里，不参与分页判断。卡片标题去掉「保留 N 步」，见 §5.3。

## 二、分页规则

**一轮** = 一条 `UserInput`，加上它之后、下一条 `UserInput` 之前的所有消息。

在 `before` 之前（不带 `before` 就是到最新为止）取一页，步骤如下：

1. `start_rounds`：`before` 之前倒数第 `PAGE_ROUNDS` 条 `UserInput` 的 id。不足 `PAGE_ROUNDS` 条时取 0，也就是从会话开头开始。
2. `start_cap`：`before` 之前倒数第 `PAGE_MESSAGES` 条消息的 id。不足 `PAGE_MESSAGES` 条时取 0。
3. `start = max(start_rounds, start_cap)`。最近 `PAGE_ROUNDS` 轮合计不超过 `PAGE_MESSAGES` 条时，一页从某一轮的开头开始；超过时（单轮很长，或多轮加起来超过）按条数上限截断，从某一轮中间开始。
4. 本页包含 `start ≤ id < before` 的全部消息，按 id 升序排列。`has_earlier` 表示 `start` 之前是否还有消息。

在 `mic-gateway/src/limits.rs` 中新增两个常量：`PAGE_ROUNDS = 20`、`PAGE_MESSAGES = 300`。

被截断的轮次中，ToolResult 可能和对应的 ToolCall 分在两页。前端现有逻辑在找不到调用时，会把结果单独显示出来（`MessageItem` 中判断 `!callIds.has`）。等更早的一页加载进来，`results` 和 `callIds` 会重新计算，结果自动归到对应的调用下面。这里不需要额外处理。

## 三、Rust 公开契约

`mic-store`：

```rust
pub struct MessagePage {
    /// id 升序。
    pub messages: Vec<Message>,
    /// 本页之前是否还有消息。
    pub has_earlier: bool,
}

impl Store {
    /// `before` 之前的一页（不含 `before`；`None` = 到最新），规则见 web-message-paging §二。
    pub async fn messages_page(&self, session: SessionId, before: Option<MessageId>,
        rounds: NonZeroU32, cap: NonZeroU32) -> Result<MessagePage, StoreError>;
}
```

实现：一条查询，按 `idx_messages_session(session_id, id)` 倒序取 `id < before` 的 `cap + 1` 行。在前 `cap` 行里从新到旧数 `UserInput`：
- 数到第 `rounds` 条，就截到这条为止；
- 数不到，就截满 `cap` 行。

截下的部分反转成升序，就是这一页。截取点之后还有剩余行，`has_earlier` 就为真。这样一页最多扫 `cap + 1` 行，结果与 §二 的规则等价，也不需要新增索引。

`mic-core`：`Kernel::messages_page` 同签名直通到 Store，`MessagePage` 从 `mic-store` 原样导出，与 `messages_after` 的处理方式一致。

`messages_after` 保持不变，仍供重连续传和微信引用（`quote.rs`）使用。

## 四、HTTP

### 4.1 流的回放起点（修订 gateway §5.2 第 2 步）

- **不带 `after`**：回放 `messages_page(session, None, PAGE_ROUNDS, PAGE_MESSAGES).messages`，把回放到的最大 id 记为 N（没有消息时 N 为空）。之后的交接逻辑不变。
- **带 `after`**：和原来一样，回放 `messages_after(after)`。

`ready` 和其他事件的格式都不变。是否还有更早的消息不在流里告知，由 §4.2 的接口负责，这样分页状态只有这一个来源。

### 4.2 新接口

| 方法与路径 | 请求 | 成功响应 |
|---|---|---|
| `GET /api/sessions/{id}/messages?before=<message_id>` | `before` 必填 | 200 `{"messages": [Message…], "has_earlier": bool}` |

- 会话不存在返回 404，判断方式与 `/stream` 相同。任何渠道的会话都可以读，微信会话在网页上是只读查看。
- `before` 缺失或不合法，返回 400。`before` 可以是任意整数，不要求是这个会话里真实存在的 id。
- 和 `POST /api/sessions/{id}/messages`（发送消息）路径相同，只是方法不同。

## 五、前端

`api/client.ts`：新增 `getMessages(sessionId, before)`，经 `decode.ts` 解码。

`SessionView` 增加以下状态和方法：

```ts
earlier = $state<"idle" | "loading" | "none" | { error: string }>("idle");
/** 取 messages[0] 之前的一页，插到最前面；同一时刻只跑一个。 */
async loadEarlier(): Promise<void>;
```

- `loadEarlier` 的执行条件：`earlier` 为 `idle` 或 `{error}`、`ready` 为真、`messages` 不为空。取 `before = messages[0].id`，成功后执行 `messages.unshift(...page.messages)`，`earlier` 设为 `page.has_earlier ? "idle" : "none"`。失败时 `earlier` 设为 `{error}`。会话关闭（abort）时丢弃结果。
- 如果回放为空（会话里还没有消息），`earlier` 直接设为 `"none"`。
- 重连时 `messages` 和 `earlier` 都保留，不受影响。

### 5.1 自动加载的触发

`MessageList` 在内容顶部放一个哨兵元素，用 `IntersectionObserver` 监听：`root` 设为滚动容器，`rootMargin` 顶部预留三屏，用户划到顶之前通常已经接上。

- 回调里哨兵相交就调用 `loadEarlier()`，是否真的发请求由它自己按条件判断。
- `IntersectionObserver` 只在相交状态变化时通知。因此用一个 `$effect` 跟踪 `view.ready` 和 `view.earlier`：两者变为「`ready` 且 `idle`」时，对哨兵先 `unobserve` 再 `observe`。浏览器会在下一次布局后（锚点校正之后）按最新位置再通知一次。这样以下两种情况都能自己接上：
  - 哨兵已经在预加载区域，但流还没 `ready`；
  - 一页加载完，哨兵仍在预加载区域。
- 单请求的约束仍由 `loadEarlier` 负责。
- 会话很短、一打开就能看到哨兵时，会立刻请求一次，返回空页，然后显示「会话开始」。这样多发了一个小请求，可以接受。

### 5.2 保持眼前内容不动

滚动容器设为 `flex-direction: column-reverse`，内部只有一个内容块（`mb-auto`，内容不满一屏时靠上）。这样 `scrollTop` 从底部起算：0 表示在底部，往上为负。

- **上方变高不用处理**：插入旧页、工具结果归并、视口上方的图片加载，都不改变离底距离，视口自然不动，程序不写 `scrollTop`。Safari 和 iOS 在惯性滚动中只要 JS 写了 `scrollTop`，惯性就会被打断，看起来像突然跳了一下；这是最初「保持离底距离」和「锚点校正」两版在真机上跳动的原因（2026-10-12 human 反馈）。
- **贴底**：在底部时内容变高，`scrollTop` 仍为 0，浏览器自然停在底部；首次打开也直接停在底部，不需要 JS 追底。距底小于 48px 算贴底，`scrollTop` 变小（往上）算用户上翻。
- **不贴底时下方变高**（实时消息、草稿）：唯一需要校正的情况。`onscroll` 记录视口内第一条消息作为锚点，`ResizeObserver` 发现它偏移了（≥1px）就把它放回原位，并记下校正后的 `scrollTop`。校正产生的 scroll 事件到达时，如果 `scrollTop` 等于这个值，就不重选锚点。
- 滚动容器设 `overflow-anchor: none`，不让 Chrome 自带的锚定再修正一次。

顶部提示文案按 §一。「重试」按钮直接调用 `loadEarlier()`。

### 5.3 压缩卡片

`ContextBoundaryView` 原来用已加载的 `view.messages` 统计「保留 N 步」。分页后这个数会少算，加载更多历史时还会变化。这个数字不是必要信息（保留步数本身在设置里），因此**去掉**，标题只保留「已压缩 / 已撤回 · 查看摘要」，不新增统计接口。

## 六、兼容

| 调用方 | 影响 |
|---|---|
| 前端首次打开会话 | 只收到最近一页；更早的通过 §4.2 获取 |
| 前端重连（带 `after`） | 不变 |
| 微信引用 `quote.rs`（`messages_after(None)`） | 不变 |
| `ready` 和其他 SSE 事件 | 不变 |
| `ContextBoundaryView` | 去掉「保留 N 步」（§5.3） |
| `gateway.md` §5.2、`web-ui.md` §五 | 由本文修订；关闭时在这两处各补一行，指向本文 |

前端和服务端在同一个二进制里一起发布。旧页面连上新服务端时，只会少看到更早的消息，不会出错。

## 七、实现顺序与验收

实现顺序：Store `messages_page`（附 `tests.rs`：按轮切分、单轮超过上限时截断、`before` 边界、空会话）→ Kernel 直通 → 流起点和新接口 → 前端 → 收尾（`cargo fmt`、`cargo clippy -- -D warnings`、`bun run check`）。

本地自动验收，在临时库里造一个 600 条以上的会话，用 Playwright 驱动 Chromium：

1. 打开会话后 DOM 中的消息数不超过一页，并且贴底。
2. 滚到顶部后自动加载上一页，加载前后视口顶部那条消息的屏幕位置偏差不超过 2px；一页加载完哨兵仍在预加载区域时，会继续加载下一页。
3. 一直滚到会话开头，显示「会话开始」，消息总数和库里一致，没有重复。
4. 构造一页的开头恰好落在 ToolResult 上：加载前这条结果单独显示，加载上一页后归到对应调用下面，视口内容不跳。
5. 用户上翻停住后，视口上方的图片加载完成，眼前内容不跳。
6. 流断开重连后，不会重复，也不会丢失。

交给 human 的部分：Safari 和手机上滚动加载时的观感。
