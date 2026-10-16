# 过度设计 / 过度防御审计（2026-10-12）

状态：审计结论，未改代码。A 组属 B2 豁免可直接修；B、C 合成一份 B2（`dead-code-cleanup`）待 human 拍板 C 与 D1 后起草；D 组交 human 决定。

范围：全部 crate、二进制与 Web 前端。方法：逐文件阅读 + 扫描零调用公开项与未构造变体 + `git log -S` 查来源 + 必要时实测上游。

起因：DeepSeek `reasoning_content` 回传 bug（3912c3b）暴露「仅工具轮回传，省 token」是初版蓝图未经验证的假设。

## 总体结论

过度设计主要集中在两处：

- **路线图脚手架**：从未产生的数据形态，散布在 message / store / core / gateway / 二进制 / Web 六层。
- **微信模块的磁盘重复校验**。

provider 基本干净，只有一处与蓝图矛盾；gateway 与工具类 crate 多为 CLAUDE.md 要求的边界校验，问题很少。

## A. 私有项，可直接改（B2 豁免）

**处理结果（2026-10-12）**：A1、A3 已改；A5 只做了 crate 内部分（`resolve_data_dir` 复用 `resolve_abs`，store 的 `now_ms` 由静默回 0 改为与其余三份一致的 `expect`），跨 crate 合并需在 `mic-message` 新增公开函数，走 B2，暂不做；A4 不改：库内 `config_json` 属磁盘外部数据，升级后旧行可能解析失败，`build` 会以「配置有误」报错，`expect` 会把它变成 panic；A6 随 D1 决定；A2 随 C 决定。

| # | 位置 | 问题 | 依据或来源 |
|---|---|---|---|
| A1 | `mic-provider-openai/src/stream.rs:67` `finish` | 正常结束（EndTurn）但没有内容时按 `Transient` 重试 | 326e21f（2026-09-24）初版沿用至今。[provider-sdk-responses](../blueprints/provider-sdk-responses.md) §六（2026-10-10 批准）写明「空正文的合法终态不再猜测重试」，但代码和 [provider-openai](../blueprints/archive/provider-openai.md) 第 14、110、148 行仍保留，两份文档互相矛盾，一处真相被破坏。删除前须确认 core 能正常落盘空 `Reply`。 |
| A2 | `mic-message/src/model_view.rs:173` `header_legend()` | 每个 system prompt 都在描述 dispatched / completion 两种消息头，但这两种消息从不出现：白花 token，也可能误导模型 | 依赖 C 的决定：删脚手架就一起删；保留脚手架也应只描述实际会出现的消息头 |
| A3 | `mic-provider-openai/src/chat.rs:240` `FinishReason::FunctionCall` | 变体只为报错而存在；去掉后 serde 照样解析失败 | — |
| A4 | `mic-provider-openai/src/factory.rs:43` `key_env` | `.ok()?` 把配置解析错误吞成 `None` | 库内配置已由工厂 check 规范化，应 `expect` |
| A5 | `now_ms` 四份拷贝（`mic-core/run.rs:432`、`mic-gateway/api.rs:341`、`mic-store/schema.rs:250`、`mic-channel-wechat/account.rs:462`）；`mic-core/assembly.rs` 的 `resolve_data_dir` 重复了 `resolve_abs` | 一处真相 | 小改动 |
| A6 | `mic-channel-wechat/src/delivery.rs:74` `stored()`；`delivery.rs:351` `finish()` | `stored()` 把 outcome 解析成 4 变体后 match 每个分支都是 `{}`，实际只用到「记录存在」，plan 也是解析完即丢；`finish()` 收 4 变体枚举却对其中 2 个写 `unreachable` | 与 D1 相关：若 D1 采用「行数据可信」，这里一并简化 |

## B. 公开 API 死代码（删除走 B2，不改变行为）

| 项 | 现状 | 来源 | 连带可删 |
|---|---|---|---|
| `Store::bind_identity`（`store.rs:136`）、`Store::set_pwd`（`store.rs:276`）、`Store::person`（`store.rs:202`） | 零调用（含测试） | 509c212 起未使用 | — |
| `Store::sessions_with_unclaimed_input`（`store.rs:738`） | 零调用 | 消费方在 ebfb1c4 删除 | — |
| `Store::open_in_memory`（`store.rs:48`） | 零调用（含测试） | 509c212 / bca566c | `Cipher::random`、`Store.diagnostic` 外层 `Option`、`DiagnosticError::Unavailable`、gateway `ApiError::Unavailable` 的诊断映射 |
| `Kernel::append_notification`（`kernel.rs:152`） | 零调用 | 消费方在 903422c 删除 | — |
| `Kernel::create_session`（`kernel.rs:99`）→ `Store::create_session` | 零调用，注释「Task/Triggered 用」 | 脚手架，见 C | — |
| `Activation::WhenConfigured`、`Module::activation` 缺省实现（`module.rs:15-30`） | 全部 6 个模块都覆盖为 `Always`，缺省值从不生效 | — | `assembly.rs:144` 的分支 |
| `GatewayModule::with_restart_requests` 与 `restart: Option` | 二进制总会传入；`Option` 与运行期「未接入重启通道」报错在防不会发生的情况 | 4996bd3 | 改为构造参数 |
| `AssembleError::RemovedModelsConfig`、`[core] max_turns` 提示（`assembly.rs:87-101`）、`[gateway] workdir` 的 `IgnoredAny`（`gateway/config.rs:17,30`） | 旧配置迁移提示 | — | `deny_unknown_fields` 本就拒绝这些项：删后仍 fail fast，只是报错从特定提示变通用 |
| `GET /api/models/default`（`gateway/models.rs` `get_default`、`DefaultView`） | 前端只用 PUT；`GET /models` 已返回 `default_model_id` | — | — |

