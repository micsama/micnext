# B2: Web 前端（web/，M10）

**状态**: CLOSED（2026-09-24 批准并实现于 `web/` 与 `crates/mic-gateway/src/web.rs`；§八 1～10 已用 DeepSeek `deepseek-flash` + Playwright 实跑通过）；2026-10-08 按 [`runtime-settings.md`](runtime-settings.md) 加设置页（§4.3）（`/settings`）与输入框旁人设选择
**来源**: [`v0a-module-map.md`](v0a-module-map.md) M10、步 6；[`gateway.md`](gateway.md)（调用的契约）；
[`product-roadmap.md`](../brainstorm/product-roadmap.md) §2.1、§2.2
**依赖不变量**: 新目录 `web/`（TS，不是 crate）。Rust 侧只改 `mic-gateway`：嵌入并服务构建产物，
新增外部依赖 `rust-embed`；不新增跨 crate 依赖。

## 〇、已定方向（2026-09-24 与 human 确认）

| # | 问题 | 决定 |
|---|---|---|
| 1 | 视觉风格 | 简洁中性（留白、中性灰、一个强调色）；跟随系统深浅色，可手动切换 |
| 2 | 思考与工具调用 | 折叠为一行、可展开；生成中的思考实时滚动显示，完成后自动收起 |
| 3 | 手机 | 基本适配：窄屏时会话列表变抽屉，聊天区全宽 |
| 4 | Markdown | 完整渲染，流式中也正确显示未闭合的代码块；代码块语法高亮 + 复制按钮 |
| — | 前序已定 | 无停止按钮、无用量显示（gateway Q5/Q6）；其它渠道会话只读、默认折叠（Q7） |

## 一、用户视角的效果

1. 终端打印的地址 `http://127.0.0.1:7878/#token=…` 在浏览器打开即可用；token 存进浏览器，
   地址栏里的 token 随即抹掉，以后直接打开 `http://127.0.0.1:7878/` 即可。
   token 缺失或失效（如重启后随机 token 变了）→ 整页提示"请用终端里打印的地址重新打开"；在同一标签页粘贴新地址
   （只有 `#` 后变化、页面不重新加载）同样生效。
2. 左栏：「新会话」按钮；Web 会话按最近活跃排列（今天 / 昨天 / 更早分组），滚动到底自动加载更多；
   最下方「其它渠道」分组默认折叠，展开后是 `-p` 等渠道的会话，只能查看。
3. 右侧：点「新会话」得到空白页，发出第一条消息后会话才出现在左栏（gateway Q4）。
   回复逐字出现；思考过程显示为"思考中…"并实时滚动，完成后收成一行"思考过程 ▸"；
   每次工具调用是一行"⚙ 工具名 参数摘要 ✓/✗"，点开看完整参数和输出。
4. agent 干活时可以继续发消息（会并入当前这轮）；执行中在底部显示一个轻量的"处理中"指示。
   一轮以失败结束时显示一条提示（如"模型调用失败，详情见服务日志"）。
5. 刷新页面、断网恢复、服务重启后，页面自动重连并补齐，不重复、不丢消息；当前 URL（`/s/<id>`）
   刷新后仍停在这个会话。
6. 回车发送，Shift+回车换行；中文输入法选词时的回车不会误发。
7. 往下看新内容时自动跟随到底部；往上翻历史时不打扰，出现"回到底部"按钮。

## 二、技术选择（AI 定，附理由）

原则：界面本身不复杂，依赖只留"自己写不值得"的部分，效果相关的依赖不为几十 KB 抠。
体积不设硬上限，`pnpm build` 后看一眼 gzip 总量（预期几百 KB 量级）与二进制增量，明显异常再审依赖。

