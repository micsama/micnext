# B1: mic-message 类型设计

**状态**: 历史 B1 论证；现行类型以 [`mic-message` B2](../blueprints/mic-message.md) 为准
**创建**: 2026-08-26 · **修订**: 2026-08-26（交叉验证 + Harness 三分）
**阅读提示**: 本文的候选和旧 Channel 例子不作为待办或实现契约。
**范围**: `crates/mic-message`——`Message`/`MessageAuthor`/消息类型闭集/`ContextBoundary`
的具体 Rust 类型形状。上层语义已在
[`next-gen-architecture.md`](next-gen-architecture.md)
§三/§4.5/§4.9/§五 定好，这里要收敛的是"翻译成强类型"时的几个真实候选。

## 一、ID 类型

**已确认**：`SessionId(i64)`/`QueryId(i64)`/`PersonId` 用自增整数，对应 SQLite
rowid。单进程无分布式 ID 冲突场景，自增最简单、天然可排序。

`Message` 和 `ContextBoundary` 共用同一个 **`SessionEntryId(i64)`**（见下§三），
不各自独立编号——这是第一轮交叉验证的修正点，见§三详述。

ID 由谁分配：`mic-store` 插入时拿到 rowid 再回填，`mic-message` 这层的 newtype
只是"外部分配后带入"的透明包装，不能自生成。插入前/插入后两态（`NewMessage` 无
id / `Message` 带 id）如果只在 store 内部转换一次就消失，不需要在 `mic-message`
公开两个类型，留到 `mic-store` B2 定，这层只定"带 id 的" `Message`。

## 二、Message 内容与结构

**已确认（判别式 enum，排除拍平字段+`kind: String`方案——CLAUDE.md 明确反对
"用字符串分发核心逻辑"）**：

```rust
enum MessageContent {
    Text { content: String },
    ToolCall { id: String, name: String, args: serde_json::Value },
    ToolResult { tool_call_id: String, outcome: /* 见§四 */ },
    Attachment { /* 占位，见§六 */ },
}
struct Message {
    id: SessionEntryId,
    session_id: SessionId,
    author: MessageAuthor,
    content: MessageContent,
    created_at: i64,        // unix millis，纯展示用途，不承担排序职责，见§三
    delivered_at: Option<i64>,
}
```

`query_id` **不进 `Message`**——第一轮交叉验证发现的问题：`next-gen-architecture.md`
§三原语表明确"Message 不归属 Query（Query 只标记自己 claim 的区间）"，claim 关系
只应由 Query 一侧记录（比如 `claimed_range`），不应该在 `Message` 上留一份可能
与之矛盾的第二份记录，否则违反"一处真相"。`ToolCall.args` 存 `serde_json::Value`
不算违反"边界收严禁止 Value 裸漂流"——具体 schema 由每个 Tool 自己的 `Args`
类型决定，`mic-message`（L0）不知道也不该知道有哪些 Tool，`Value` 在这层是"尚未
到达其边界"的中间态，真正的边界解析发生在 `mic-tool` 反序列化成具体 `Args` 的
那一刻。

## 三、Message 与 ContextBoundary：共享严格顺序

蓝图原文（§4.9）："Session 内除 `Message` 外再有一类 `ContextBoundary` 条目"——
是**与 Message 平级的另一种条目类型**，不是 `Message` 的一个 `kind` 分支：

```rust
enum ContextBoundary { Compaction { summary: String }, UserClear }
enum SessionEntry { Message(Message), Boundary(ContextBoundary) }
struct BoundaryEntry { id: SessionEntryId, session_id: SessionId, boundary: ContextBoundary, created_at: i64 }
```

**第一轮交叉验证的修正**：原方案打算靠 `created_at` 排序，但毫秒时间戳可能相同
（比如压缩后紧接着给用户回一条确认消息，容易落在同一毫秒）。既然 `mic-store` 已
决定 boundary 和 message 共用一张表，两者共用同一个自增 `SessionEntryId`（即共享
rowid 空间）是免费的、最简单的修复——`created_at` 退化为纯展示用途，不再承担排序
职责。`ContextBoundary` 需要 `id` + `session_id`，不需要 `author`（压缩由压缩流程
写、`/clear` 由用户触发，但"谁写的"这件事不需要落进 `MessageAuthor` 体系，因为
`ContextBoundary` 从不参与 claim/投递这些依赖 author 的逻辑，见下）。

