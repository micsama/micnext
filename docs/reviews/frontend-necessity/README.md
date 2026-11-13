# 前端实现必要性审查

日期：2026-11-13。目标：检查当前前端的复杂度是否服务于真实需求，优先使用浏览器、Svelte 和已有 Bits UI 的能力。

本次只审查，不修改业务实现、依赖、公开契约或 todo 状态。工作区有并行进行的图片修改；本文以读取时的实现为准，图片方案单独引用现有 B2，不替它批准。没有根据其他商业聊天产品的外观推断其内部实现。

## 结论

不需要整体重写，也没有足够理由迁移 SvelteKit、增加全站查询库或换聊天框架。主要问题集中在资源交付、请求生命周期和少量交互基础设施。业务状态本身大多有必要，问题是有些基础能力由应用重复实现，另一些必要的并发规则却没有完整建立。

先处理设置覆盖和附件生命周期；按图片 B2 删除历史图片的 Blob 管理；再决定附件下载、tooltip、选择器和页面懒加载的产品取舍。不能用“减少行数”代替正确性，也不能用引入更多库代替明确数据所有者。

## 已确认的缺陷

以下为源码可推导的触发路径，尚未做浏览器慢网复现；检查通过不能证明这些竞态不存在。

### F1 / P1：字段自动保存会覆盖刚保存的其他字段

位置：`web/src/components/settings/PrefsSection.svelte:41-53`；`crates/mic-gateway/src/settings.rs:115-152`。

每个字段看起来独立保存，实际 PUT `{ ...pick(settings), [field]: value }` 提交整份设置。修改 A 后，在其 PUT 和后续 refresh 完成前修改 B，两次请求都基于旧 settings；B 请求含旧 A，后提交会把新 A 覆盖。即使服务器串行处理请求也会发生。各字段随后仍可显示“已保存”。

建议保留当前 API，先在页面内串行提交整份待保存草稿，明确编辑中、待提交和已确认的值；不能仅在旧对象上分别排队保存字段。另一候选是字段级更新 API，但需要 B2，没有必要先扩大 HTTP 契约。验收：延迟 A 响应，连续改 A/B，最终重读服务端确认两项都保留；失败时不得显示成功。

### F2 / P2：设置刷新无提交顺序，且刷新失败仍可被显示为保存成功

位置：`web/src/state/settings.svelte.ts:19-39`；`web/src/App.svelte:66-72`；`PrefsSection.svelte:49-50`。

窗口 focus 与多个编辑页面均调用 refresh；没有去重、取消或轮次限制。较早读取旧设置的请求可以晚返回并覆盖新请求结果。一次 refresh 又并发读取六种资源，任一失败使整批新值都不提交。refresh 捕获错误后正常 resolve，调用方随后仍设置 saved；例如 PUT 成功但重读失败，页面会宣称已保存，却仍持有下一次全量 PUT 所用的旧值，加剧 F1。

建议先建立当前 store 的刷新提交规则，并让调用方区分“写入成功”和“刷新失败”。设置、模型目录、启动限制不必始终作为同一个成功单元；按当前消费者拆取，而不是机械地一资源一 store。无需先引入全站查询框架。

### F3 / P2：附件卸载后继续下载，完成后可遗留 Blob URL

位置：`web/src/components/FileCard.svelte:22-29,66-69`；`web/src/api/client.ts::fetchFile`。

图片文件附件挂载即 fetch，既无视口懒加载也无取消信号。请求过程中离开会话，onDestroy 执行时 url 尚为空；下载随后完成，创建的新 object URL 再无卸载回调负责撤销。离开后仍占带宽，与历史图片造成的问题同类。通知中心附件也使用此组件。

短期若保留 Blob 路径，必须明确请求归属、卸载取消以及完成与卸载交错时的释放。长期优先采用下文 R2 的直接 URL 方案，但 HTML sandbox、文件可变和错误提示契约必须保留。

### F4 / P2：账号设置路由没有被解析

位置：`web/src/state/route.svelte.ts::parse`；`web/src/components/settings/SettingsView.svelte::tabs`。

