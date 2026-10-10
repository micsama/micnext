# B1：微信 Channel V2——对齐官方 SDK 能力

**状态：方向已定（2026-10-10）：按 §二 顺序推进；工具进度与无转写语音不接，斜杠指令另起跨 Channel B1**

human 方向：官方 SDK / API 支持的能力尽量都接上。依据为本地 Bun 缓存的
`@tencent-weixin/openclaw-weixin@2.4.9/src`（约 7k 行），协议事实见 [`wechat-protocol.md`](wechat-protocol.md)。

## 一、SDK 能力盘点

| 能力 | SDK 位置 | 我们现状 | V2 判断 |
|---|---|---|---|
| 扫码登录、长轮询入站、文本发送、分段 | `auth/`、`monitor/`、`send.ts` | 已接（V1） | — |
| typing | `getconfig` / `sendtyping`，5 秒续发 | 已接（Phase 6） | — |
| -14 会话失效 | `session-guard.ts` | 已接 | — |
| 上下线通知 | `ilink/bot/msg/notifystart` / `notifystop` | 未接 | 暂不接：「未连接」由微信按轮询中断自行显示，SDK 未说明通知的额外效果；实测停服后提示明显滞后再补 |
| 入站图片 | `cdn/pic-decrypt.ts`、`media-download.ts`：CDN 下载 + AES-128-ECB | 占位 `[图片]` | **接**：作图片输入；非 PNG/JPEG/WebP（表情包、GIF）内部转换 |
| 入站文件、视频 | `media-download.ts` | 占位 | **接**：存日期目录，占位带路径，模型用文件工具读 |
| 入站无转写语音 | `media/silk-transcode.ts`（silk → wav） | 占位 `[语音]` | 不接：正常语音均有服务端转写，缺转写属少见情况，维持占位 |
| 引用回复 | `inbound.ts` + `partial-quote.ts` + `quote-store.ts` | 引用前缀与微信 ID 关联已实现，待服务器验收 | **接**：被引用内容以 `[引用：…]` 前缀进文字；契约见 [引用 B2](../blueprints/wechat-quotes.md) |
| 引用 bot 自己的消息 | `quote-store.ts` 按 svr_id 缓存收发消息 | 已实现，待服务器验收 | 复用出站 `external_message_id` 与分段计划，反查对应发送段，不另建缓存 |
| 部分引用 | `partial-quote.ts` 按 md5 定位片段 | 已实现普通文本定位与明确全文降级 | Markdown/表格选区精确还原为已知 bug，human 明确延期；协议字段继续完整接收 |
| 出站图片、文件、视频、语音 | `cdn/upload.ts`、`send-media.ts`：`getuploadurl` → 加密上传 → item 引用 | 只发文本 | **接**：需模型侧「发文件」能力（工具或回复块），走 B2 |
| 工具调用进度 | `reply-progress-sender.ts`：item 11/12 + `message_state` | 不发 | 不接：human 判断会刷屏 |
| Markdown 过滤 | `markdown-filter.ts`（373 行） | 不转换，靠渠道提示 | 不接：已选渠道提示路线 |
| 斜杠指令 `/echo`、`/toggle-debug` | `slash-commands.ts` | 无 | **做**，但不在微信 crate 内：统一指令管理，Web 与各 Channel 同步，另起 B1 |
| 多账号、pairing | `accounts.ts`、`pairing.ts` | 单对象 | 不接（单进程个人工具） |

## 二、建议顺序

1. **引用回复**：微信内部 quote 模块与入站 ID 关联已实现；[B2](../blueprints/wechat-quotes.md) 已批准，本地检查/API 验收通过，待服务器验收。
2. **入站图片**：复用现有图片输入；新增 AES 依赖与转换规则。
3. **入站文件/视频**：先定临时目录位置、清理与尺寸上限，与工具隔离（数据目录边界）一起看。
4. **出站媒体**：需要模型侧发文件能力，跨 core/tool 契约，最后做。
5. **斜杠指令**：跨 Channel 统一，单独 B1。

## 三、引用实测与最终边界（2026-10-10）

引用第 1 步已按 human 批准的 [B2](../blueprints/wechat-quotes.md) 实现，本地检查与 API 验收通过，待服务器人工验收。

- 用户文字、图片引用均为 type 0 的 ID 引用，无正文；title、svr_id 均为 null。四组样本确认引用 ID 对应原入站顶层 message_id：文字 `7514585484160251144`、图片 `7514585726439989128`；原 item 的 `v1:…` ID 不用于关联。
- bot 引用 ID 对应既有 `wechat_delivery_attempt.external_message_id`。按 chunk_index 取持久化计划中的实际发送段，不以整条内部 Reply 代替用户引用的段。已核对 message 203、219、221 的样本。
- 普通文本部分引用按锚点出现序号（从 0 起）定位，截取包含首尾锚点的片段；UTF-8 MD5 校验决定候选是否有效。本地 SDK 对 endindex 同时尝试全文与相对计数，均须校验，不能只凭索引猜。
- 简单样本 `嗯，就这情况。` 的 MD5 为 `e21c348c1bb0e81a2144ece8408952a9`，已与 message 221 的发送段及日志匹配。重复锚点样本 `看甲用。看乙用。看丙用。` 选中 `看乙用`，index 1/1，MD5 `11409f8325b269b33af44bb68fa1243b` 匹配。
- Markdown 样本 message 243 的跨强调选区 `加粗、普通、斜体，下`，MD5 `d5cc4eebc66760412055c0338bb956cb` 匹配显示文字，原 Markdown 不匹配；单字 `加` 的 MD5 `85be08f9260a8c930f333201afe6c54d` 匹配。跨表格选区的 MD5 `b97ddddd48abf91a8fa18157a2354bb0` 未能还原，不能认定表格被替换为 `[表格]`。
- human 决定：部分选区无法精确还原时提供完整被引消息并注明；Markdown/表格的精确定位作为低优先级已知 bug 延期，不阻塞本轮。bot 的完整被引消息指实际发送段。本步不重做微信渲染器。
- 成功入站快照在 finish_batch 中立即删除；不能用空查询判断 ID 不匹配，也不能把快照作为永久引用库。采样日志在实现后删除，ID 关联改由微信专用表保存。

## 四、引用所有权决定

目标是微信引用，当前消费者只有微信。关联与解析归微信模块，不新增核心通用来源登记接口。

正规正文与内部 MessageId 已由各渠道共用的 core_messages 保存。微信只增加外部 ID 到内部消息及原内容位置的关联，不另存正文；内部 ID 仍是全局排序序号，不能被微信外部 ID 替换。分段发送是一对多关系，出站复用已有发送计划和尝试记录。

入站正文落盘与微信关联登记分属两个事务；human 已批准的 B2 接受两者之间的崩溃窗口，后续引用明确显示原消息未找到，不补导入、不引入恢复或去重机制。引用前缀位置由组装侧记录，后续只读本条原内容，避免展开引用污染部分定位的出现序号。

## 五、后续未知项

- 上下线通知：停服后「未连接」是否明显滞后，决定是否接 notifystop。
- 入站媒体单个上限（SDK 默认 25 MB）、保留天数（SDK 7 天）是否照搬。
- 表情包的实际 item 形态（type 2 还是其他）。