| 项 | 选择 | 理由 |
|---|---|---|
| 构建 | Vite + TypeScript（strict） | 事实标准，热更新快，产物是纯静态文件 |
| UI 框架 | Svelte 5（runes） | 编译期框架，运行时约 10 KB 级（React 19 约 60 KB gzip）；细粒度响应式正适合逐字追加的流式渲染 |
| 样式 | Tailwind CSS v4 | 只打包用到的类；深浅色靠 CSS 变量 |
| 组件 | 不引组件库，手写 | 需要的只有折叠（原生 `<details>`）、抽屉、主题切换、输入框，shadcn/Radix 一类用不上 |
| 图标 | 内联 SVG（十来个，取自 Lucide 的路径） | 省一个依赖 |
| Markdown | markdown-it（`html: false`） | 原始 HTML 一律转义：模型输出不可信，页面里存着 token，杜绝 XSS，也就不需要 DOMPurify；未闭合的代码块按代码显示，流式中不闪 |
| 公式 | KaTeX + markdown-it-texmath | 支持 `$…$`、`$$…$$`、`\(…\)`、`\[…\]`（DeepSeek 常用后两种）；texmath 的块级 `\[…\]` 不能打断段落，另补一条行内显示公式规则，接住写在句中或紧跟正文行的 `\[…\]`（内容不含反引号，不跨进行内代码）；`throwOnError: false`、`trust: false`，写错或流式中未闭合的公式按原文显示，不开放 `\href` 等可执行扩展 |
| 代码高亮 | highlight.js 官方常用语言包（`highlight.js/lib/common`，约 40 种）；另注册 `dockerfile`、`nginx`、`powershell`（toml 由包内 `ini` 覆盖） | 覆盖日常所见；Shiki 更精细但需 WASM 与大体积语法包 |
| SSE 解析 | 手写（约 40 行） | 协议简单（gateway §5.1 只用 `event`/`id`/`data`），不值一个依赖 |
| 包管理 | pnpm | 本机已有；`pnpm-lock.yaml` 入库 |

运行时依赖：`markdown-it`、`markdown-it-texmath`、`katex`、`highlight.js`；其余全是构建期依赖。
不引入状态管理与请求缓存库，Svelte 的 `$state` 足够。

## 三、Rust 侧：`mic-gateway` 服务静态资源

- `rust-embed` 嵌入 `web/dist`（`allow_missing = true`，未构建时也能编译，保证没有 node 的环境
  `cargo clippy` 可过）。
- **Fail Fast**：Service 启动时产物里没有 `index.html` → 返回错误
  `Web 前端未构建：先运行 pnpm -C web install && pnpm -C web build`，不静默起一个空站点。
- 路由：`/api/*` 不变；其它 GET 按路径找嵌入文件，找到即返回（`Content-Type` 按扩展名）；
  `assets/` 下找不到 → 404（旧页面请求已不存在的哈希文件，不能把 HTML 当脚本返回）；其余找不到 → 返回 `index.html`（前端路由 `/s/<id>`）。`/assets/*`（带内容哈希）加
  `Cache-Control: public, max-age=31536000, immutable`，`index.html` 加 `no-cache`。
- 静态资源**不鉴权**：页面外壳不含数据，数据都在 `/api` 后面。
- 开发期：`pnpm -C web dev` 起 Vite（5173），把 `/api` 代理到 `127.0.0.1:7878`；
  浏览器开 `http://localhost:5173/#token=<token>`。debug 构建的 rust-embed 从磁盘读，
  `pnpm build` 后刷新即生效，不必重编 Rust。

gateway.md §四 "`/api` 之外返回 404（静态资源随 M10）" 随本文改为上面的规则。

## 四、前端结构

```
web/
  package.json  vite.config.ts  tsconfig.json  index.html
  src/
    main.ts  App.svelte
    api/types.ts       # 与 gateway §4.2、§5.1 及 mic-message serde 一一对应的 TS 类型
    api/client.ts      # fetch 封装：带 token、解析 {error}、401 → 清 token
    api/stream.ts      # fetch 读流 + SSE 解析 → 强类型事件；按 §五 重连
    state/session.svelte.ts  # 单个会话的状态与事件归约（§五）
    lib/markdown.ts    # markdown-it + KaTeX + highlight.js 配置
    components/…       # 侧栏、消息列表、各类消息、工具行、输入框、主题切换
```

