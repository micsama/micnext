# B2: system prompt 结构与消息头说明

**状态**: CLOSED（2026-10-09 批准并实现于 `mic-message/src/model_view.rs`、`mic-core/src/request.rs`）
**改动 crate**: `mic-message`（新增 1 个公开函数）、`mic-core`（私有）
**依赖方向**: 不变，`mic-core → mic-message` 已存在。

## 一、动机

1. 统一管理消息头的解释。消息头格式写在 `model_view.rs`，说明手写在 `system.md`，两处已漂移：代码有 7 种头，
   `system.md` 只说明 5 种，`[dispatched …]`、`[completion …]` 未说明。
2. system prompt 用少量顶层块区分框架规则、用户设定和运行上下文，便于维护；具体段落随实际需求迭代。

## 二、决定（2026-10-09 与 human 确认）

- XML 标签为最外层，只分 3 块；块内为普通文本，不再嵌套标签。
- 保留 `mic-message::model_view` 内私有 `Header`，标签、属性名、说明同在一处，实际渲染与说明共用定义。
- 去掉 `render(values: &[&str])` 的位置参数协议，各种头用命名字段携带取值。
- 完整性由 review 守：新增头同步说明清单；修改失败分类同步解释，不增加枚举遍历宏或自动发现机制。
- 本次仅统一消息头解释与 system 布局，不重构落盘消息、Provider 协议或模型输入所有权，不引入可插拔 prompt 框架。

## 三、system prompt 布局

顺序固定，使内容未变的前缀保持稳定；空的可选段整段省略。

```
<instructions>
{system.md}

{mic_message::header_legend()}
</instructions>

<persona>
{人设提示词}

User preferences:          ← 通用偏好非空时
{通用偏好}
</persona>

<context>
Working directory: {pwd}

{渠道提示}                 ← Root 会话且渠道已声明时

{各工具 prompt_hint，按工具顺序各占一段}

Summary of the earlier conversation:   ← 有摘要时
{摘要}
</context>
```

| 块 | 归属 | 谁能改 |
|---|---|---|
| `instructions` | 框架规则 + 消息头说明 | 只读，随二进制 |
| `persona` | 助手怎么做事 | 用户（人设、通用偏好） |
| `context` | 工作目录、渠道提示、可用工具提示、会话摘要 | core 组装；渠道提示由渠道声明（[channel-prompt](channel-prompt.md)），工具提示由工具定义，摘要来自会话 |

块的含义靠标签名自明，`system.md` 不另作说明。三块是文本组织方式，不映射为三个 Rust 模块；段落划分随实际需求迭代。

块内正文原样放入，不做 XML 转义：模型不解析 XML，人设与偏好由 owner 本人填写。
若以后摘要引入外部内容，再单独决定是否只中和三块框架标签（保留字式），不整体转义。

设置页「系统提示词（只读）」展示完整的 `instructions` 块（含标签）。

## 四、`mic-message` 契约

### 4.1 新增公开函数

```rust
/// 全部消息头的说明，供 system prompt 使用；英文，多行，无首尾空行。
pub fn header_legend() -> String;
```

`lib.rs` re-export。纯函数，不可失败。

### 4.2 私有枚举（`model_view.rs`）

```rust
#[derive(Clone, Copy)]
enum Header<'a> {
    User { id: &'a str, at: &'a str },
    RuntimeNote,
    Notification { src: &'a str, at: &'a str },
    Completion { exec_id: &'a str },
    Dispatched { exec_id: &'a str },
    Failed { kind: &'a str },
    Cancelled,
}

impl<'a> Header<'a> {
    fn tag(self) -> &'static str;              // "user"、"runtime-note" …
    fn fields(self) -> Vec<(&'static str, &'a str)>; // 命名字段绑定属性名与取值，最多两个
    fn meaning(self) -> &'static str;          // 一句英文说明，穷尽 match
    fn render(self) -> String;                 // `[tag k=v …]`，取值经 header_value 转义
}

const ALL: [Header<'static>; 7]; // 每种头一个样例，所有取值均为 "…"
```

