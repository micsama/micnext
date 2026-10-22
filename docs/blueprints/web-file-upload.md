# B2：Web 上传文件，统一文件消息

状态：2026-10-22 human 批准，实现完成，待验收。

## 目标

Web 输入框可以附带任意文件。系统把文件按发送人存进数据目录，消息里记下文件；模型看到的是一行带绝对路径的标注，需要内容时自己用 `read`/shell 读。微信收到的文件改走同一条路径，两个渠道共用一种文件事实。

不做：模型直接读取文件内容（不内联、不转 Provider 文件输入）、文件下载/预览接口、删除已存文件。

## 现状

- `ContentPart::File(FileRef { path, mime, size_bytes })` 已存在，但没有生产者；Provider 遇到它直接拒绝。本地开发库中 File 消息为 0 条。
- 微信把文件存到 `<pwd>/微信文件/<日期>/`，以文字占位 `[文件：名字，已保存到 /abs/path]` 入列。保存函数里的 `.part` 临时名会截断同名已有文件（审查 I2）。
- Web 只收图片，JSON base64，请求体上限 24 MB。

## 契约

### 消息类型（mic-message）

```rust
/// 用户随消息附带、已存入 `<data_dir>/files/<person_id>/` 的文件。
pub struct FileRef {
    /// 绝对路径。
    pub path: String,
    pub size_bytes: u64,
}
```

删掉 `mime`（没有消费者）和"相对 `<data_dir>/files`"的旧约定。展示名就是路径的最后一段（同名去重后的 `报告 (2).pdf` 也照实显示）。

`limits.rs` 新增 `MAX_FILES_PER_INPUT = 4`、`MAX_FILE_BYTES = 10 MB`。图片限额不变，两者分开计数。

### 入站（mic-core）

`IncomingPart` 新增 `File { name: String, bytes: Vec<u8> }`。内核在 `append_user_input` 里负责保存：

- 位置：`<data_dir>/files/<person_id>/<本机日期>/<name>`，按发送人分开。每条输入原本就带有 person，所以不新增任何字段或机制，将来多用户时，各自的文件天然分属不同目录。文件不进会话工作目录，避免在用户的项目或 git 仓库里留下文件。微信也改用这个目录，不再单独放到 `<pwd>/微信文件`。用 person id 而不用名字：名字可以改，id 不会变。
- 访问：模型凭消息里的绝对路径用 `read` / shell 读取，fs 工具目前不限制目录，所以无需额外授权。将来真正做多用户时，按 person 限制访问范围的规则在那时的 B2 里定，现在不预先做。
- 名字：只取最后一段，去掉控制字符；为空或是 `.`、`..` 时用 `文件`。
- 写入：先用 `create_new` 独占最终名，冲突时改用 `名字 (n).扩展名`，然后直接写入这个独占路径。不再使用 `.part` 和 rename，顺带修掉 I2。写入失败、输入事务失败或被取消时，删掉本次创建的文件。
- 数量与大小限额在各渠道边界检查：Web 在 Gateway 检查，微信沿用 CDN 下载上限。core 不另做限额。

保存函数从微信 `media.rs` 搬到 core，只保留这一份实现。

### 给模型看的样子（mic-core request）

`mic-message` 的模型视图把 File 转成文本 `[Attached file saved at /abs/path/报告.pdf (12345 bytes)]`。Provider 永远不会收到 File，`unsupported_file` 和各协议里的 File 分支一并删掉。上下文用量按这段文本估算。

### 微信

- 文件、视频下载成功后，改用 `IncomingPart::File` 入列。失败的仍用现有的文字占位。
- 引用：`quote.rs` 把 Image 映射成 `[图片]`，File 映射成 `[文件：名字]`，顺带修掉 I1。
- CDN 下载上限仍是 100 MB，不受 Web 的 10 MB 限制。

### HTTP（mic-gateway）

`PartBody` 新增 `{ "kind": "file", "name": "...", "base64": "..." }`。`MAX_INPUT_BODY_BYTES` 从 24 MB 提到 80 MB，即 4×10 MB 文件加 4×4 MB 图片，再按 base64 的 4/3 放大后取整。`GET /api/models` 的 limits 增加 `max_files`、`max_file_bytes`。

消息 JSON 中 File 的形状改为 `{ path, size_bytes }`。

### Web

- 输入框：附件按钮、粘贴、拖拽都接受任意文件。图片继续走现有的图片预览；其他文件显示为文件卡片（图标、名字、大小，可以移除）。超限在前端先拦下，提示文案与图片的一致。
- 消息里沿用 `MessageItem` 现有的文件胶囊，加上大小；悬停时显示完整路径。
- Trace 内容视图显示 `[文件：名字]`。

## 调用方影响

| 位置 | 改动 |
|---|---|
| mic-message | `FileRef` 改形状，新增两个限额 |
| mic-message | 模型视图把 File 转成文本 |
| mic-core | `IncomingPart::File`、保存逻辑、`context_usage` |
| mic-store | `NewInputPart` 增加 File；trace 的文本转换 |
| mic-provider-openai | 删除 File 分支和 `unsupported_file` |
| mic-channel-wechat | 只负责下载，成功的附件以 File 交内核保存，引用映射；删除微信侧保存逻辑和 `WECHAT_FILES_DIR` |
| mic-gateway | `PartBody::File`、请求体上限、limits 字段 |
| CLI | 显示 File 时用名字和路径 |
| web | 类型解码、Composer、MessageItem、TraceContent |

已有数据：没有生产者，旧形状的 File 不存在，不做迁移。如果服务器库里有 File 记录，先停下来报告。

## 验收

- 本地：上传文本文件和二进制文件；同名文件连续上传两次得到 `x (2)`；超过大小或数量被拒绝；先上传 `a.txt.part`，再上传 `a.txt`，前者内容不变；模型请求里出现路径标注，并能用 `read` 读到。
- 微信（human）：收到文件后会话里显示文件胶囊；引用图片或文件后正常回复。
- 收尾：`cargo fmt`、`cargo clippy -- -D warnings`、`cargo test --workspace`、`bun run check`。