SettingsTab 和页面菜单都包含 account，解析正则却只列 prefs/personas/tags/models/env/wechat/backup。点击 `/settings/account` 会落到 new，账号设置不能按菜单进入。这是重复路由枚举已经产生的实际漂移，不是引入完整路由框架的理由。局部补齐并用现有路由表核对所有菜单地址即可。

### F5 / P2：可取消 sleep 不释放已完成等待的监听器

位置：`web/src/lib/sleep.ts:2-6`；消费者为会话流、全局流、日志流和消息定位。

正常超时后未 removeEventListener；once 只在 abort 事件触发后移除。长期重连会在同一个 signal 上积累已完成等待的闭包。传入已经 aborted 的 signal 也会继续等满定时器，因为不会补发 abort 事件。

保留这个小工具，补齐已中止时立即返回、任一完成路径清理 timer/listener。没有必要为几行等待代码加依赖。

## 可减少的基础设施与架构取舍

### R1：历史图片交给原生 img，收益明确

当前 HistoryImage 为 fetch→Blob→object URL 管理 Map、引用计数、闲置计时、取消和 IntersectionObserver。已有同源 cookie 鉴权，不需要 Blob 绕行；按 `docs/blueprints/image-thumbnail.md` 用缩略图 URL 和原生 lazy loading 是合适方向。

