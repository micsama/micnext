# B2：对话记录与用户所见大致一致（第一步：微信媒体占位）

**状态：APPROVED 并已实现（2026-10-09）；fmt/clippy/svelte-check 通过，待服务器验收后 CLOSED**

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
- 是否触发执行独立于内容类型；纯占位不启动新执行，使用 held 处置，减少无意义的模型调用。
- 只给模型的单向通道仍只有 HarnessNote。

最终消费者是模型与微信用户；直接收益是模型不再因缺失用户行为或系统解释而误读对话。

## 二、契约

复用已有 held 输入语义：已落盘、可被后续上下文读取、自身不触发执行；held 不表达「不支持媒体」。
输入与处置说明在 store 内同事务写入，说明带 `about` 指向其输入；core 按 id 顺序发布后才唤醒。模型窗口内说明的可见性跟随其输入（§五）。

```rust
// mic-message：Notification 增加可选关联；缺省不序列化，旧库行读为 None
pub enum MessageBody {
    // ...
    /// 投递给用户的框架通知；`about` 为其解释的那条输入，模型窗口内与该输入同进同出。
    Notification {
        source: String,
        text: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        about: Option<MessageId>,
    },
}

// mic-store：改签名，唯一调用方是 core kernel
pub enum InputDisposition {
    /// 未认领，等调度或当前 run 并入。
    Pending,
    /// 落盘即 held：进入后续上下文，永不被认领。
    Held,
}

impl Store {
    /// 写一条用户输入，可附一条解释它的 Notification（`about` = 该输入）；图片行、消息、hold 行与通知同事务。
    pub async fn append_input(
        &self, session_id: SessionId, person: PersonId, parts: Vec<NewInputPart>,
        disposition: InputDisposition, notice: Option<NewNotice>, at: i64,
    ) -> Result<Vec<Message>, StoreError>;
}

pub struct NewNotice { pub source: String, pub text: String }

// mic-core：新增；`append_user_input` 签名不变，与新方法共用同一私有写入/发布路径
pub enum InputHandling {
    /// 照常执行：发布后唤醒。
    Run,
    /// 只记录：以 held 落盘，不唤醒。
    Record,
}

impl Kernel {
    /// 渠道入站存在处理差异时使用：输入与处置说明原子落盘，按 id 顺序发布，`Run` 才唤醒。
    pub async fn append_explained_input(
        &self, session_id: SessionId, person: PersonId, parts: Vec<IncomingPart>,
        handling: InputHandling, source: &str, notice: String,
    ) -> Result<MessageId, KernelError>;
}
```

- `Store::context_window` 读取规则新增一条：`about` 指向的输入不在本次窗口内时，该通知不进窗口。
  覆盖未认领、person 不匹配、held 与 Boundary：同事务写入使两者 id 相邻，Boundary 不会落在两者之间。
  `about = None` 的既有通知规则不变；`context_window` 唯一调用方为 `run.rs` 的模型调用。
- 其余 Notification 生产者（run、recovery、启动 hold、`append_notification`）填 `about: None`，行为不变。
- `source` 与 `append_notification` 一致，由调用方显式传入（Kernel 句柄不带模块身份）。
- `append_user_input` = `Run` + 无通知；Web 调用不变。
- 启动时 `hold_unclaimed_inputs` 只处理「未认领且未 held」，已 held 的不会再生成「上次有消息未执行」通知。
- 错误沿用 `KernelError` / `StoreError`，无新增变体。

## 三、微信入站

`IncomingContent::Unsupported` 携带媒体类型，按 item 原顺序与文本一起打扁成 Text 片段：

| item | 占位 |
|---|---|
| 图片 | `[图片]` |
| 语音无转写 | `[语音]` |
| 文件 | `[文件：{file_name}]`，无文件名时 `[文件]` |
| 视频 | `[视频]` |
| type 0 / 11 / 12 | `[不支持的消息]` |

- 含任一真实文本 → `append_explained_input(.., Run, ..)`（照常执行），模型在本轮就能从占位看出缺了什么。
- 全是占位 → `append_explained_input(.., Record, ..)`（只记录）。
- 处置说明随输入同一次调用提交，源 `wechat`，文案写明类型与处置：
  - 只记录：「微信渠道暂不支持图片，这条消息仅作记录。」
  - 混合：「微信渠道暂不支持图片，仅将文字部分交给 AI 处理。」
  - 多种类型按首次出现顺序去重，以「、」连接；同类型多项保留各自占位，解释不重复类型。

临时适配退出条件：某类内容具备真实处理能力后，该类走真实内容入站，删除其占位与「不支持」说明；长期记录、Notification 和执行处置契约继续保留。历史占位仍按当时事实读取，不回填为真实媒体。

## 四、调用方

