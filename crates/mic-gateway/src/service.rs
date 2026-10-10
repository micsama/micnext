use std::future::{Future, IntoFuture};
use std::pin::Pin;
use std::sync::Arc;

use axum::extract::DefaultBodyLimit;
use axum::handler::Handler;
use axum::http::StatusCode;
use axum::routing::{delete, get, post, put};
use axum::{middleware, Router};
use mic_core::{BoxError, Kernel, Service};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::Config;
use crate::limits::{MAX_BODY_BYTES, MAX_INPUT_BODY_BYTES, SHUTDOWN_GRACE, TOKEN_BYTES};
use crate::update::{RestartRequest, Updater};
use crate::{api, channels, log_stream, models, settings, sql, stream, update, web, DeveloperLogs};

pub(crate) struct Gateway {
    pub(crate) config: Config,
    pub(crate) logs: DeveloperLogs,
    pub(crate) restart: Option<mpsc::Sender<RestartRequest>>,
}

/// 各请求共享的只读状态。
pub(crate) struct App {
    pub(crate) kernel: Kernel,
    pub(crate) token: String,
    pub(crate) logs: DeveloperLogs,
    /// `None` = 未配置 `update_repo`。
    pub(crate) updater: Option<Arc<Updater>>,
    /// 进程停止时结束所有 SSE 流。
    pub(crate) stop: CancellationToken,
}

impl Service for Gateway {
    fn run(
        self: Box<Self>,
        kernel: Kernel,
        stop: CancellationToken,
    ) -> Pin<Box<dyn Future<Output = Result<(), BoxError>> + Send>> {
        Box::pin(serve(*self, kernel, stop))
    }
}

async fn serve(gateway: Gateway, kernel: Kernel, stop: CancellationToken) -> Result<(), BoxError> {
    let Gateway {
        config,
        logs,
        restart,
    } = gateway;
    web::ensure_built()?;
    let updater = match (config.update_repo, restart) {
        (None, _) => None,
        (Some(_), None) => {
            return Err("配置了 [gateway] update_repo，但装配根未接入重启通道".into())
        }
        (Some(repo), Some(restart)) => {
            if !repo.join("build.sh").is_file() {
                return Err(format!(
                    "[gateway] update_repo 指向的 {} 下没有 build.sh，请改成部署仓库根目录",
                    repo.display()
                )
                .into());
            }
            Some(Arc::new(Updater::new(repo, restart)))
        }
    };
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
        logs,
        updater,
        stop: stop.clone(),
    });
    let api = Router::new()
        .route("/channels/wechat", get(channels::status))
        .route("/channels/wechat/login", post(channels::begin))
        .route("/channels/wechat/login/{id}", delete(channels::cancel))
        .route(
            "/channels/wechat/login/{id}/code",
            post(channels::submit_code),
        )
        .route(
            "/sessions",
            get(api::list_sessions)
                .post(api::create_session.layer(DefaultBodyLimit::max(MAX_INPUT_BODY_BYTES))),
        )
        .route("/sessions/{id}", get(api::get_session))
        .route(
            "/sessions/{id}/messages",
            post(api::send_message.layer(DefaultBodyLimit::max(MAX_INPUT_BODY_BYTES))),
        )
        .route("/sessions/{id}/images/{image_id}", get(api::get_image))
        .route("/sessions/{id}/stream", get(stream::stream))
        .route("/sessions/{id}/persona", put(settings::set_session_persona))
        .route(
            "/settings",
            get(settings::get_settings).put(settings::put_settings),
        )
        .route("/sessions/{id}/model", put(models::set_session_model))
        .route("/model-kinds", get(models::list_kinds))
        .route(
            "/models",
            get(models::list_models).post(models::create_model),
        )
        .route(
            "/models/default",
            get(models::get_default).put(models::put_default),
        )
        .route(
            "/models/{id}",
            put(models::update_model).delete(models::delete_model),
        )
        .route(
            "/endpoints",
            get(models::list_endpoints).post(models::create_endpoint),
        )
        .route("/endpoints/test", post(models::test_endpoint))
        .route(
            "/endpoints/{id}",
            put(models::update_endpoint).delete(models::delete_endpoint),
        )
        .route(
            "/personas",
            get(settings::list_personas).post(settings::create_persona),
        )
        .route(
            "/personas/{id}",
            put(settings::update_persona).delete(settings::delete_persona),
        )
        .route("/developer/logs/stream", get(log_stream::stream))
        .route("/developer/sql", post(sql::query))
        .route("/developer/sql/schema", get(sql::schema))
        .route("/developer/update", get(update::status).post(update::start))
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
