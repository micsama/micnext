# B2：对话记录与用户所见大致一致（第一步：微信媒体占位）

**状态：CLOSED（2026-10-10）。r1 已实现并经服务器验收；r2 删除图文混发路径及其连带的 `about` 关联，已实现，fmt/clippy/svelte-check 通过**

方向来源：[`docs/brainstorm/conversation-parity.md`](../brainstorm/conversation-parity.md)。

## 一、动机

微信用户只发一张图片时，入站不落 UserInput，只追加「暂不支持这类消息。」通知。
模型只看到半句，误判为自己的回复被微信拒收。对话里应是：用户「[图片]」→ 系统「暂不支持」→ 用户下一句 → 模型回答。

核心目标（human 已定）：让模型掌握用户这轮交互里发生了什么，使模型所见与用户所见大致一致；无法直接呈现给模型的内容打扁，保留影响理解的事实，并尽量节省 token。

按独立变化轴拆成两部分：

| 职责 | 所有权与消费者 | 生命周期 |
|---|---|---|
| 对话记录与系统解释 | core 负责落盘、事件与执行处置；懂领域的一侧产生解释。用户与模型共同消费记录 | 长期保留：记录用户做了什么、系统如何处置 |
| 微信不支持媒体的打扁适配 | 微信 client 保留媒体类型与文件名，service 按微信当前能力生成占位和处置说明 | 临时保留：对应能力实现后，逐项替换为真实内容入站并删除打扁分支 |

这里确认的是两个职责边界，不预定新增 crate 或 Rust 模块；若需要新增模块，另补 B2 契约。

长期规则：

- 能直接处理的内容照常传，不额外解释；模型拿不到的内容用短占位保留类型。
- 占位回答「用户发了什么」；Notification 回答「系统如何处置」。面向用户的系统提示双方同文，措辞对双方都成立。
- 只有处理差异时才追加解释；同一条入站合并为一条短 Notification，不逐片段重复说明。既有失败/恢复提示继续保留。
- 本步占位复用 Text，不新增消息或片段类型；core 不从文本里的 `[图片]` 猜测执行策略。用户真的输入 `[图片]` 仍是普通文字。
- 是否触发执行独立于内容类型；含占位的入站不启动新执行，使用 held 处置，减少无意义的模型调用。
- 只给模型的单向通道仍只有 HarnessNote。

最终消费者是模型与微信用户；直接收益是模型不再因缺失用户行为或系统解释而误读对话。

## 二、契约

复用已有 held 输入语义：已落盘、可被后续上下文读取、自身不触发执行；held 不表达「不支持媒体」。
输入、hold 与处置说明在 store 内同事务写入，core 按 id 顺序发布，不唤醒。

```rust
// mic-message：不变（r1 新增的 Notification.about 删除）

// mic-store：改签名，唯一调用方是 core kernel
pub enum InputDisposition {
    /// 未认领，等调度或当前 run 并入。
    Pending,
    /// 落盘即 held：进入后续上下文，永不被认领。
    Held,
}

impl Store {
    /// 写一条用户输入，可附一条 Notification；图片行、消息、hold 行与通知同事务。
    pub async fn append_input(
        &self, session_id: SessionId, person: PersonId, parts: Vec<NewInputPart>,
        disposition: InputDisposition, notice: Option<NewNotice>, at: i64,
    ) -> Result<Vec<Message>, StoreError>;
}

pub struct NewNotice { pub source: String, pub text: String }

// mic-core：新增；`append_user_input` 签名不变，与新方法共用同一私有写入/发布路径
impl Kernel {
    /// 只记录不执行的入站：输入以 held 落盘，与处置说明同事务，按 id 顺序发布，不唤醒。
    pub async fn append_recorded_input(
        &self, session_id: SessionId, person: PersonId, parts: Vec<IncomingPart>,
        source: &str, notice: String,
    ) -> Result<MessageId, KernelError>;
}
```

