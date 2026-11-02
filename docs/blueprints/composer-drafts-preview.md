# B2：会话草稿保留与发送前图片预览

状态：已批准，实现完成，待验收。

## 行为

- 按会话分别保留未发送的文字、附件和回复引用，新会话用独立的 `new` 草稿；切走再回来可恢复。只放页面内存，刷新即消失。
- 发送中的状态随草稿走，切走再回来仍不能重复发送；成功清空，失败保留。
- 待发送图片点击可放大预览，与历史文件预览一致。

## 前端契约

- 新增 `web/src/state/composer-drafts.svelte.ts`：`ComposerDraft`（text、attachments、reply、sending）与 `composerDrafts.get(key: number | "new")`。草稿拥有附件的对象 URL，移除或发送成功时释放。
- `Composer` 新增可选 `draftKey`，ChatView 可写实例传 view.id，NewChat 传 `"new"`；去掉 `reply` 绑定。
- 删除 `SessionView.reply`，推送追问与 JobsDock 回复直接写目标草稿的 reply。
- 从 FileCard 提取 `FilePreview.svelte`，FileCard 与 Composer 共用。

不改 Rust、HTTP、数据库或依赖。

## 验收

- A、B、新会话各自编辑后往返恢复，互不串用；刷新后消失。
- 发送中切走再切回不会重复发送；成功清空、失败保留。
- 待发送图片可放大预览；历史文件预览不变。
- `bun run check`、`bun run build`；页面观感由 human 验收。
