use axum::http::{header, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use rust_embed::RustEmbed;

use crate::limits::{ASSET_CACHE_CONTROL, INDEX_CACHE_CONTROL};

/// M10 构建产物（web-ui §三）。debug 构建运行时从磁盘读，release 构建嵌入二进制。
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

const INDEX: &str = "index.html";
const ASSET_DIR: &str = "assets/";

#[cfg(debug_assertions)]
const MISSING_ASSETS: &str = "Web 前端未构建：请先安装 Node.js 和 pnpm，在源码目录运行 pnpm -C web install && pnpm -C web build，然后重新启动；完整 release 构建可运行 ./build.sh";
#[cfg(not(debug_assertions))]
const MISSING_ASSETS: &str = "此二进制未包含 Web 页面：请在源码目录运行 ./build.sh，完成前端构建并重新编译 Rust；构建环境需先安装 Rust、Node.js 和 pnpm";

/// 启动检查：产物缺失即报错，不起空站点。
pub(crate) fn ensure_built() -> Result<(), String> {
    match Assets::get(INDEX) {
        Some(_) => Ok(()),
        None => Err(MISSING_ASSETS.into()),
    }
}

/// `/api` 之外的 GET：命中产物文件即返回；`assets/` 下未命中为 404（旧页面请求已不存在的哈希文件），
/// 其余回落到 `index.html`（前端路由）。
pub(crate) async fn asset(uri: Uri) -> Response {
    let path = uri.path().trim_start_matches('/');
    if let Some(file) = Assets::get(path).filter(|_| !path.is_empty()) {
        return serve(path, file);
    }
    if path.starts_with(ASSET_DIR) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match Assets::get(INDEX) {
        Some(file) => serve(INDEX, file),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

fn serve(path: &str, file: rust_embed::EmbeddedFile) -> Response {
    let cache = if path.starts_with(ASSET_DIR) {
        ASSET_CACHE_CONTROL
    } else {
        INDEX_CACHE_CONTROL
    };
    let mime = file.metadata.mimetype();
    (
        [
            (
                header::CONTENT_TYPE,
                HeaderValue::from_str(mime).expect("mime 类型是合法的头部值"),
            ),
            (header::CACHE_CONTROL, HeaderValue::from_static(cache)),
        ],
        file.data,
    )
        .into_response()
}