- `Store::context_window` 不变：held 输入与其后的通知均按既有规则可见，同事务写入使两者同时出现。
- `source` 与 `append_notification` 一致，由调用方显式传入（Kernel 句柄不带模块身份）。
- `append_user_input` = `Pending` + 无通知 + 唤醒；Web 调用不变。
- 启动时 `hold_unclaimed_inputs` 只处理「未认领且未 held」，已 held 的不会再生成「上次有消息未执行」通知。
- 错误沿用 `KernelError` / `StoreError`，无新增变体。

## 三、微信入站

`IncomingContent::Unsupported` 携带媒体类型，按 item 原顺序与文本一起打扁成 Text 片段：

| item | 占位 |
|---|---|
| 语音有转写 | `[语音转写] {text}`（真实内容，照常执行，不附解释） |
| 图片 | `[图片]` |
| 语音无转写 | `[语音]` |
| 文件 | `[文件：{file_name}]`，无文件名时 `[文件]` |
| 视频 | `[视频]` |
| type 0 / 11 / 12 | `[不支持的消息]` |

- 不含占位 → `append_user_input`（照常执行）。
- 含任一占位 → `append_recorded_input`，源 `wechat`，文案「微信渠道暂不支持{类型}，这条消息仅作记录。」
  - 多种类型按首次出现顺序去重，以「、」连接；同类型多项保留各自占位，解释不重复类型。
  - 微信客户端图片与文字分条发送，图文同条在实际使用中不出现；协议上若出现，文字随整条记录，下一次执行时可见。

临时适配退出条件：某类内容具备真实处理能力后，该类走真实内容入站，删除其占位与「不支持」说明；长期记录、Notification 和执行处置契约继续保留。历史占位仍按当时事实读取，不回填为真实媒体。

## 四、调用方

| 调用方 | 改动 |
|---|---|
| `mic-channel-wechat` service / client | 按 §三 打扁与分流 |
| `mic-gateway` | 不变（Web 图片照常作为图片输入） |
| `mic-message` 匹配方 | 不变（r1 补的 `..` 与 `about: None` 回退） |
| Web `api/types.ts` / `api/decode.ts` | 不变（r1 的 `about` 与 `optional` 解码器回退） |
| `bin/micnext` `-p` 输出 | 不变 |
| core kernel | `append_user_input` 改走共用私有路径，行为不变；新增 `append_recorded_input` |
| core 调度 / recovery | 不变：已按 `UNCLAIMED AND NOT HELD` 认领；recovery 的 held 排除规则保留 |
| store 上下文窗口 | 不变 |
| Web 消息列表 | 不变：held 输入按普通用户消息显示 |

依赖方向不变。

## 五、输入与解释的一致性

| 实体 | 写者 | 状态/转换 |
|---|---|---|
| 入站内容与媒体事实 | 微信 client / service | 上游 item → 强类型媒体事实 → 真实文本与占位 |
| held UserInput 与解释 | store 同事务写入，core 发布 | 不存在 → 两条同时可见（id 相邻，输入在前） |
| 模型上下文 | core run 读取 store | held 输入与非输入消息按既有规则进入窗口 |

不变量：

- 对同一条入站，模型窗口内输入与解释同时可见；顺序为输入 → 解释。held 输入落盘即可见，不存在「只见解释」的窗口。
- Held 不触发新执行，但可进入正在执行的 run 的后续上下文。
- 事件只在事务提交后发布，按 id 顺序，共用发布锁。
- 写入失败 → 输入、hold、解释都不存在，返回 Err；微信 service 沿用既有处置（Service Err，批次保留 importing，重启转 interrupted 不重导）。

r2 修订理由：r1 的图文混发（Pending 输入 + 解释）在 absorb 与读窗口之间可能只见解释，为此引入 `about` 关联与窗口过滤。微信客户端图文分条发送，该路径实际不可达；按「消费者消失时生产者一并删除」，删除混发路径、`InputHandling`、`about` 及过滤。日后出现「部分执行 + 解释」的真实需求（如当前模型不支持图片），按当时形态重新设计。