**Boundary 怎么定位（插入时机）**：不需要跟"被覆盖"的某条消息共享或匹配 id。
压缩发生的时刻，表的当前尾部是什么（比如已写到 `#50`），boundary 就作为新的一条
追加在 `#51`，覆盖范围靠位置隐含（"比这个 entry id 小的都算被概括了"），不需要
显式存一个"我覆盖了 `#1..#40`"的区间字段。SQLite 单 writer 串行写决定了不存在
"同时写入导致顺序不确定"的竞态；唯一的边缘情况是压缩流程读到 `#1..#40` 时决定
概括，但写入前 `#41..#45` 抢先落盘，boundary 因此拿到 `#46`、"顺带覆盖"了
`#41..#45`（即便 summary 文本没提到它们）——这不是正确性问题，只是信息损失，
属于"简单失败语义"能接受的范围，不需要引入锁或额外机制去防。

**`ContextBoundary` 与 claim 无关**：`next-gen-architecture.md` §4.9 已经写明
压缩"不另建 Query、不进 Session FIFO"——`ContextBoundary` 从来不是"进 message
表等着被 claim"的那类条目，是压缩流程/`/clear` 直接写入的，根本不走"未 claim
message → 触发 Query"这条调度路径。第一轮交叉验证曾担心它会不会意外触发 Query，
这个担心不成立，不需要专门排除。

## 四、ToolResult.outcome 的表示

**已确认**：`mic-message` 自建窄类型（如 `{ ok: bool, summary: String, ... }`），
排除存不透明 JSON。

理由（一处真相）：`ToolOutcome`/`ToolError` 是 `mic-tool` 的**执行期**事实，归
执行侧所有；但持久化后要被 Store 落盘、Web/钉钉历史渲染、未来查询工具等多个消费
者读取，属于"换个消费者仍需要的元数据"，按规则该下沉为 `mic-message` 的协议字段。
反过来存不透明 JSON 才是违反一处真相——没有一个类型定义"持久化的 tool result
长什么样"，每个消费者会各自写一份 parse 逻辑猜结构，是多处重复解释同一份事实。
对存储层影响很小：`mic-store` 只序列化/反序列化整条 `Message`，不需要自己另定义
类型；唯一要写转换代码的是 `mic-core`（同时依赖 `mic-tool` 和 `mic-message` 的
那一层），执行完 Tool 拿到 `ToolOutcome`/`ToolError` 后转换成这份窄类型再构造
`Message`——转换点只有一处。

`ToolResult` 本身**不存 `name`**——`name` 只在 `ToolCall` 里存一份，读取时通过
`tool_call_id` 关联对应的 `ToolCall` 消息取得，不冗余存第二份（第一轮交叉验证
发现的文案错误：曾写"`ToolResult` content 里已有 name"，但§二的类型草稿里
`ToolResult` 只有 `tool_call_id`，这里改正）。

## 五、MessageAuthor：采纳 micbot 已验证的三分

蓝图原先只写"沿用 `Harness`/`Notification`"，含糊到底要不要落盘、要不要触发
后续动作。第二轮交叉验证时确认拆回 micbot 已验证过的形状（`crates/message/src/message.rs`）：

```rust
enum MessageAuthor {
    User { id: PersonId },
    Assistant { id: String },
    Tool { name: String },
    HarnessNote,                  // 框架事实型标记，不期待模型动作，不投递
    Notification { source: String }, // 三方系统/事件要投递给用户的通知
}
```

**`HarnessRequest` 不进这个类型**：micbot 原文标注它"只活在单次 request view，
不落盘"——既然 `mic-message::MessageAuthor` 是给**持久化消息**用的类型，一个
永远不会出现在存储里的变体放进来就是类型不诚实（"边界收严"的反面：允许构造出
一个永远非法的状态）。`HarnessRequest` 代表的"框架主动发起、期待模型动作"（压缩/
撞顶收尾/background resume）这个概念，属于 `mic-core` 构造单次模型请求时的临时
拼装逻辑，不属于 `mic-message` 的持久化契约，留给 `mic-core` B2 自己定形状。

