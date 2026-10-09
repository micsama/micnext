//! 入站图片限额。

/// 一条用户输入最多带几张图。
pub const MAX_IMAGES_PER_INPUT: usize = 4;
/// 单张图片的最大字节数。
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
