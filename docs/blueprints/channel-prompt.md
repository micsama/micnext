# B2：渠道提示

**状态：CLOSED（2026-10-10，服务器验收通过；含 B3 三处修订）。**

## 一、动机

模型不知道自己在哪个渠道。按用户实测，微信支持大部分 Markdown，代码块可点击复制但无高亮，
不支持数学块、脚注和折叠块；用户看不到工具执行过程。Web 支持 Markdown 并实时展示工具过程。
渠道的呈现特点只有渠道模块自己知道，core 不能依赖具体渠道，所以由渠道在装配时声明，
core 只负责放进 system prompt。最终消费者是模型；直接收益是回复适合渠道呈现，微信避开不支持的格式，工具调用时附带的进度文字保持在用户舒适的频率。
提示只描述界面特点与表达适配；回复详略仍由用户要求、人设和通用偏好决定。

## 二、契约

`mic-core` 新增一个公开方法和一个公开错误变体，其余改动为私有：

```rust
impl Registry {
    /// 本渠道 Root 会话的呈现说明，进入 system prompt `context` 块；同一渠道只能声明一次。
    pub fn channel_prompt(&mut self, channel: &'static str, prompt: &'static str);
}

pub enum AssembleError {
    // 新增
    DuplicateChannelPrompt { channel: &'static str },
}
```

- 可选：没声明的渠道不加这一段，与工具的可选 `prompt_hint()` 一样。
- 重复声明 → `AssembleError::DuplicateChannelPrompt`，启动失败。
- 只对 `SessionKind::Root { channel, .. }` 按自身 `channel` 查找。Task、Triggered 不加渠道提示，受众规则随其生产者另定。
- 不使用事件归属 `session_channel` 或投递目标推断界面；事件与投递契约不变。
- 注册键显式传入渠道名：Gateway 的模块名是 `gateway`，其会话渠道是 `web`，不取 `Registry.current`。

## 三、数据流

`Registry.channel_prompts` → `Assembly` 校验去重成 `HashMap` → `Engine.channel_prompts` →
`Exec` 根据已有 `session.kind` 选择提示 → `request::build` 多收一个 `Option<&str>` →
`context` 块中紧跟 `Working directory`，作为独立段落。

提示是装配时固定的静态文本，同一会话执行期间保持稳定；升级提示会改变 system 文本，不承诺缓存命中。
设置页只读展示的 `instructions` 块不变。Provider 继续透传 system 文本，不改消息、事件、落盘和取消路径。

微信告知与工具调用放在同一次模型回复，经已有 Reply 落盘与投递路径发送。
告知是模型行为建议；core 不等微信发送完成再执行工具，不增加投递 ack 或事件。

## 四、调用方与声明

| 调用方 | 改动 |
|---|---|
| `mic-channel-wechat` install | 以 `wechat` 登记下方微信提示 |
| `mic-gateway` install | 以 `web` 登记下方 Web 提示 |
| `Assembly::new` / `Engine` / `Exec` / `request::build` | 私有字段与参数同步调整，按 §三传递提示 |
| `bin/micnext` 装配错误收口 | 现有 anyhow `with_context` 兼容新增错误，无穷尽匹配 |
| `-p`（core 内 `cli`） | 不声明，执行路径不变 |
| Provider / 设置页 / 其他模块 | 公开调用不变，无需调整 |

微信：

```text
WeChat: Markdown mostly works (no math, footnotes, collapsibles; code blocks unhighlighted). Tool activity is invisible, but text sent with tool calls reaches the user as messages: keep such updates short and occasional.
```

措辞靠长期使用磨合，不追求一次定稿；改文本不改契约。

Web：

```text
Web: Markdown is supported; tool activity is visible live.
```

依赖方向不变：渠道 → `mic-core`。文本直接写在各自 install 的登记处，与工具 `prompt_hint()` 的先例一致；不放 `limits.rs`（那里只放数值旋钮）。

## 五、同步文档

批准后随实现同步：

- `system-prompt-layout.md`：`context` 内容与来源补可选的 Root 渠道提示。
- `mic-core-module.md`：补 Registry 方法与装配错误变体；公开错误枚举的穷尽匹配者需处理新增变体。
- `wechat-channel.md`：补渠道提示声明，说明告知复用 Reply 投递、无送达先于工具执行的保证。

`run-execution.md` 已引用 system 布局，不重复维护提示文本。

## 六、验收

- 核对模型请求：微信/Web Root 带各自提示；`cli`、Task、Triggered 不带；其余 prompt 块保持现有布局。
- review 核对：去重逻辑与 `DuplicateChannelSetup` 同构，重复登记返回 `DuplicateChannelPrompt`（不为此写测试）。
- 微信发需要多步工具的问题：观察带工具调用的 Reply 是否只含简短、频率适度的进度文字，正文避开数学块、脚注和折叠块；告知沿现有路径按消息顺序尝试投递。
  普通 Markdown 和代码块可正常使用，代码块可复制但不要求高亮。
  不以用户收到告知先于工具执行作为验收条件。用户要求详细说明时，回复仍可展开。
- Web 同样问题可正常使用 Markdown；`-p` 正常执行。
- `cargo fmt`、`cargo clippy -- -D warnings`；只有改动覆盖已有测试时才跑对应测试。
