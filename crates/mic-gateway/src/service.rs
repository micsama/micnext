use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::routing::{get, post};
use axum::{middleware, Router};
use mic_core::{BoxError, Kernel, Service};
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::limits::{MAX_BODY_BYTES, TOKEN_BYTES};
use crate::{api, stream};

pub(crate) struct Gateway {
    pub(crate) config: Config,
}

/// 各请求共享的只读状态。
pub(crate) struct App {
    pub(crate) kernel: Kernel,
    pub(crate) token: String,
    pub(crate) workdir: String,
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
    tokio::fs::create_dir_all(&config.workdir)
        .await
        .map_err(|e| format!("无法创建 Web 会话工作目录 {}：{e}", config.workdir))?;
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
        workdir: config.workdir,
        stop: stop.clone(),
    });
    let api = Router::new()
        .route(
            "/sessions",
            get(api::list_sessions).post(api::create_session),
        )
        .route("/sessions/{id}/messages", post(api::send_message))
        .route("/sessions/{id}/stream", get(stream::stream))
        .layer(middleware::from_fn_with_state(app.clone(), api::authorize))
        .layer(DefaultBodyLimit::max(MAX_BODY_BYTES));
    let router = Router::new().nest("/api", api).with_state(app.clone());

    // NOTE: 面向用户的提示，含 token，故不走 tracing。
    eprintln!("Web 已启动：http://{}/#token={}", config.listen, app.token);
    tracing::info!(listen = %config.listen, "gateway listening");
    axum::serve(listener, router)
        .with_graceful_shutdown(async move { stop.cancelled().await })
        .await?;
    Ok(())
}

pub(crate) fn random_hex(bytes: usize) -> String {
    let mut buf = vec![0u8; bytes];
    getrandom::fill(&mut buf).expect("系统随机源不可用");
    buf.iter().map(|b| format!("{b:02x}")).collect()
}
