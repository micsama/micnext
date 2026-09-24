# v0a 模块地图：Web 闭环

**状态**: 已批准（2026-09-23；模块划分、依赖方向、对接方式、实现顺序）。步 1、2、3、3' 已完成
**范围**: [`product-roadmap.md`](../brainstorm/product-roadmap.md) §三 v0a——DeepSeek 官方 + OpenAI 兼容两种模型的流式对话、
六个基础工具 + `web_fetch`、Web 多会话列表与切换、断线回放、固定 token、崩溃收尾。
微信（v0b）排在 Web 打通之后，本图不含。

**用法**: 本文是 v0a 的总索引。每次只做一个模块：该模块的 B2 **及它所用接口所在的 B2
均已批准且与本文一致**时，读 `CLAUDE.md` + 本文 + 这些 B2 即可动工，不需要其它模块的
实现细节。模块边界或对接方式要变，先改本文。

## 一、模块与依赖

```
                     bin/micnext（装配根）
        ┌──────────┬──────┴─────┬───────────────┐
   mic-gateway  mic-provider-openai  mic-tool-fs / -shell / -web-fetch
        │  (+web 前端产物)│            │
        └──────────┴──── mic-core ───┘（port 与主路径）
                        │      │
                  mic-store  mic-tool
                        └──┬───┘
                      mic-message
```

箭头方向见 `CLAUDE.md` 依赖不变量：功能模块只依赖 `mic-core`（port）+ 需要的下层 crate，
模块之间互不依赖，只在 `bin/micnext` 装配。

| # | 模块 | crate | 职责 | B2 | 状态 |
|---|---|---|---|---|---|
| M1 | 消息类型 | `mic-message` | Session 条目数据类型 | [mic-message](mic-message.md) | ✅ 已实现 |
| M2 | 持久化 | `mic-store` | 内核表、claim、迁移、会话列举 | [mic-store](mic-store.md) | ✅ 已实现 |
| M3 | 工具 port | `mic-tool` | `Tool` trait、模型可见说明与使用提示、参数边界解析、结果与错误（沿用 micbot 输入/业务/依赖三分类，同步改 `ExecOutcome` 与模型视图的 `[failed]` 头） | [mic-tool](mic-tool.md) | ✅ |
| M4 | 装配 | `mic-core` | Module / Registry / Service / Kernel、配置、启动与退出 | [mic-core-module](mic-core-module.md) | ✅ 已实现 |
| M5 | 模型 port | `mic-core` | `Provider` trait、流式事件、用量、失败分类；`ToolSpec`（放 `mic-tool`，M3 沿用） | [provider-port](provider-port.md) | ✅ |
| M6 | 执行主路径 | `mic-core` | 调度（claim/串行）、Agent 循环、落盘、崩溃收尾、实时事件 | [query-execution](query-execution.md) | ✅ 已实现 |
| M7 | 模型实现 | `mic-provider-openai` | Chat Completions 协议的 `Provider`：DeepSeek 官方预设（只填模型名 + key，key 缺省读 `DEEPSEEK_API_KEY`）与通用 OpenAI 兼容（地址、key、模型名、自定义请求头） | [provider-openai](provider-openai.md) | ✅ |
| M8 | 基础工具 | `mic-tool-fs`、`mic-tool-shell`、`mic-tool-web-fetch` | `read/glob/grep/write/edit`、`bash`、`web_fetch` | [tools-basic](tools-basic.md)（一份覆盖三个 crate，含 `Module::activation` 修订） | ✅ |
| M9 | 网关 | `mic-gateway`（由 `mic-channel-web` 改名） | HTTP API、固定 token、SSE、嵌入前端产物 | gateway（待起草） | — |
| M10 | Web 前端 | `web/`（TS） | 会话列表/切换、流式显示、断线重连 | web-ui（待起草） | — |
| M11 | 装配根 | `bin/micnext` | 读配置、组装模块、信号、日志；一次性 `-p` 调试入口 | 随 M4；`-p` 随 M6（[query-execution](query-execution.md) §六） | ✅ 已实现（含 `-p`） |

## 二、对接方式

模块之间只经下表的接口对接；接口定义归"定义在"一列的 B2，使用方不复述。

