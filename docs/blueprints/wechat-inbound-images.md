# B2：微信入站图片 + 图片能力共享层

**状态：CLOSED（2026-10-10 服务器验收；GIF 仅本地冒烟）。r2 已批准（2026-10-10，human；附加约束：core 的 InputError 保持精简，图片只占一个变体）。** 来源：[V2 B1](../brainstorm/wechat-v2.md) §二 第 2 步。r2 吸收外部 review 5 条（全部 accept，均为本 Blueprint 缺陷）与 human 的分层要求：图片能力已有 Web、微信两个消费者，按变化轴拆出共享层。

## 一、用户可见行为（human 已确认）

- 微信图片交给模型看，网页会话里同样可见。只发图也马上回复；渠道提示加 `If the user's intent is unclear, ask.`（以后收文件同样适用）。
- 微信对不合规的图（大图、GIF、表情包等）统一规整后交给模型。
- 微信图片失败（地址/密钥解析、下载、解密、规整、超出单条数量）→ `[图片：…]` 占位，整条只记录不回复，处置说明推回微信并提示重发。仅下载会重试：失败立即重来，共 2 次，每次 15 s。
- Web 策略不变（超限拒收），但「合规」改由共享层判定，比现在更严：坏文件、长边超限的图在发送时即被拒，而不是到模型调用时报错。

## 二、拆分与归属

| 变化轴 | 归属 | 不负责 |
|---|---|---|
| 图片能力：格式识别、解码校验、缩放、编码、解码资源上限 | **新 crate `mic-media`** | 渠道协议、失败后是否回复、落盘 |
| 什么图能进入消息（合规判定的唯一调用点） | `mic-core::input::validate` | 规整 |
| 微信协议：CDN 地址、密钥、下载解密、失败占位与通知、单条数量策略 | `mic-channel-wechat` | 编解码细节 |
| Web 策略：超限拒收 | 不变（经 core 的 `InputError`） | — |

取舍：

- **为什么是新 crate**：编解码随格式、库版本、资源上限变化，与 agent 编排（core）和消息类型（message，L0 零依赖）都无关。放进 core 会让 Channel 依赖 core 的非编排能力；放进 message 会让 L0 带上 codec 依赖。
- **刻意不做**：
  - 不共享下载/CDN（只有微信用）。
  - 不建图片与文件的统一流水线。
  - 共享层不提供 async 或重试，接口是同步纯函数，调用方自行 `spawn_blocking`。
  - 不抽出「媒体处理策略」trait。

## 三、`mic-media` 契约

依赖：`mic-message`（`ImageFormat`、`ImageData`、`MAX_IMAGE_BYTES`）与外部 `image`（`default-features = false`，features `png` `jpeg` `gif` `webp`）、`thiserror`。

```rust
/// 不合规原因；由调用方决定拒收还是规整。
#[derive(Debug, Clone, Copy, PartialEq, Eq, thiserror::Error)]
pub enum MediaError {
    #[error("不是可识别的 PNG、JPEG、WebP 图片")]
    Unsupported,        // 魔数不在三种之内（GIF 等可解码但不合规的也在此）
    #[error("图片已损坏")]
    Corrupt,            // 魔数正确但解码失败，或超过解码内存上限
    #[error("图片不能超过 {} MB", MAX_IMAGE_BYTES / 1024 / 1024)]
    TooManyBytes,
    #[error("图片长边不能超过 {MAX_EDGE} 像素")]
    TooLargeDimensions,
}

/// 合规 = 魔数为 PNG/JPEG/WebP ∧ 字节 ≤ MAX_IMAGE_BYTES ∧ 长边 ≤ MAX_EDGE ∧ 完整解码成功。
/// 先读头部尺寸再解码；合规才原样保留字节，不强制重编码。
pub fn inspect(bytes: Vec<u8>) -> Result<ImageData, MediaError>;

/// 合规原样返回；否则解码首帧，长边超 NORMALIZE_EDGE 则等比缩小，重新编码
/// （有透明通道 PNG，否则 JPEG）。不可解码 → Corrupt；结果仍不合规 → 对应错误。
pub fn normalize(bytes: Vec<u8>) -> Result<ImageData, MediaError>;
```

`mic-media/src/limits.rs`：`MAX_EDGE = 8000`（主流模型 API 的硬上限）、`NORMALIZE_EDGE = 2048`、`JPEG_QUALITY = 85`、`DECODE_MAX_ALLOC = 256 MiB`。

`ImageFormat::sniff` 从 `mic-message` 移入 `mic-media`（唯一调用方是 core 的 validate），格式识别只留一处。

## 四、调用方逐项

