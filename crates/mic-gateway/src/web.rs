use std::borrow::Cow;
use std::io::Read;

use axum::http::{header, HeaderMap, HeaderValue, StatusCode, Uri};
use axum::response::{IntoResponse, Response};
use brotli_decompressor::Decompressor;
use rust_embed::RustEmbed;

use crate::limits::{ASSET_CACHE_CONTROL, INDEX_CACHE_CONTROL};

/// M10 构建产物（web-ui §三）。debug 构建运行时从磁盘读，release 构建嵌入二进制。
/// 文本产物只有构建时预压缩的 `<路径>.br`，其余文件保持原样。
#[derive(RustEmbed)]
#[folder = "../../web/dist"]
#[allow_missing = true]
struct Assets;

const INDEX: &str = "index.html";
const ASSET_DIR: &str = "assets/";
const BROTLI_SUFFIX: &str = ".br";

#[cfg(debug_assertions)]
const MISSING_ASSETS: &str = "Web 前端未构建：请先安装 Bun，在源码目录运行 ./build.sh";
#[cfg(not(debug_assertions))]
const MISSING_ASSETS: &str =
    "此二进制未包含 Web 页面：请在源码目录运行 ./build.sh 重新构建；构建环境需先安装 Rust 和 Bun";

/// 启动检查：产物缺失即报错，不起空站点。
pub(crate) fn ensure_built() -> Result<(), String> {
    match find(INDEX) {
        Some(_) => Ok(()),
        None => Err(MISSING_ASSETS.into()),
    }
}

/// `/api` 之外的 GET：命中产物文件即返回；`assets/` 下未命中为 404（旧页面请求已不存在的哈希文件），
/// 其余回落到 `index.html`（前端路由）。
pub(crate) async fn asset(uri: Uri, headers: HeaderMap) -> Response {
    let path = uri.path().trim_start_matches('/');
    let accepts_br = headers
        .get(header::ACCEPT_ENCODING)
        .and_then(|v| v.to_str().ok())
        .is_some_and(|v| {
            v.split(',')
                .any(|e| e.split(';').next().is_some_and(|e| e.trim() == "br"))
        });
    if let Some(found) = Some(path).filter(|p| !p.is_empty()).and_then(find) {
        return serve(path, found, accepts_br);
    }
    if path.starts_with(ASSET_DIR) {
        return StatusCode::NOT_FOUND.into_response();
    }
    match find(INDEX) {
        Some(found) => serve(INDEX, found, accepts_br),
        None => StatusCode::NOT_FOUND.into_response(),
    }
}

/// 产物及其是否为 brotli 压缩件。
fn find(path: &str) -> Option<(Cow<'static, [u8]>, bool)> {
    Assets::get(&format!("{path}{BROTLI_SUFFIX}"))
        .map(|f| (f.data, true))
        .or_else(|| Assets::get(path).map(|f| (f.data, false)))
}

fn serve(path: &str, (data, compressed): (Cow<'static, [u8]>, bool), accepts_br: bool) -> Response {
    let cache = if path.starts_with(ASSET_DIR) {
        ASSET_CACHE_CONTROL
    } else {
        INDEX_CACHE_CONTROL
    };
    let mime = mime_guess::from_path(path).first_or_octet_stream();
    let mut headers = HeaderMap::new();
    headers.insert(
        header::CONTENT_TYPE,
        HeaderValue::from_str(mime.as_ref()).expect("mime 类型是合法的头部值"),
    );
    headers.insert(header::CACHE_CONTROL, HeaderValue::from_static(cache));
    if !compressed {
        return (headers, data).into_response();
    }
    headers.insert(header::VARY, HeaderValue::from_static("accept-encoding"));
    if accepts_br {
        headers.insert(header::CONTENT_ENCODING, HeaderValue::from_static("br"));
        return (headers, data).into_response();
    }
    let mut plain = Vec::new();
    Decompressor::new(&*data, 4096)
        .read_to_end(&mut plain)
        .expect("构建时写入的 brotli 产物可解压");
    (headers, plain).into_response()
}