| 对接 | 提供方 → 使用方 | 形式 | 定义在 |
|---|---|---|---|
| 装配 | 各模块 → mic-core | 实现 `Module`，在 `install` 里向 `Registry` 登记贡献 | M4 |
| 工具 | M8 → mic-core | `Registry::tool(impl Tool)`；模型可见说明由工具自带，core 只汇总 | M3 |
| 模型 | M7 → mic-core | `Registry::provider(name, impl Provider)`；模型统一配在 `[models.<名字>]`（`kind` 选实现），`[models] default` 选用哪个 | M5 |
| 入站 | M9 → mic-core | `Kernel` 上的写入口：`resolve_root_session`（建/取会话，M4 实现）、`append_user_input`（写入并唤醒调度，M4 占定、M6 实现）；Gateway 不直接写 Store | M4 / M6 |
| 实时事件 | mic-core → M9（及以后各 Channel） | `Kernel::subscribe` 订阅全部会话，事件带 `session_id`/`channel` 供过滤：文本/推理增量、草稿结束、entry 已落盘（带 id）、Query 起止；增量与落盘 entry 的关联见 query-execution §3.1；事件可丢，真相在 Store | M6 |
| 稳定历史 | mic-store → M9 | 只读：`entries_after`、`list_root_sessions`、`session_usage`；以 `Kernel` 上的只读委托方法提供，随 M9 B2 与消费者一起加 | M2 / M6 |
| 前后端 | M9 ↔ M10 | HTTP JSON + SSE；衔接顺序见下 | M9 |

v0a 已定的简化（各 B2 照此写，不再讨论）：

- **回放与实时的衔接**：**先订阅、后回放**。Gateway 先订阅该会话的事件并缓冲，再按
  客户端游标读 `entries_after` 回放到当前最新 entry id。缓冲期内已被回放的输出，
  其增量和落盘事件都不能再呈现；M6 B2 定义增量与落盘 entry 的关联方式，M9 B2
  定义过滤和交接顺序。稳定游标只能是 entry id，增量 chunk 不作游标；中途接入时
  正在生成的那条只显示接入后的增量，落盘事件到达后以稳定 entry 为准。事件缓冲
  溢出（订阅方落后）→ 断开 SSE，客户端按游标重连补齐。
- **v0a 不定义出站 port**：Web 的回复靠上一条的事件流 + 回放呈现，不走
  `pending_deliveries`；Web 会话 `delivery_target = None`。
- **v0b 出站方向未定**：倾向"core 只产事件 + 落盘，各 Channel 自行消费和记账"，但
  投递完成判据、重启补发、适配器形态仍是 roadmap §四-3 的待核实项，由微信 B2 决定，
  届时可能需要改 core 接口。v0a 只保证事件流可按会话或全部订阅、带落盘 entry id 与
  Query 终态，不为微信预设更多。
- **单用户**：`[core]` 配置声明 owner person；固定 token 即 owner，Web 身份绑定到它。
- **Web 新建会话** = Gateway 生成 `chat` 后调 `Kernel::resolve_root_session`，首条消息后进入列表。
- **工具默认放行**，可达范围 = 进程 OS 权限（roadmap §2.3）。
- **配置文件是唯一真相**：v0a 手写；以后 Web 设置页改的是同一个文件（保留注释写回），
  保存后重启生效，与微信设置同一套，不做热更新。配置与数据位置按 XDG 约定，默认
  `~/.config/micnext/config.toml` 与 `~/.local/share/micnext/`，可用 `--config` 与
  `[core] data_dir` 覆盖（见 M4 B2 §四）；含 key 的配置文件权限应为 600。设置页的写回接口随 Web 设置页的 B2 定，不在 v0a。

## 三、实现顺序与单独验收

每步完成即可独立验收并提交；同一行内的模块互不依赖，可任意先后。步 3 与 3' 共用的 `ToolSpec` 由 M5 定义在 `mic-tool`，两步互不阻塞。

| 步 | 模块 | 单独验收 |
|---|---|---|
| 1 | M2 会话列举增量 | 临时工程调用：排序、翻页、预览截断 |
| 2 | M4 + M11 骨架 | `micnext --config x.toml` 起停：建库、未知配置段/字段报错、Ctrl-C 干净退出 |
| 3 | M3 → M8 | 临时工程直接调用各工具：参数解析错误、执行结果、错误形状 |
| 3' | M5 → M7 | 临时工程发一轮流式请求，打印增量与用量 |
| 4 | M6 + `-p` | `micnext -p "列出当前目录"`：落盘回复与工具调用；杀进程后重启收尾为 Interrupted |
| 5 | M9 | curl：token 校验、建会话、发消息、SSE 收流、断线后按游标回放、会话列表 |
| 6 | M10 | 浏览器：多会话切换、流式显示、刷新/断网后恢复 |

## 四、技术选择

纯技术选择（前端构建工具、HTTP 框架、日志库等）在对应 B2 里由 AI 定，附理由。
