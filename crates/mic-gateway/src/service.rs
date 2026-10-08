use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::http::StatusCode;
use axum::routing::{get, post, put};
use axum::{middleware, Router};
use mic_core::{BoxError, Kernel, Service};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::limits::{MAX_BODY_BYTES, SHUTDOWN_GRACE, TOKEN_BYTES};
use crate::{api, settings, stream, web};

pub(crate) struct Gateway {
    pub(crate) config: Config,
}

/// 各请求共享的只读状态。
pub(crate) struct App {
    pub(crate) kernel: Kernel,
    pub(crate) token: String,
    /// 进程停止时结束所有 SSE 流。
    pub(crate) stop: CancellationToken,
}

impl Service for Gateway {
    fn run(
        self: Box<Self>,
        kernel: Kernel,
        stop: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>> {
        Box::pin(serve(self.config, kernel, stop))
    }
}

async fn serve(config: Config, kernel: Kernel, stop: CancellationToken) -> Result<(), BoxError> {
    web::ensure_built()?;
    let token = match config.token {
        Some(t) => t,
        None => random_hex(TOKEN_BYTES),
    };
    let listener = tokio::net::TcpListener::bind(config.listen)
        .await
        .map_err(|e| {
            format!(
                "Web 无法监听 {}：{e}。端口被占用时在配置 [gateway] 里改 listen",
                config.listen
            )
        })?;

    let app = Arc::new(App {
        kernel,
        token,
        stop: stop.clone(),
    });
    let api = Router::new()
        .route(
            "/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/sessions/{id}", get(api::get_session))
        .route("/sessions/{id}/messages", post(api::send_message))
        .route("/sessions/{id}/stream", get(stream::stream))
        .route("/sessions/{id}/persona", put(settings::set_session_persona))
        .route(
            "/settings",
            get(settings::get_settings).put(settings::put_settings),
        )
        .route(
            "/personas",
            get(settings::list_personas).post(settings::create_persona),
        )
        .route(
            "/personas/{id}",
            put(settings::update_persona).delete(settings::delete_persona),
        )
        .fallback(|| async { StatusCode::NOT_FOUND })
        .layer(middleware::from_fn_with_state(app.clone(), api::authorize))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));
    let router = Router::new()
        .nest("/api", api)
        .fallback(get(web::asset))
        .with_state(app.clone());

    // NOTE: 面向用户的提示，含 token，故不走 tracing。
    eprintln!("Web 已启动：http://{}/#token={}", config.listen, app.token);
    tracing::info!(listen = %config.listen, "gateway listening");
    let server = axum::serve(listener, router)
        .with_graceful_shutdown(stop.clone().cancelled_owned())
        .into_future();
    // 客户端不读时连接写不完，优雅关闭会一直等。
    let grace = async {
        stop.cancelled().await;
        tokio::time::sleep(SHUTDOWN_GRACE).await;
    };
    tokio::select! {
        served = server => served?,
        () = grace => tracing::warn!("gateway shutdown grace elapsed, dropping open connections"),
    }
    Ok(())
}

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("系统随机源不可用");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