| 调用方 | 变化 | 兼容 |
|---|---|---|
| `mic-core::input::validate`（`append_user_input`、`append_recorded_input`、`assembly.rs` 的 `-p` 路径） | 图片改调 `mic_media::inspect`；validate 因解码而在 `spawn_blocking` 中执行，签名改为 async | 内部函数；三处调用加 `.await` |
| `mic_core::InputError`（公开） | `InvalidImage`、`ImageTooLarge` 合并为一个 `Image(MediaError)`，文案透传；最终为 `Empty` / `ImageLimit` / `Image(MediaError)` 三项 | 唯一消费者 `mic-gateway/error.rs`：`TooManyBytes` → 413，其余 → 400，文案取 `MediaError` |
| `mic-gateway` Web 发图 | 无代码变化（`IncomingPart::Image(bytes)` 照旧） | 前端 Composer 的类型/字节预检保留，尺寸与损坏由服务端报错显示 |
| `mic-message` | 删除 `ImageFormat::sniff` | 仅 core 使用 |
| `mic-channel-wechat` | 新增依赖 `mic-media`；见 §五 | — |

依赖方向：`mic-media` → `mic-message`；`mic-core`、`mic-channel-wechat` → `mic-media`。无环。批准后 `CLAUDE.md` 依赖不变量补一条：「`mic-media` 只依赖 `mic-message`，与 store/tool 互不依赖」。

## 五、微信侧

**解析**（`client.rs`，边界一次 parse）：

```rust
pub(crate) enum IncomingContent {
    Text(String),
    VoiceText(String),
    Image(Result<ImageSource, ImageFailure>),  // 解析失败也在这里携带
    Unsupported(Unsupported),                  // 去掉 Unsupported::Image
}

pub(crate) struct ImageSource {
    pub url: String,           // media.full_url，缺则 <CDN_BASE>/download?encrypted_query_param=…
    pub key: Option<[u8; 16]>, // None = CDN 明文
}

#[derive(Clone, Copy)]
pub(crate) enum ImageFailure {
    Unparsable,   // 缺地址或密钥形状不符
    Download,     // 重试后仍失败或超过下载上限
    Decrypt,
    Media(MediaError),
    OverLimit,    // 超出单条 MAX_IMAGES_PER_INPUT
}
```

密钥规则按 SDK：优先 `image_item.aeskey`（hex），否则 `media.aes_key`（base64 后为 16 字节原文或 32 位 hex 文本）。

**处理**（新 `media.rs`）：`fetch(http, &ImageSource) -> Result<ImageData, ImageFailure>`：
1. 下载：`IMAGE_DOWNLOAD_ATTEMPTS = 2` 次，立即重试，每次 `IMAGE_DOWNLOAD_TIMEOUT = 15s`，响应体超 `IMAGE_DOWNLOAD_BYTES = 32 MiB` 中止。
2. 解密：AES-128-ECB + PKCS7（`aes` + `ecb`），不重试。
3. 规整：`spawn_blocking(mic_media::normalize)`，不重试。

**组装**（`service.rs`）：
- `flatten` 改为 async，按原顺序产出 parts。
- 成功的图为 `IncomingPart::Image`，失败的为占位文字 `[图片：下载失败]` 等。
- 拼上引用前缀后统计图片数，超过 `MAX_IMAGES_PER_INPUT` 的从第 N+1 张起改为 `[图片：超出单条上限]`，记为 `OverLimit` 失败。
- `Flattened { parts, unsupported: Vec<&str>, failures: Vec<ImageFailure> }`：任一非空即走 `append_recorded_input`。
- 处置说明由两部分拼成，各自有固定文案：「暂不支持文件」类与「图片下载失败，请重发」类。
- 不新增 runner、事件或终态。

**取消**：
- 每张图的 `fetch` 与 `stop.cancelled()` 做 `select`。
- 取消时这条消息不调用任何 append、不落盘，循环返回 `Stopped`。
- 批次未 `finish_batch`，下次启动按既有规则标 `interrupted`，不重放，与现有行为一致。

**不变量**：交给内核的图片均来自 `normalize` 的合规输出，数量已截到上限内，因此微信入站不会因图片得到 `InputError`。

`limits.rs` 新增上述三项与 `CDN_BASE = "https://novac2c.cdn.weixin.qq.com/c2c"`。依赖：`mic-media`、`aes` 0.8、`ecb` 0.1，tokio 加 `rt`。

表情包 item 形态未采样：若不是 type 2，维持 `[不支持的消息]`，实现时采样报告，不在本 B2 扩展。

## 六、验收

- **微信**：
  - 普通照片、勾原图的大照片、截图、GIF 各发一张，网页可见、bot 能描述。
  - 断网或改坏 CDN 后发图，微信收到「下载失败，请重发」，网页显示占位。
  - 只发图时 bot 询问意图。
- **Web**：
  - 正常图照常发送。
  - 损坏文件（改扩展名的随机字节）被拒，显示「图片已损坏」或「不是可识别的…」。
  - 长边超 8000 的图被拒。