判据（决定用 `HarnessNote` 还是 `Notification`）：**这条消息之后是否有一个必然
紧跟的新 Query 会通过模型的回复把这件事自然转达给用户**——有，用 `HarnessNote`
（比如插嘴打断提示，`next-gen-architecture.md` §4.5：打断本身触发了新 Query，
模型的新回复会体现这一点，不需要再单独投递一条通知）；没有，用 `Notification`
（比如崩溃恢复后 `Root`/`Cron` 留的失败记录，`next-gen-architecture.md` §4.7：
不像插嘴那样必然有后续 Query 替它转达，只能靠投递本身让用户看到）。

`User` 带 `PersonId`（硬约束，非候选）：`next-gen-architecture.md` §4.5"插嘴"
一节明确"`Query.person` 唯一确定"依赖 `author` 能区分不同 Person。

`Tool` 变体只带 `name`，不重复 `ToolResult.status`（micbot 原型把 `status:
ToolResultStatus` 也放进 `Tool` 变体，但那是 tool_result 专属属性，跟本项目
§四"`ToolResult.outcome` 独立窄类型"的方案重复——同一份事实两处存，选一处即可，
这里选§四的方案，`author=Tool` 只标"这条是工具产出的"这一个事实）。

## 六、时间戳与展示时区

**已确认**：`i64` unix millis（UTC epoch），排序即数值比较（但 Message/Boundary
的排序真正靠共享 `SessionEntryId`，见§三；`created_at` 纯展示用途）。

**展示时区**：落盘层不感知时区；`mic-channel-web`/`mic-channel-dingtalk` 渲染
历史/推送时默认按 IANA 时区标识 `Asia/Shanghai` 转换展示——用具体时区名而非
"UTC+8"这个偏移量本身，因为同一偏移对应多个地区（西澳大利亚 `Australia/Perth`、
新加坡、马来西亚等同样是 UTC+8），只写偏移量有歧义；选 `Asia/Shanghai` 是因为
这是当前用户所在地，不代表"UTC+8 只有这一个地方"。此为既定默认值，留到对应
channel crate 的 B2 时接线（可能后续开放配置覆盖，当前无需求不预留）。

## 七、未纳入本轮 B1 的范围

- `mic-tool` 的 `ToolError`/`ToolOutcome` 具体形状——那是 `mic-tool` 自己的
  B2，这里只需要知道"`mic-message` 不能依赖它们"这个方向性结论。
- 附件二进制内容怎么存（path 引用 vs blob）——依赖待办 #19（Tool 结果尺寸
  边界），本轮先留 `Attachment` 变体字段为占位候选，不细化。
- `author` 与 `content` 的合法组合（比如 `ToolResult` 只该配 `author=Tool`）
  不在类型层面做 typestate 或运行时交叉校验——`Message` 唯一写者收在 Channel/
  `execute_query`/completion 路径这几处（见 §五唯一写者表），由写者的构造路径
  保证组合合法，边界后默认数据合法，不为单一写者能保证的不变量上重机制。

---

## 已确认（转 B2）

1. ID 类型：自增 `i64`；`Message`/`ContextBoundary` 共享 `SessionEntryId`。
2. Message 内容：判别式 enum，不含 `query_id`。
3. `ToolResult.outcome`：`mic-message` 自建窄类型；`ToolResult` 不存 `name`，
   通过 `tool_call_id` 关联 `ToolCall` 取得。
4. `ContextBoundary`：独立类型 + `SessionEntry` 聚合枚举，与 Message 共享
   entry id 排序，不参与 claim。
5. `MessageAuthor`：`User`/`Assistant`/`Tool`/`HarnessNote`/`Notification`
   五个持久化变体；`HarnessRequest` 不进此类型，留给 `mic-core`。
6. 时间戳：`i64` unix millis；展示时区默认 `Asia/Shanghai`（IANA 标识，非
   "UTC+8"偏移量本身），留到 channel crate B2 接线。

B1 收敛完成，下一步：起草 `mic-message` 的 B2 blueprint（`docs/blueprints/mic-message.md`）。
