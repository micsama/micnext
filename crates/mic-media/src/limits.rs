/// 规整后的长边上限。
pub(crate) const NORMALIZE_EDGE: u32 = 2048;
/// 合规格式且长边未超限时，原样保留的字节上限；超出则重新编码。
pub(crate) const KEEP_BYTES: usize = 1024 * 1024;
pub(crate) const JPEG_QUALITY: u8 = 85;
/// 单次解码的内存上限，防解压炸弹。
pub(crate) const DECODE_MAX_ALLOC: u64 = 256 * 1024 * 1024;