- `Header` 只处理模型可见的方括号头；存储仍使用 `MessageBody`，不增加落盘格式或事件。
- 数字、时间、失败分类仍从现有强类型数据得到文本，再构造相应的命名字段；头不负责解析或分类。
- 属性名仅在 `fields()` 中定义；`render()` 复用 `tag()` 与 `fields()`，不接受无名取值数组。
- `model_view` 中所有头改由对应 `Header` 变体渲染，头后正文的拼接保持现状，消息输出逐字节不变。
- `header_legend()` 遍历 `ALL`，复用 `render()` 和 `meaning()`：一行引导语 + 每种头一行说明。
- `ALL` 紧邻枚举；穷尽 match 保证各变体有定义，但不保证清单完整。新增成员须在 review 中核对 `ALL`。
- `ExecFailureKind` 的分类语义仍归其现有定义；分类改变时同步 `Failed` 的说明。

### 4.3 说明文本

```
Bracketed headers are added by the framework:
- [user id=… at=…]: a user message.
- [runtime-note]: a framework notice, not the user.
- [notification src=… at=…]: an event from src at that time, not the user; it may no longer hold now.
- [dispatched exec_id=…]: a background tool started; its result arrives as a completion.
- [completion exec_id=…]: the result of that background tool.
- [failed kind=…]: the tool call failed (input: fix the arguments; business: rejected by tool rules; dependency: environment error).
- [cancelled]: the call was cancelled or interrupted; its effect may be unknown, so check before retrying.
```

`completion` 后可接 `[failed …]` 或 `[cancelled]`，分别说明任务身份和结果；解释适用于头出现的位置，
不要求所有状态头都在消息首行。后台完成与派发按 `exec_id` 关联。

## 五、`mic-core` 改动（私有）

- `request.rs`：`system_prompt()` 按 §三 组装；`instructions` 块用 `LazyLock<String>` 拼一次。
- `base_prompt()` 返回该块；`Kernel::system_prompt() -> &'static str` 签名不变。
- `prompts/system.md`：删去手写的消息头段落，其余收紧措辞；运行规则与确认破坏性操作的规则保持。

## 六、调用方兼容

| 调用方 | 影响 |
|---|---|
| `mic-message::model_view` | 内部改走 `Header`，模型看到的消息字节不变 |
| `mic-core::request::build` | 产出的 system 文本结构变化；各 Provider 只透传，无需改 |
| `Kernel::system_prompt` / `mic-gateway` 设置接口 | 签名不变，内容变为完整 `instructions` 块 |
| Web 设置页 | 只读展示，无需改 |
| 已有会话 | system 文本变化可能影响 Provider 的 prompt cache；内容未变的前缀保持稳定，不承诺缓存命中 |
| 已有测试 | 若断言 system 文本需随之更新 |

## 七、历史决策与文档同步

- 消息呈现仍沿用 `provider-port.md` §3.5 的 `mic-message::model_view` 所有权；本次将消息头解释移到渲染侧维护，
  替换 `system.md` 中的手写说明，不改 role、正文、属性名和转义行为。
- `orthogonality-review-2026-09-24.md` §一的模型输入所有权重构仍是未决 B1；本次不批准或撤销该方向。
  如以后迁移呈现，消息头定义与解释须一起迁移，core 仍只消费说明。
- `runtime-settings.md` Q2 与 `run-execution.md` §4.4 的原顺序细化为 §三布局；人设和偏好的 run 快照语义保持。

实现时同步以下已批准契约的描述：

- `provider-port.md` §3.5：补一句「头的格式与说明由私有 `Header` 唯一定义，`header_legend()` 对外提供说明」。
- `run-execution.md` §4.4 system prompt 一条与附录 A：改指向本文 §三。
- `runtime-settings.md` Q2：分块描述改指向本文。
- `mic-message.md`：公开 API 增加 `header_legend`。
- 实现验收后标本文 `CLOSED` 并更新 `todo.md`；草案修订不提前修改已批准契约。

## 八、验收

- `cargo fmt`、`cargo clippy -- -D warnings` 通过。
- 设置页「高级」显示带 `<instructions>` 的完整块，含 7 行消息头说明。
- Review 核对七种头的格式、转义与正文拼接保持原行为，说明短而准确，`ALL` 无遗漏。
- 发起一轮带工具调用的对话，工具执行与回复正常；从设置页核对说明与标签。
