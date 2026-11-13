/// 规整后的长边上限。
pub(crate) const NORMALIZE_EDGE: u32 = 2048;
/// 合规格式且长边未超限时，原样保留的字节上限；超出则重新编码。
pub(crate) const KEEP_BYTES: usize = 1024 * 1024;
pub(crate) const JPEG_QUALITY: u8 = 85;
/// 单次解码的内存上限，防解压炸弹。
pub(crate) const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;
/// 缩略图边界：聊天里最高显示 256px，按 3 倍屏留足。
pub(crate) const THUMB_WIDTH: u32 = 1024;
pub(crate) const THUMB_HEIGHT: u32 = 768;
/// 已在缩略图边界内时，原样返回的字节上限；超出则重新编码。
pub(crate) const THUMB_KEEP_BYTES: usize = 200 * 1024;
pub(crate) const THUMB_JPEG_QUALITY: u8 = 80;