### 4.1 类型边界

`api/types.ts` 手写，照 gateway §4.2 与 mic-message 的 serde 形状（外部标签枚举，如
`{"Text": {"text": "…"}}`、`MessageBody` 内部标签 `kind`）。入站数据在 `client.ts`/`stream.ts`
解析处按 `api/decode.ts` 的解码器逐字段校验一次（字段集合须恰好一致、枚举分支须已知、非法 JSON
同样算），失败即抛错并显示"界面与服务版本不一致，请刷新"，不重连；组件内不再检查。
mic-message 形状变化时，改它的 B2 须把本文列为调用方（gateway §4.2 已写明）。

### 4.2 消息呈现

| `MessageBody` | 呈现 |
|---|---|
| `UserInput` | 右侧气泡；`Text` 原文（不渲染 Markdown），`File` 显示文件名小标签 |
| `Reply` | 左侧无气泡，按 `blocks` 顺序：`Reasoning::Visible` → 折叠行"思考过程"；`Redacted` → "（推理内容已加密）"；`Text` → Markdown；`ToolCall` → 工具行 |
| `ToolResult` | 不单独成行，并入 `tool_call_id` 相同的工具行（✓ / ✗ 与输出）；找不到对应调用时单独显示一行 |
| `Completion` | 一行"后台任务完成：工具名"，可展开输出（v2 才会产生，先最小呈现） |
| `HarnessNote` | 不显示（给模型看的备注） |
| `Notification` | 居中的灰色系统提示 |
| `Boundary` | 分隔线："以上内容已压缩" / "上下文已清空" |

工具行的参数摘要：参数对象的第一个字符串值，截到一行；展开后显示完整 JSON 和输出文本，
输出区限高、内部滚动。失败输出 `Failed{kind, message}` 显示 message；`Cancelled` 显示"已取消"。

其它细节：
- 复制按钮两处：每个代码块右上角（复制代码原文，不含语言标记与围栏）；回复末尾（复制该条 `Reply`
  的正文 Markdown 原文）。复制后按钮短暂显示"已复制"。悬停消息显示时间。
- 会话顶栏显示工作目录（`workdir`）与渠道；取自 `GET /api/sessions/{id}`，与左栏无关。
  `writable = false` 时不显示输入框，改为"该会话来自 X 渠道，只能在这里查看"。
- 发送失败（413、500 等）在输入框上方显示服务端返回的中文错误，输入内容保留。
- 打开不存在的会话（`/s/<id>` 返回 404）→ 显示"会话不存在"并提供回到新会话的入口。
- 流式中草稿的 Markdown 重渲染合并到每帧至多一次，长回复不卡。

### 4.3 设置页与人设选择

- 路由 `/settings`，侧栏底部入口；`state/settings.svelte.ts` 登录后加载设置与未删除人设，编辑后刷新。
- `SettingsView`：对话偏好表单（未保存离开提示；系统提示词可展开只读查看）、人设列表与编辑器；内置人设只读，
  「复制并编辑」新建「<名字> 副本」；删除前确认；服务端 409 文案原样显示；「模型」区暂为说明文字。
- `PersonaPicker` 放在 `Composer` 发送按钮左侧；`executingRun` 非空时切换提示「下一轮生效」；
  会话选着的人设已不在列表时，选择器按 `settingsStore.resolve` 显示默认人设并提示「原人设已删除，下一轮改用默认人设」，不禁用发送。
- 相关接口与 `SessionItem.persona_id` 全部经 `decode.ts` 解码。

## 五、会话状态与流

每个打开的会话一份状态：

