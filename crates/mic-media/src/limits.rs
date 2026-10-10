/// 合规图片长边上限；主流模型 API 的硬上限。
pub const MAX_EDGE: u32 = 8000;
/// 规整时缩到的长边。
pub(crate) const NORMALIZE_EDGE: u32 = 2048;
pub(crate) const JPEG_QUALITY: u8 = 85;
/// 单次解码的内存上限，防解压炸弹。
pub(crate) const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;
