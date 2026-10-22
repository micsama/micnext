//! 入站图片与文件限额。

/// 一条用户输入最多带几张图。
pub const MAX_IMAGES_PER_INPUT: usize = 4;
/// 单张图片的最大字节数。
pub const MAX_IMAGE_BYTES: usize = 4 * 1024 * 1024;
/// Web 一条输入最多带几个文件；微信按 CDN 下载上限另行约束。
pub const MAX_FILES_PER_INPUT: usize = 4;
/// Web 上传单个文件的最大字节数。
pub const MAX_FILE_BYTES: usize = 10 * 1024 * 1024;
