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
| 引用回复 | `inbound.ts` + `partial-quote.ts` + `quote-store.ts` | `ref_msg` 只校验，内容丢弃 | **接**：被引用内容以 `[引用：…]` 前缀进文字 |
| 引用 bot 自己的消息 / 部分引用 | `quote-store.ts` 按 svr_id 缓存收发消息；`partial_text` 按 md5 定位片段 | 无 | 随引用一起定：我们已有出站 `external_message_id`，可据此反查原 Reply，不另建缓存 |
| 出站图片、文件、视频、语音 | `cdn/upload.ts`、`send-media.ts`：`getuploadurl` → 加密上传 → item 引用 | 只发文本 | **接**：需模型侧「发文件」能力（工具或回复块），走 B2 |
| 工具调用进度 | `reply-progress-sender.ts`：item 11/12 + `message_state` | 不发 | 不接：human 判断会刷屏 |
| Markdown 过滤 | `markdown-filter.ts`（373 行） | 不转换，靠渠道提示 | 不接：已选渠道提示路线 |
| 斜杠指令 `/echo`、`/toggle-debug` | `slash-commands.ts` | 无 | **做**，但不在微信 crate 内：统一指令管理，Web 与各 Channel 同步，另起 B1 |
| 多账号、pairing | `accounts.ts`、`pairing.ts` | 单对象 | 不接（单进程个人工具） |

## 二、建议顺序

1. **引用回复**：只动微信 crate 内部，不改公开契约，量小。
2. **入站图片**：复用现有图片输入；新增 AES 依赖与转换规则。
3. **入站文件/视频**：先定临时目录位置、清理与尺寸上限，与工具隔离（数据目录边界）一起看。
4. **出站媒体**：需要模型侧发文件能力，跨 core/tool 契约，最后做。
5. **斜杠指令**：跨 Channel 统一，单独 B1。

## 三、未知项

- 上下线通知：停服后「未连接」是否立即出现，决定是否接 notifystop。
- 引用反查：`ref_msg.svr_id` 是否等于我们发送时拿到的 `message_id`，需实测。
- 入站媒体单个上限（SDK 默认 25 MB）、保留天数（SDK 7 天）是否照搬。
- 表情包的实际 item 形态（type 2 还是其他）。