## 六、r1 改动去留

保留（r2 仍需要）：

| 位置 | r1 改动 | 理由 |
|---|---|---|
| wechat `client.rs` | `IncomingContent::Unsupported(Unsupported)` 携带媒体类型与文件名 | 占位与解释的事实来源 |
| wechat `service.rs` | `flatten` 按原顺序生成占位、类型去重成文案；空内容 `continue` | 占位与解释的生成；空入站不应报 `InputError::Empty` 退出 |
| store `types.rs` / `lib.rs` | `InputDisposition`、`NewNotice` | 落盘处置与通知参数 |
| store `images.rs` / `store.rs` | `append_input` 同事务写输入、hold 行、通知；`insert_message` 改 `pub(crate)` | 原子性：不出现只有占位或只有解释 |
| core `kernel.rs` | 私有 `append_input`（落盘 + 按 id 发布）；`append_user_input` 改走它 | 一条主路径 |
| core `assembly.rs` | `-p` 改调私有 `append_input(Pending, None)` | 跟随签名 |

改写：

| 位置 | r1 | r2 |
|---|---|---|
| core `kernel.rs` | `append_explained_input(.., handling, source, notice)`，`Run` 唤醒 / `Record` 不唤醒 | `append_recorded_input(.., source, notice)`，固定 `Held`，不唤醒 |
| wechat `service.rs` | 三分支：纯文本 / 混发 `Run`「仅将文字部分交给 AI 处理」/ 纯占位 `Record` | 两分支：无占位 → `append_user_input`；有占位 → `append_recorded_input`「这条消息仅作记录」。`Flattened.has_text` 与日志字段 `has_text` 无消费者，删除 |

删除（仅为混发并发可见性服务）：

| 位置 | r1 改动 |
|---|---|
| core `input.rs` / `lib.rs` | `InputHandling` 枚举及导出 |
| `mic-message` `lib.rs` | `Notification.about` 字段 |
| `model_view.rs`、`bin/micnext` `main.rs` | 为 `about` 补的 `..` |
| `run.rs`、`recovery.rs`、kernel `append_notification`、store 启动 hold 通知 | 为 `about` 补的 `about: None` |
| store `context_window` | 按 `about` 过滤及 `HashSet` 导入 |
| Web `types.ts` | `about?: number` 与 `about: optional(num)` |
| Web `decode.ts` | `optional` 解码器；`obj` 恢复「缺字段即失败」 |
| 文档 `mic-message.md` | `Notification` 恢复为 `{ source, text }` |
| 文档 `wechat-channel.md` | `append_explained_input` 与「混合内容照常执行」改为 r2 口径（`model-settings.md` 的 `append_input` 签名不变，保留） |

## 七、不在本步

以下场景按同一规则后续逐个处理，各自判断是否需要 B2：

- 当前模型不支持图片：遵循长期规则；占位与解释的生成位置、时序及是否重复落盘另行确定。
- `InputError` 整条拒收是否改为入记录。
- 微信投递失败是否入记录。

## 八、验收

- 空闲会话微信只发图片：微信收到「微信渠道暂不支持图片，这条消息仅作记录。」；Web 历史出现用户「[图片]」与该提示，不启动新 run。（r1 已验收）
- 再发文字：照常执行，实际模型请求依次包含图片占位、提示与新文字。
- 正在执行时收到图片：不另开 run；当前 run 后续请求中占位与提示同时出现。
- 只有文字：照常执行，不增加本功能的 Notification。
- 用户手打 `[图片]`：作为真实文字执行，不按占位判定。
- 重启后不对上述 held 输入补发「上次有消息未执行」。
- 旧库与 r1 期间写入的 Notification（含 `about` 字段）照常读取与展示：`MessageBody` 未开 `deny_unknown_fields`，残留 `about` 被忽略。
- `cargo fmt`、`cargo clippy -- -D warnings`、Web `svelte-check`。