```ts
type SessionView = {
  info: SessionItem | null;     // 首次连流前取得；404 → "会话不存在"
  messages: Message[];          // 稳定消息，id 升序
  draft: { reasoning: string; text: string } | null;  // 正在生成的回复
  executingRun: number | null;
  ready: boolean;               // 回放完成
};
```

事件归约（与 gateway §5.1、§5.3 对应）：

| 事件 | 处理 |
|---|---|
| `message` | 追加（服务端保证 id 升序、不重复，gateway §5.3）；是 `Reply` 则 `draft = null` |
| `text_delta` / `reasoning_delta` | 追加到 `draft`（无则新建） |
| `draft_discarded` | `draft = null` |
| `ready` | `ready = true`，`executingRun` 取其值 |
| `run_started` / `run_finished` | 更新 `executingRun`（以最后收到的为准）；`run_finished` 后刷新左栏，非 `completed` 显示提示 |

重连：流结束或出错 → 按 1、2、4、8 秒（上限 10 秒）退避，带 `after = 最大消息 id` 重连；重连时先
`draft = null`（接入后只收到剩余增量，保留旧半截会拼出缺段的文字），其余状态保留。
401 → 清 token，显示第 1 条的提示页，不重连。切换会话时中止旧流。

发送：`POST` 期间输入框禁用；成功后清空输入框。消息本身等流里的 `message` 事件出现，
不做乐观插入（一处真相）。新会话：`POST /api/sessions` → 跳到 `/s/<session_id>` → 开流（不带 `after`）。

左栏：`GET /api/sessions?channel=web` 分页；「其它渠道」展开时按前端常量 `OTHER_CHANNELS = ["cli"]`
逐个拉取（新渠道由接入它的 B2 追加，如 v0b 微信）。列表在发送成功、`run_finished`、窗口重新获得焦点时刷新第一页。

## 六、副作用与依赖

- 前端只调 gateway §四 的接口，只在 `localStorage` 存 token 和主题偏好。
- `mic-gateway` 新增 `rust-embed`（含 `mime-guess` 特性）；`.gitignore` 加 `web/node_modules`、`web/dist`。
- 发布构建顺序：`pnpm -C web install && pnpm -C web build` → `cargo build --release`。

## 七、调用方

| 调用方 | 变更 | 兼容 |
|---|---|---|
| `mic-gateway` | 静态资源路由、启动检查产物（§三） | 新增行为；`/api` 不变 |
| `gateway.md` §四 | "`/api` 之外返回 404" 改为静态资源规则 | 文档修订 |
| `v0a-module-map.md` M10 | 链到本文 | 文档修订 |

## 八、验收（步 6，浏览器）

1. 未构建前端时启动 → 报错说明构建命令；构建后启动，打开终端打印的地址进入界面，地址栏 token 被抹掉；
   换一个错误 token 的地址 → 提示页。
2. 新会话发"列出当前目录，然后写一个 hello.txt 再读出来"：逐字显示、思考实时滚动后收起、
   工具行 ✓ 可展开、最终 Markdown 回复带代码高亮；代码块与回复末尾的复制按钮复制出的内容正确；左栏出现该会话。
3. 执行中再发一条：立刻出现在对话里，本轮结束时回复涵盖它。
4. 生成中刷新页面：停在同一会话，历史完整，当前回复从接入处继续，最终无重复。
5. 生成中停掉服务再启动：页面自动重连补齐；随机 token 已变时显示提示页。
6. 多会话来回切换，列表排序与分组正确；「其它渠道」折叠，展开可看 `-p` 会话，无输入框。
7. 深浅色跟随系统、手动切换生效；窄屏下侧栏为抽屉。
8. 中文输入法选词回车不发送；上翻历史时不被新内容拉回底部。
9. 回复里含 `<script>`/`<img onerror>` 等原始 HTML 时按文本显示；让模型写行内与块级公式（含 `\[…\]`）正确渲染，写错的公式按原文显示。
10. 记录 `pnpm build` 的 gzip 总量与 release 二进制增量。
