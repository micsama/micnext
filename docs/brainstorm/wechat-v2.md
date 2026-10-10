# B1：微信 Channel V2——对齐官方 SDK 能力

**状态：方向已定（2026-10-10）：按 §二 顺序推进，第 1、2 步 CLOSED；工具进度与无转写语音不接，斜杠指令另起跨 Channel B1**

human 方向：官方 SDK / API 支持的能力尽量都接上。依据为官方包
`@tencent-weixin/openclaw-weixin@2.4.9/src`（约 7k 行），协议事实见 [`wechat-protocol.md`](wechat-protocol.md)。
基础契约见 [微信 B2](../blueprints/wechat-channel.md)。

## 一、SDK 能力盘点

| 能力 | SDK 位置 | 我们现状 | V2 判断 |
|---|---|---|---|
| 扫码登录、长轮询入站、文本发送、分段 | `auth/`、`monitor/`、`send.ts` | 已接（V1） | — |
| typing | `getconfig` / `sendtyping`，5 秒续发 | 已接（V1） | — |
| -14 会话失效 | `session-guard.ts` | 已接 | — |
| 上下线通知 | `ilink/bot/msg/notifystart` / `notifystop` | 未接 | 暂不接：「未连接」由微信按轮询中断自行显示，SDK 未说明通知的额外效果；实测停服后提示明显滞后再补 |
| 入站图片 | `cdn/pic-decrypt.ts`、`media-download.ts`：CDN 下载 + AES-128-ECB | 已接，[B2](../blueprints/wechat-inbound-images.md) | 非 PNG/JPEG/WebP 及超大图经 `mic-media` 规整；失败为占位并提示重发 |
| 表情 | SDK 无表情包 item 类型 | 默认表情以 `[发呆]` 这类文本到达，随文本支持 | human 实测只有默认表情会到达，无需另接 |
| 入站文件、视频 | `media-download.ts` | 占位 | **接**：存日期目录，占位带路径，模型用文件工具读 |
| 入站无转写语音 | `media/silk-transcode.ts`（silk → wav） | 占位 `[语音]` | 不接：正常语音均有服务端转写，缺转写属少见情况，维持占位 |
| 引用回复（含引用 bot、部分引用） | `inbound.ts` + `partial-quote.ts` + `quote-store.ts` | 已接，[B2](../blueprints/wechat-quotes.md) | 被引内容以 `[引用：…]` 前缀进文字；bot 引用复用出站分段计划；Markdown/表格部分选区精确还原为延期的已知 bug |
| 出站图片、文件、视频、语音 | `cdn/upload.ts`、`send-media.ts`：`getuploadurl` → 加密上传 → item 引用 | 只发文本 | **接**：需模型侧「发文件」能力（工具或回复块），走 B2 |
| 工具调用进度 | `reply-progress-sender.ts`：item 11/12 + `message_state` | 不发 | 不接：human 判断会刷屏 |
| Markdown 过滤 | `markdown-filter.ts`（373 行） | 不转换，靠渠道提示 | 不接：已选渠道提示路线 |
| 斜杠指令 `/echo`、`/toggle-debug` | `slash-commands.ts` | 无 | **做**，但不在微信 crate 内：统一指令管理，Web 与各 Channel 同步，另起 B1 |
| 多账号、pairing | `accounts.ts`、`pairing.ts` | 单对象 | 不接（单进程个人工具） |

入站单条消息的缺陷（未知 item、残缺字段、引用无法解析）按条降级为占位并打日志，不断开入站连接；规则见 [微信 B2](../blueprints/wechat-channel.md) §6.2。

## 二、顺序

1. **引用回复**：CLOSED（2026-10-10 服务器验收）。
2. **入站图片**：CLOSED（2026-10-10 服务器验收；GIF 仅本地冒烟）。
3. **入站文件/视频**：先定临时目录位置、清理与尺寸上限，与工具隔离（数据目录边界）一起看。
4. **出站媒体**：需要模型侧发文件能力，跨 core/tool 契约，最后做。
5. **斜杠指令**：跨 Channel 统一，单独 B1。

## 三、引用实测样本（2026-10-10）

- 用户文字、图片引用均为 type 0 的 ID 引用，无正文；title、svr_id 均为 null。引用 ID 对应原入站顶层 message_id：文字 `7514585484160251144`、图片 `7514585726439989128`；原 item 的 `v1:…` ID 不用于关联。
- bot 引用 ID 对应既有 `wechat_delivery_attempt.external_message_id`，按 chunk_index 取持久化计划中的实际发送段。已核对 message 203、219、221 的样本。
- 普通文本部分引用按锚点出现序号（从 0 起）定位，截取包含首尾锚点的片段；UTF-8 MD5 校验决定候选是否有效。SDK 对 endindex 同时尝试全文与相对计数，均须校验。
- 简单样本 `嗯，就这情况。` 的 MD5 为 `e21c348c1bb0e81a2144ece8408952a9`，与 message 221 的发送段匹配。重复锚点样本 `看甲用。看乙用。看丙用。` 选中 `看乙用`，index 1/1，MD5 `11409f8325b269b33af44bb68fa1243b` 匹配。
- Markdown 样本 message 243 的跨强调选区 `加粗、普通、斜体，下`，MD5 `d5cc4eebc66760412055c0338bb956cb` 匹配显示文字，原 Markdown 不匹配；单字 `加` 的 MD5 `85be08f9260a8c930f333201afe6c54d` 匹配。跨表格选区的 MD5 `b97ddddd48abf91a8fa18157a2354bb0` 未能还原，不能认定表格被替换为 `[表格]`。
- 成功入站快照在 finish_batch 中立即删除，不能作为引用库；ID 关联由微信专用表保存。

## 四、引用所有权

关联与解析归微信模块，当前消费者只有微信，不新增核心通用来源登记接口。

正文与内部 MessageId 由 core_messages 保存；微信只存外部 ID 到内部消息及原内容位置的关联，不另存正文。分段发送是一对多关系，出站复用已有发送计划和尝试记录。入站正文落盘与关联登记分属两个事务，两者之间的崩溃窗口接受为「原消息未找到」，不引入恢复或去重机制。

## 五、后续未知项

- 上下线通知：停服后「未连接」是否明显滞后，决定是否接 notifystop。
- 入站媒体单个上限（SDK 默认 25 MB）、保留天数（SDK 7 天）是否照搬。