| 调用方 | 改动 |
|---|---|
| `mic-channel-wechat` service / client | 按 §三 打扁与分流 |
| `mic-gateway` | 不变（Web 图片照常作为图片输入） |
| `mic-message` 匹配方 | 微信 delivery 用 `{ text, .. }`，model_view 解构补 `..`；其余为构造处补 `about: None` |
| Web `api/types.ts` 解码器 | `Notification` 增加可选 `about`；展示不变 |
| `bin/micnext` `-p` 输出 | `Notification` 解构补 `..`，行为不变 |
| Web `api/decode.ts` | 新增 `optional` 解码器；`obj` 对缺省字段传 `undefined`，非 optional 字段仍报错 |
| core kernel | `append_user_input` 改走共用私有路径，行为不变；新增 `append_explained_input` |
| core 调度 / recovery | 不变：已按 `UNCLAIMED AND NOT HELD` 认领；recovery 的 held 排除规则保留 |
| store 上下文窗口 | 按 §二 增加关联过滤 |
| Web 消息列表 | 不变：held 输入按普通用户消息显示 |

依赖方向不变。

## 五、输入与解释的并发一致性

实体与写者：

| 实体 | 写者 | 状态/转换 |
|---|---|---|
| 入站内容与媒体事实 | 微信 client / service | 上游 item → 强类型媒体事实 → 真实文本与占位 |
| UserInput 与关联解释 | store 同事务写入，core 发布 | 不存在 → 两条同时可见（id 相邻，输入在前） |
| 输入执行处置 | store，core 调度消费 | Pending：未认领 → 调度或当前 run 的 absorb 认领；Held：落盘即 held，始终不被认领 |
| 模型上下文 | core run 读取 store | 每次模型调用读取窗口：已认领或 held 的输入；非输入消息中，带 `about` 的通知仅当其输入在窗口内 |

关键场景与目标 sequence：

1. 空闲会话收到图文：事务写 Pending 输入 + 通知 → 按 id 发布两条 → 唤醒 → 调度认领 → 读窗口。模型首次看到输入时通知已可见。
2. 执行中会话收到纯图片：事务写 Held 输入 + 通知 → 发布，不唤醒。当前 run 下次读窗口时两条同时出现。
3. 执行中会话收到图文：事务写 Pending 输入 + 通知。提交若落在 absorb 与读窗口之间，本次窗口过滤掉该通知；之后输入被认领时两条一起出现。认领时机仍受 person 与 run 状态等既有规则约束，关联过滤与之无关。

不变量：

- 对同一条入站，模型窗口内输入与解释同时可见或同时不可见；可见时顺序为输入 → 解释。不得只见占位，也不得只见解释。适用范围为本功能产生的输入/解释对，既有独立 Notification 不受此配对规则约束。
- Held 不触发新执行，但可进入正在执行的 run 的后续上下文；Pending 继续遵守既有认领规则。
- 事件只在事务提交后发布，按 id 顺序；发布两条事件时共用发布锁，唤醒在发布之后。模型读取可见性由读取侧契约保证，不能靠事件发布顺序保证。
- 写入失败 → 两条都不存在，返回 Err；微信 service 沿用既有处置（Service Err，批次保留 importing，重启转 interrupted 不重导）。这是既有失败行为，本步不扩展入站恢复能力；验收须区分落盘原子性与重导保证。

审查结论：

- **accept / 本次改动修正**：同事务写入输入、hold 与解释，保留枚举处置及提交后发布方案。
- **reject / 本次契约缺陷**：不接受「先看到通知、下一次补原话」。
- **accept / 收敛为候选 A**：解释显式关联输入，窗口按关联过滤。一条读取规则覆盖全部场景，不改认领时机与 run 循环。
  候选 B（认领与读窗口同事务）不采纳：person 不匹配的 Pending 输入无法并入，其解释仍会单独可见；且需改全部模型调用路径。
- 关联只有窗口一个消费者；消费者消失时字段一并删除。不靠相邻 id、通知文案、重复通知或延时推断关联。

## 六、不在本步

以下场景按同一规则后续逐个处理，各自判断是否需要 B2：

- 当前模型不支持图片：遵循长期规则；占位与解释的生成位置、时序及是否重复落盘另行确定。
- `InputError` 整条拒收是否改为入记录。
- 微信投递失败是否入记录。

## 七、验收

- 空闲会话微信只发图片：Web 历史出现用户「[图片]」与系统提示，不启动新 run；再发文字，实际模型请求包含前面的图片占位与提示。
- 微信图文混发：本轮执行；实际模型请求包含按原顺序排列的文字、占位及处置 Notification，微信收到同文提示。不以模型是否主动提到图片作为确定性验收。
- 正在执行时收到纯图片或图文：每份实际模型请求中，该输入与解释同时出现或同时缺席；出现时输入在前；纯占位不会另开 run。
- 定点复现 Pending 图文在 absorb 后、窗口读取前提交：该次请求不含该通知；后续认领后两条同时出现。用实际请求检查，不以模型回复措辞验收。
- person 不匹配时 Pending 输入不能并入当前 run：关联解释也不得单独进入当前请求；后续可认领时再一起出现。
- 注入写入失败：输入、hold、解释均不残留，微信沿用既有 interrupted 处置；不承诺重启重导。
- 只有文字：照常执行，不增加本功能的 Notification；同类媒体多项只解释一次类型。
- 用户手打 `[图片]`：作为真实文字执行，不按占位判定。
- 重启后不对上述 held 输入补发「上次有消息未执行」。
- 旧库中的 Notification 照常读取与展示（`about` 缺省）；Web 历史正常加载带 `about` 的通知。
- `cargo fmt`、`cargo clippy -- -D warnings`。