删除四项检查（正确性、隐私边界、失败行为、可审计性）：以上均无生产者或无调用方，删除不改变任何可观察行为。迁移提示一项改变的是报错文案，失败行为不变。

## C. 路线图脚手架（交 human 决定）

从未产生过的形态：

- `SessionKind::Task` / `Triggered`、`parent_session_id`（总为 `None`）、`ToolScope::Only`（总为 `All`）
- `MessageBody::Boundary` / `ContextBoundary::{Compaction, UserClear}`
- `MessageBody::Completion`、`ToolResultOutcome::Dispatched`
- `ContentPart::File` / `FileRef`（2026-09-23 起无生产者；微信文件走文本路径）

对应 roadmap v1/v2：压缩、cron、异步子 agent（见 `todo.md`）。

波及范围：

| 层 | 位置 |
|---|---|
| mic-message | 类型定义；`model_view` 渲染与 `header_legend` |
| mic-store | `row.rs` 解码；`context_window`（`store.rs:833`）每次扫描 Boundary；`absorb` 查询带 `'Completion'` |
| mic-core | `kernel.rs` `session_channel` 沿 Task 父会话上溯的循环；`create_session` |
| mic-provider-openai | `chat.rs:188`、`responses.rs:199`、`content.rs:20` `unsupported_file` |
| mic-gateway | `api.rs` `item()` 返回 `Option` 配 `expect` |
| bin/micnext | `main.rs:255` `-p` 渲染 File |
| Web | `api/types.ts` 解码器；`MessageItem.svelte` 渲染 Completion / Boundary / File；`ToolRow.svelte` 的 dispatched；`outcome.ts` |

**推荐全删**。依据：CLAUDE.md「抽象有进有出：消费者消失时连生产者一起删」「不为假想未来上重机制」。roadmap 真正动工时形状大概率会变，届时走 B2 加回。删前用诊断 SQL 确认库内无这些种类的数据行。

## D. 需 human 拍板的设计取舍

### D1. 微信模块的磁盘重复校验

- 约 25 处 `AccountError::Invalid`，重新校验模块自己在同一事务内写入的行。例：`account.rs` `find()` 交叉核对会话种类、投递目标版本与 user_id；`delivery.rs` 的 `stored()` / `recover()` 解析 plan 后丢弃。
- 表上已有 `CHECK` 约束枚举值（`connection`、`phase`、`outcome`、`attempt_no`）；另有逐行 `plan_version` / `snapshot_version`，与 schema 迁移版本并存。
- 但 [wechat-channel](../blueprints/archive/wechat-channel.md) 第 198、221 行明确要求「磁盘边界一次解析成强类型」，CLAUDE.md 也把「磁盘上的库文件版本」列为外部，所以现状合规，不是 bug。

分歧在于这条规则的解释：

- 解释一：「磁盘边界」只指 schema 迁移版本，行数据默认可信。
- 解释二：每次读行都重新校验。

**推荐解释一**：约一半校验可删，逐行 version 列可去。user_id 归属检查属隐私边界，保留。

### D2. max_turns 收尾轮发 `tools=[]`

`run.rs:153` 达到上限时以空工具调用模型。tools 属于缓存前缀，去掉会让整段前缀缓存失效。替代：保留 tools，加 `tool_choice: "none"`。待实测 DeepSeek 与 Responses 是否接受后再决定。

### D3.（低置信）`ExecFailureKind` 三分类

只作给模型看的标签，未查来源。

### D4.（低置信）`wechat` 可选 feature

`bin/micnext/Cargo.toml` 的 `wechat` 是可选 feature，`build.sh` 从不关闭。它带来 gateway `WechatView::Unavailable` 与前端 `available: false` 分支。保留它符合「模块只由二进制装配」，删除收益小。

## 已核实，不算问题

- **DeepSeek `reasoning_effort` 白名单**：2026-10-12 实测 `deepseek-flash`，`none` / `low` / `high` / `max` 均被接受，`none` 确实关闭思考（无 `reasoning_content`）；`medium` / `xhigh` 也被接受。白名单比 API 窄，无害。
- `mic-tool-fs/src/grep.rs:140` 单文件读失败跳过：与 rg 一致。
- gateway 入参校验、Responses 同模型密文回传、`request.rs` 的 `order()`（工具结果必须相邻）、`token.rs` 本地检查 exp。
- 微信投递 user_id 归属检查：隐私边界。

## 建议顺序

1. A1、A3–A6：私有改动，fmt + clippy 收尾。A1 先确认空 Reply 落盘路径。
2. human 拍板 C 与 D1 后，把 B + C 合成 `docs/blueprints/dead-code-cleanup.md`（B2），枚举全部调用方，批准后实现。
3. D2 先实测 `tool_choice: "none"`。
