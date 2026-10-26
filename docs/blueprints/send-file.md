# B2：模型交付文件（send_file 与 push 附件）

状态：2026-10-26 human 批准；实现完成，本地检查通过，待 human 验收。

来源：human 2026-10-26。模型画好的泳道图 HTML 只能报一个服务器路径，用户拿不到。方向已定：
- 文件交付做成一个通用能力，不按 HTML、图片等类型分开做；
- 文件挂在回复流里，由 core 落盘、Channel 投递；
- Web 和微信出站一起做（[微信 V2](../brainstorm/wechat-v2.md) 第 4 步）；
- 「发什么」和「发到哪」正交：内容统一用 `FileRef`；发到当前会话用 `send_file`，发给人用 `push` 的 `files`。

不做：分享链接、站点部署、Office 文档预览、自动清理快照。以后清理按磁盘日期删文件，库里路径保留，下载时返回「文件已清理」；不建文件表。

## 行为

- **文件内容不进数据库**：库里（消息 payload、`core_pushes.files`）只存 `FileRef { path, size_bytes }`，字节留在磁盘上，与入站文件一致。
- **只交付显式点名的文件**。调用时把文件复制一份快照到 `<data_dir>/files/<person>/<日期>/`，复用入站的独占命名保存逻辑。消息里记的是快照的路径，之后原文件再怎么改，已经交付的内容都不变。
- **`send_file { files: [path] }`**：文件进入当前会话的回复流。
  - Web：显示为文件卡片。
  - 微信：会话绑定了微信时，作为媒体消息发出。
  - 开放范围：主会话和定时任务会话提供，sub-agent 不提供。
  - 定时任务里用户一般不会去看这个会话，工具说明会引导模型改用 push 发文件。
- **`push { text, files?, via? }`**：文件跟着推送一起进通知中心和 IM。text 仍然必填，要求自成一条完整的消息。
- **校验**：
  - 每个路径必须是存在的普通文件（跟随软链接）。
  - 单次最多 `MAX_SEND_FILES = 8` 个文件，单个文件最大 `MAX_SEND_FILE_BYTES = 100 MB`。
  - 任一文件不合格，整次调用不交付，返回 input 错误。
  - 快照失败按 dependency 错误返回，并删掉本次已经复制的文件。
- **模型看到的**：工具结果写成 `Sent to the user: <快照路径> (<bytes> bytes)`，每个文件一行。

## 契约

### mic-message

- 新增 `MessageBody::Attachment { files: Vec<FileRef> }`：`send_file` 交给用户的快照。
  - 不进模型上下文。如果放进去，它会插在工具调用和工具结果之间，破坏协议要求的相邻关系；模型已经从工具结果里知道发了什么。
  - 投递。
- `Notification` 新增 `files: Vec<FileRef>`。模型视图里每个文件追加一行 `[Attached file saved at …]`，与入站文件写法一致。
- `FileRef` 注释改为"内核存入 `<data_dir>/files/<person>/` 的文件"，覆盖入站和出站两个方向。

### mic-core

- core 自有工具 `send_file`。它需要 data_dir、store 和事件发布，形态与 push 相同。
  - 流程：快照 → 在当前会话追加 `Attachment` → 发布事件。
  - 计入 `CORE_TOOLS`，并加入 sub-agent 的排除列表。
- `input.rs` 的 `save` 改为既接受字节也接受源文件路径（大文件流式复制）。保存位置和命名规则只在这一处实现。
- push：`PushArgs.files: Option<Vec<String>>`，快照规则与 `send_file` 共用；`Pushes::send` 接收 `Vec<FileRef>`。框架推送 `notify` 和 `append_notification` 传空列表。
- `limits.rs`：`MAX_SEND_FILES`、`MAX_SEND_FILE_BYTES`。

### mic-store

- `pending_deliveries` 投递的消息类型加上 `Attachment`。
- `core_pushes` 新增 `files TEXT NOT NULL DEFAULT '[]'`（`FileRef` 的 JSON 数组），`NewPush.files` 和 `PushView.files` 跟着加。
- `create_push` 把 files 同时写进各条 `Notification`。
- schema core 27 → 28。转换脚本 `scripts/db/core-28.sql` 做两件事：
  - 给 `core_pushes` 加列；
  - 给已有的 Notification payload 补上 `files: []`。

### HTTP（mic-gateway）

- `GET /api/sessions/{id}/messages/{message_id}/files/{index}`：只返回该消息引用的第 index 个文件，适用于 UserInput、Attachment 和 Notification。
- `GET /api/pushes/{id}/files/{index}`：通知中心的文件。
- 响应头：
  - `Content-Type` 用 `mime_guess` 判断。
  - 附带 `nosniff`、`no-store`、`Content-Security-Policy: sandbox`。
  - `Content-Disposition: inline; filename*=…`。
- 消息 JSON 增加 `attachment` 类型；Notification 和推送列表增加 `files`。

### Web

- 文件卡片复用上传那一版的文件胶囊组件，回复流、Notification 和通知中心共用同一个组件。按扩展名决定交互：
  - **图片**：显示缩略图，点击看原图。
  - **HTML**：在 `<iframe sandbox="allow-scripts">` 里加载 blob。不给 `allow-same-origin`，页面拿不到主站的 token；支持全屏浮层。不提供"新标签打开"，因为 blob 是同源的。
  - **PDF**：在新标签打开 blob。
  - **其他**：只能下载。
- 所有类型都可以下载：经 Bearer 拉取 blob 后保存，与现在的图片走同一条路。
- 卡片只显示加载和下载错误。单条消息的投递失败 Web 现在不展示（Reply 也一样），推送的投递状态仍走通知中心原有的状态标签。

### 微信

- 投递内容是 Reply 正文、Notification 的 text 加 files、Attachment 的 files。投递顺序：先发文本分段，再依次发文件。
- 上传：`getuploadurl` → AES-128-ECB 加密 → 上传到 CDN → 发送消息项。图片用 `image_item`（media_type 1），其余用 `file_item`（3）。每个文件发一次，不重放。
- 失败处理：任一项发送失败，按现有规则给整条消息记 `delivery_failure`，原因写「只送达前 N 条：…」；文件已被清理时原因为「文件已清理」。
- 发送计划 `Plan` 增加 `files`。每个文件和文本段一样记录发送事实，这样用户引用 bot 发出的文件时，能映射成 `[文件：名字]`。
- schema 微信 5 → 6，转换脚本给已有的 plan 补上 `files: []`。

## 实现补充

- 待投递索引 `idx_messages_undelivered` 和未读计数纳入 `Attachment`；core-28 转换脚本重建该索引。
- 文件下载时，文件已被清理返回 404「文件已清理」，其他读取失败如实报错。
- `convert.py` 规划阶段改用读写模式打开：服务停止后库没有 `-shm`，只读模式打不开 WAL 库。

## 验收

- **本地**：
  - `send_file` 发 HTML 和二进制文件：快照与原文件分离；同名文件得到 `(2)` 后缀；路径不存在或是目录时整次被拒。
  - Web 消息接口只能下载该消息引用的文件。
  - 转换脚本在旧库副本上跑通。
  - 收尾检查：`cargo fmt`、`clippy`、`cargo test --workspace`、`bun run check`。
- **human**：
  - Web：预览泳道图 HTML，确认脚本能运行、页面拿不到登录态；图片和 PDF 预览；下载。
  - 微信：在会话里收到图片和文件；定时任务 push 带文件后，通知中心和微信都能收到；引用 bot 发出的文件后能正常回复。
