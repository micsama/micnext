# B2：微信入站文件与视频

**状态：已实现，待服务器验收（2026-10-10）。** 来源：[V2 B1](../brainstorm/wechat-v2.md) §二 第 3 步。仅微信模块内部，不改 core/store/message 公开契约，不新增 crate 或跨 crate 依赖。

## 一、用户可见行为（human 已确认，2026-10-10）

- 微信发来的文件、视频存到**该微信会话的工作目录**：`<pwd>/微信文件/<YYYY-MM-DD>/<文件名>`（本机日期）。用户和模型都能直接找到；不放 micnext 数据目录。
- 单个上限 **100 MB**（同 SDK）。不自动清理，文件归用户管。
- 保存成功即**正常回复**：消息里是 `[文件：报告.pdf，已保存到 /abs/path]`、`[视频：已保存到 /abs/path]`，模型按需用文件工具读；视频模型看不了内容，只拿到路径。只发文件不带话时，按既有渠道提示 bot 会问意图。
- 失败（地址/密钥解析、下载、超过 100 MB、解密、写盘）→ `[文件：报告.pdf，下载失败]` 这类占位，整条只记录不回复，处置说明推回微信并提示重发；与图片失败同一路径。仅下载重试，规则同图片（立即重来，共 2 次）。

## 二、命名与写盘

- 文件名取 `file_item.file_name` 的最后一段：去掉路径分隔符与控制字符，空、`.`、`..` 视为缺失。缺失 → `文件`；视频固定 `视频.mp4`（SDK 无视频文件名）。
- 同名已存在不覆盖：依次尝试 `名 (2).ext`、`名 (3).ext`…，以 `create_new` 占住最终名保证不覆盖。
- 先写 `<最终名>.part` 再 rename 覆盖占位，取消或失败时删占位与 `.part`，不留半截文件。
- 目录不存在则创建。工作目录取当时的会话记录（`Kernel::session`），不另存一份。

## 三、微信侧契约（crate 私有）

```rust
// client.rs：边界一次 parse
pub(crate) enum IncomingContent {
    Text(String),
    VoiceText(String),
    Image(Result<CdnSource, ImageFailure>),
    Attachment(Attachment),
    Unsupported(Unsupported),          // 去掉 File、Video 变体
}

/// 原 ImageSource 改名，图片与附件共用。
pub(crate) struct CdnSource { pub url: Url, pub key: Option<[u8; 16]> }

pub(crate) struct Attachment {
    pub kind: AttachmentKind,
    pub source: Result<CdnSource, AttachmentFailure>,
}
pub(crate) enum AttachmentKind { File { name: Option<String> }, Video }

// media.rs
pub(crate) enum AttachmentFailure { Unparsable, Download, TooLarge, Decrypt, Save }

pub(crate) async fn save(client: &Client, attachment: &Attachment, dir: &Path)
    -> Result<PathBuf, AttachmentFailure>;
```

- 地址解析同图片（`full_url` 优先，否则 CDN_BASE + `encrypted_query_param`）；密钥按 SDK 只取 `media.aes_key`（16 字节原文或 32 位 hex），缺失为 `Unparsable`（SDK 对文件/视频不接受明文）。
- `Client::download` 增加上限与超时参数：图片沿用 32 MiB / 15 s；附件 `ATTACHMENT_DOWNLOAD_BYTES = 100 MiB`、`ATTACHMENT_DOWNLOAD_TIMEOUT = 120 s`，超限为 `TooLarge`，不重试（`download` 返回私有 `DownloadError { Client(ClientError), TooLarge }`，不扩 `ClientError`）。新常量放 `limits.rs`。
- 下载与解密共用图片的重试/解密实现，图片再接 `mic_media::normalize`，附件接写盘。
- `service::flatten` 新增 `dir: &Path` 参数（`<pwd>/微信文件/<日期>`），失败统一收为原因短语列表，处置说明的重发提示由「请重发图片」改为「请重发」。
- 取消沿用图片规则：与 `stop.cancelled()` select，这条消息不落盘；`.part` 由 drop 时清理。
- 新外部依赖：`chrono`（与 mic-message 同版本、同 features）取本机日期。

## 四、调用方

| 调用方 | 变化 |
|---|---|
| `client::parse_updates` | type 4/5 产出 `Attachment`；`parse_image` 拆出共用的地址与 `media.aes_key` 解析 |
| `service` 入站循环 | 每条消息读会话 pwd 构造目录，传给 flatten；其余路径不变 |
| `media.rs` | 新增附件保存与失败类型；图片 fetch 复用下载+解密 |
| core/store/gateway/web | 无变化：模型与 Web 只看到文本占位 |

不新增 runner、事件或终态；不建附件表——事实是工作目录里的文件与消息里的路径。

## 五、验收

- 微信发 PDF、带中文名的文档、无扩展名文件、一段视频：工作目录下出现对应文件，bot 能读文件内容并回复；视频回复说明只有路径。
- 连发两个同名文件：第二个为 `名 (2).ext`。
- 只发文件：bot 问意图。
- 超过 100 MB 的视频（若可发）或断网：微信收到「下载失败/过大，请重发」，Web 显示占位，不回复。