仍需非零尺寸占位、失败展示和按需打开原图。原生加载不保证组件卸载后立即终止网络，不能未经慢网验收承诺修复 54。协商缓存不清除已显示内容和磁盘副本。参照 [MDN 图片懒加载](https://developer.mozilla.org/en-US/docs/Web/API/HTMLImageElement/loading)、[HTTP 缓存](https://developer.mozilla.org/en-US/docs/Web/HTTP/Guides/Caching)。

### R2：普通附件下载优先交给链接，预览与下载分开

FileCard/fetchFile 目前把整个文件收集进内存，才能发起保存或打开 PDF。为了页面内百分比，应用承担读取 chunks、进度、Blob URL、重复点击和弹窗绕行。

候选 A（建议）：下载使用同源 `<a href download>`，进度交浏览器；图片用 URL，HTML 继续 sandbox iframe，PDF 是否直接导航由现有响应头与浏览器验证。候选 B：若页面内下载进度与“先下载一次再复用预览”是必要体验，则保留 fetch 并修复 F3。A 会改变交付与错误展示：401 不能继续经 ensureOk 自动切登录，HTTP 失败也不再显示在卡片；必须在 B2 明确，而不能当作无行为变化的清理。

文件接口现有 no-store 与原生链接并不冲突。若直接预览被当前 attachment 响应头阻止，需要设计预览与下载响应的区别；不能为简化前端直接放宽 HTML 安全边界。参照 [MDN download](https://developer.mozilla.org/en-US/docs/Web/HTML/Reference/Elements/a)、[Content-Disposition](https://developer.mozilla.org/en-US/docs/Web/HTTP/Reference/Headers/Content-Disposition)。

### R3：全局接管 title 的 tooltip 应退出或交给已有 Bits UI

位置：`web/src/lib/tooltip.ts`、`web/src/main.ts`。

全局监听 pointerover，删除元素 title，搬到 dataset，自己定位浮层，再恢复属性。没有 focus 显示和 aria-describedby 关联；Svelte 更新 title 与 DOM 手改属性之间还有隐含协作。自定义短延迟与样式是真实体验，但不值得自己维护一套全站浮层基础设施。

候选 A：普通说明恢复原生 title，代码最少。候选 B：需要统一外观、键盘可达说明的地方用已有 [Bits UI Tooltip](https://bits-ui.com/docs/components/tooltip)，逐个声明触发器与内容。B 增加模板代码，但删除全局 DOM 改写和定位逻辑。由 human 决定是否保留全站自定义外观，不默认批量更换所有 title。

### R4：选择器和菜单应复用语义组件，但普通 Popover 不需要一律替换

ModelPicker、PersonaPicker 以 Popover 加普通按钮实现单选；SessionTags 在 Popover 内给按钮标 menuitemcheckbox，却没有相应 menu 容器和菜单方向键模型。Popover 管理浮层与焦点边界，不等于完整的 Select/菜单语义。

已有 Select 和 Bits UI 依赖。优先让单选使用 Select，动作菜单使用 DropdownMenu，标签可用普通 checkbox 组（或完整 CheckboxItem 菜单）；保留现有展示内容。参照 [Bits UI Dropdown Menu](https://bits-ui.com/docs/components/dropdown-menu)。收益是复用键盘与选中态规范，不是减少所有模板行数。SearchPalette 的跨会话命中和消息定位有专属逻辑，暂不要求迁移 Command/Combobox；先实际验收键盘和读屏，避免无收益替换。

### R5：侧栏全量重拉是明确的一致性取舍，存在请求放大

位置：`web/src/state/sessions.svelte.ts::SessionList.refresh/loadMore/SessionsSync`。

loadMore 把 pages 加一后从第一页重拉；从一页翻到 N 页，总请求数为 1+2+…+N；后台 session_activity 又刷新已加载的全部页、多个渠道和标签词表。没有必要把标签词表视为每次消息活动都会改变的数据。

但不能简单追加一页：列表按最近活动重排，旧游标和新首页直接拼接会漏项；此前 `docs/reviews/sidebar-refresh/README.md` 已说明，当前轮次保护与统一提交路径有实际作用。

候选 A：保留重拉页数的一致性策略，减少无关资源刷新，观察真实请求量。候选 B：增加局部失效/更新策略，减少重拉，但须明确游标重排行为，可能涉及事件契约。先选 A；不以性能偏好推翻已有修复，不直接把分页换成普通 append。

### R6：页面按需加载比继续手调 chunk 分组更直接

App 静态导入设置、开发者、Trace、聊天全部页面；Markdown 通过聊天组件静态进入依赖图，独立 markdown chunk 并不等于按路由懒加载。首次打开登录页也需要取得入口静态依赖。

本次生产构建未压缩 JS：index 551.43KB、vendor 280.66KB、markdown 791.36KB；Brotli 文件磁盘占用约 128/72/152KiB（磁盘块统计，非精确传输字节）。完整文件图标 chunk 696.10KB，已动态导入，不能把它全算到首屏。体积不是实际耗时测量。

建议先给设置/开发者/Trace 这些低频页面加动态 import 边界，利用 [Vite 动态导入](https://vite.dev/guide/features.html#async-chunk-loading-optimization)，再比较加载瀑布。自动预压缩和 Rust 单份嵌入有部署消费者，应保留。完整图标集合、公式和代码高亮都有现有产品价值，删能力或裁图标不能当作纯技术优化；参见既有二进制体积审查。

### R7：不立即引入 TanStack Query，但普通查询值得借鉴其规则

当前 settings、sidebar、pushes、schedules、trace 各自处理刷新与并发，策略不一。成熟查询方案已有 query key、失效、取消和后台刷新；参照 [Svelte Query](https://tanstack.com/query/latest/docs/framework/svelte/guides/queries)、[取消](https://tanstack.com/query/latest/docs/framework/svelte/guides/query-cancellation)。

候选 A（当前建议）：修复 F1/F2，保留业务专属刷新，只有实际共同变化的少量生命周期逻辑才提取私有函数。候选 B：以 settings 等普通 GET 为小范围试点，比较引入查询库后净删除多少状态、是否保持错误/重试契约，再决定扩展。引入依赖与公开入口须 B2。不要同时保留查询库和另一套手写缓存，也不要把 SSE 业务归约迁到查询库以求形式统一。默认重试策略还须显式配置，不能顺手改变写入失败行为。

## 已有明确消费者，应保留的实现

| 区域 | 必要性与边界 |
|---|---|
| SessionView 的流事件归约 | 消息回放、草稿快照、认领、预测、终态是业务协议；原生 EventSource 只提供运输，不能代替这些状态。 |
| EventSource 及应用重连 | 已有 Safari 问题依据、回放游标、ready 和日志不同恢复策略；不机械恢复原生自动重连，也不改回 fetch 读流。 |
| MessageList 阅读冻结、贴底、历史定位 | 有“上翻不被输出推走”和 Safari 惯性滚动约束。通用虚拟列表不能自然代替；先保留，不为代码量换滚动库。双观察器与提示状态可随体验调整局部整理。 |
| 草稿中的 File/object URL | 本地未上传文件没有服务器 URL，按会话保留有明确需求；与历史图片 Blob 缓存不同。 |
| API/SSE 运行时解码 | 真正外部输入边界；TypeScript 类型不能验证 JSON。现有小型解码器有消费者，不必为换 Zod 重写全部协议。 |
| 简单 router | 约 50 行、支持 URL/历史记录；修 F4 并减少重复清单即可。无需为此迁移整个应用。导航元素可逐步恢复带 href 的链接，保留开新页等浏览器行为。 |
| Markdown/KaTeX/highlight.js | 已采用成熟库；HTML 关闭、公式 trust=false、链接保护应保留。流式长文重渲染成本需 profile 后再处理，不能凭感觉增量解析。 |
| 输入框 textarea | 粘贴、拖放、IME、停止、预测、slash、撤销都有具体体验；不引入富文本编辑器。高度计算未来可比较 CSS field-sizing，但须按目标 Safari 支持验证；execCommand 的撤销行为不随意替换。 |
| Dialog/AlertDialog/Select 薄封装 | 复用 Bits UI 的焦点与浮层，属于已使用成熟能力；不要为删文件破坏标准组件结构。 |
| 全局确认框、Web Push、Service Worker | 各有明确消费者；SW 当前只负责通知，不应顺手增加全站离线缓存。 |
| 日志帧合并与容量限制 | 高频流存在当前场景，requestAnimationFrame 与有界保留合理。 |
| 原生 range、简单 Switch、存储小工具 | 代码小且职责明确，不因已有组件库就全部替换；发现实际行为缺陷时再处理。 |
| 临时 LoadProbe | 当前仍在收集慢加载证据；按已有蓝图退出，不扩成长期通用前端监控框架。 |

## 建议执行次序与验收

1. 修复 F1/F2（设置写入与刷新），F4（账号路由）、F5（等待清理）；F3 若附件方案未定先局部止损。均先判断是否触及导出签名，公开契约变化另走 B2。
2. 按已存在的图片 B2 完成原生图片方案，验收真实旧图、慢网切换、原图按需下载及鉴权先于 304。
3. 为附件 URL 交付写窄 B2；先明确进度、错误展示、HTML/PDF 预览取舍，再实现。
4. Tooltip/选择器与低频页面懒加载可分批进行，分别核对键盘行为、Safari、首屏网络；不与设置竞态混为一个重写项目。
5. 请求放大先观察实际瀑布；查询库仅为候选，不预先安排全站迁移。

本次执行 `web/` 下 `bun run check`（0 错误、0 警告）、`bun run build`（成功）。未修改 Rust，无需为审查运行 fmt/clippy。未运行真实浏览器、读屏、慢网、生产请求或设置写入，以上交互验收仍需实施时完成。审查文档不登记为待批准功能，也不关闭既有 B2。

## 处理结果（2026-11-13）

- F1：设置的保存改为串行执行，每次提交都带上所有还没从服务端读回的值；重新读取失败时提示"已保存，但重新读取失败"。
- F2：多次刷新同时进行时，只采用最后发起的那次结果；`refresh` 返回这次是否成功。
- F3：文件附件里的图片改用 `ChatImage`，由原生 `<img>` 懒加载，显示失败时退回文件卡片；其余类型的文件仍点开时带进度下载（R2 未定）；组件卸载后才完成的下载不再生成 Blob URL。
- F4：设置分类改为由同一个列表同时生成类型和路由解析，补上了 `account`。
- F5：`sleep` 任意一种方式结束都会清理定时器和监听器；传入的信号已中止时立即返回。
- R1：已按图片 B2 实现。
