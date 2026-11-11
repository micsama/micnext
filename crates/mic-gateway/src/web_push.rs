//! 系统通知（Web Push）：通知中心的镜像，新推送发给各订阅设备，最多一次。
//! 契约：docs/blueprints/web-push.md。

use std::sync::Arc;

use axum::extract::rejection::JsonRejection;
use axum::extract::State;
use axum::http::StatusCode;
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use mic_core::{Kernel, KernelEventKind};
use mic_store::{PushId, PushView};
use rusqlite::{params, Connection, OptionalExtension};
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use web_push_native::jwt_simple::algorithms::ES256KeyPair;
use web_push_native::p256::elliptic_curve::sec1::ToEncodedPoint;
use web_push_native::p256::{PublicKey, SecretKey};
use web_push_native::{Auth, WebPushBuilder};

use crate::api::json;
use crate::error::ApiError;
use crate::limits::{
    WEB_PUSH_BODY_CHARS, WEB_PUSH_CONTACT, WEB_PUSH_ENDPOINT_MAX, WEB_PUSH_TIMEOUT,
};
use crate::service::App;

/// VAPID 私钥与对应的未压缩公钥（65 字节）。
struct Vapid {
    private_key: Vec<u8>,
    public_key: Vec<u8>,
}

/// 首次使用时生成；之后一直沿用，换了会让所有订阅失效。
fn vapid(conn: &Connection) -> rusqlite::Result<Vapid> {
    let row = conn
        .query_row(
            "SELECT private_key, public_key FROM web_push_keys WHERE id = 1",
            [],
            |row| {
                Ok(Vapid {
                    private_key: row.get(0)?,
                    public_key: row.get(1)?,
                })
            },
        )
        .optional()?;
    if let Some(v) = row {
        return Ok(v);
    }
    let private_key = ES256KeyPair::generate().to_bytes();
    let public_key = SecretKey::from_slice(&private_key)
        .expect("刚生成的 P-256 私钥")
        .public_key()
        .to_encoded_point(false)
        .as_bytes()
        .to_vec();
    conn.execute(
        "INSERT INTO web_push_keys (id, private_key, public_key) VALUES (1, ?1, ?2)",
        params![private_key, public_key],
    )?;
    Ok(Vapid {
        private_key,
        public_key,
    })
}

struct Subscription {
    endpoint: String,
    p256dh: Vec<u8>,
    auth: Vec<u8>,
}

fn subscriptions(conn: &Connection) -> rusqlite::Result<Vec<Subscription>> {
    conn.prepare("SELECT endpoint, p256dh, auth FROM web_push_subscriptions")?
        .query_map([], |row| {
            Ok(Subscription {
                endpoint: row.get(0)?,
                p256dh: row.get(1)?,
                auth: row.get(2)?,
            })
        })?
        .collect()
}

fn remove(conn: &Connection, endpoint: &str) -> rusqlite::Result<()> {
    conn.execute(
        "DELETE FROM web_push_subscriptions WHERE endpoint = ?1",
        [endpoint],
    )?;
    Ok(())
}

#[derive(Serialize)]
pub(crate) struct KeyView {
    key: String,
}

pub(crate) async fn key(State(app): State<Arc<App>>) -> Result<Json<KeyView>, ApiError> {
    let v = app.kernel.with_module_tx(|tx| vapid(tx)).await?;
    Ok(Json(KeyView {
        key: URL_SAFE_NO_PAD.encode(v.public_key),
    }))
}

/// 浏览器 `PushSubscription.toJSON()`；其余字段（如 `expirationTime`）忽略。
#[derive(Deserialize)]
pub(crate) struct SubscriptionBody {
    endpoint: String,
    keys: SubscriptionKeys,
}

#[derive(Deserialize)]
struct SubscriptionKeys {
    p256dh: String,
    auth: String,
}

impl SubscriptionBody {
    /// 只接受指向公网推送服务的订阅：https、域名、长度有限，密钥长度符合 RFC 8291。
    fn parse(self) -> Result<Subscription, ApiError> {
        let invalid = |what: &str| ApiError::BadRequest(format!("订阅不合法：{what}"));
        if self.endpoint.len() > WEB_PUSH_ENDPOINT_MAX {
            return Err(invalid("endpoint 过长"));
        }
        let url = reqwest::Url::parse(&self.endpoint).map_err(|_| invalid("endpoint 不是 URL"))?;
        if url.scheme() != "https" || url.domain().is_none() {
            return Err(invalid("endpoint 须为 https 域名地址"));
        }
        let decode = |s: &str| URL_SAFE_NO_PAD.decode(s.trim_end_matches('='));
        let p256dh = decode(&self.keys.p256dh)
            .ok()
            .filter(|k| k.len() == 65 && PublicKey::from_sec1_bytes(k).is_ok())
            .ok_or_else(|| invalid("p256dh 不是 P-256 公钥"))?;
        let auth = decode(&self.keys.auth)
            .ok()
            .filter(|k| k.len() == 16)
            .ok_or_else(|| invalid("auth 须为 16 字节"))?;
        Ok(Subscription {
            endpoint: self.endpoint,
            p256dh,
            auth,
        })
    }
}

pub(crate) async fn subscribe(
    State(app): State<Arc<App>>,
    body: Result<Json<SubscriptionBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let sub = json(body)?.parse()?;
    let now = now_ms();
    app.kernel
        .with_module_tx(move |tx| {
            tx.execute(
                "INSERT INTO web_push_subscriptions (endpoint, p256dh, auth, created_at)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT(endpoint) DO UPDATE SET p256dh = excluded.p256dh, auth = excluded.auth",
                params![sub.endpoint, sub.p256dh, sub.auth, now],
            )
        })
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct UnsubscribeBody {
    endpoint: String,
}

pub(crate) async fn unsubscribe(
    State(app): State<Arc<App>>,
    body: Result<Json<UnsubscribeBody>, JsonRejection>,
) -> Result<StatusCode, ApiError> {
    let endpoint = json(body)?.endpoint;
    app.kernel
        .with_module_tx(move |tx| remove(tx, &endpoint))
        .await?;
    Ok(StatusCode::NO_CONTENT)
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_millis() as i64
}

/// service worker 读取的通知内容。
#[derive(Serialize)]
struct Payload {
    title: String,
    body: String,
    url: String,
}

impl Payload {
    fn of(push: &PushView) -> Self {
        let mut body: String = push.text.chars().take(WEB_PUSH_BODY_CHARS).collect();
        if body.len() < push.text.len() {
            body.push('…');
        }
        if !push.files.is_empty() {
            if !body.is_empty() {
                body.push('\n');
            }
            body.push_str(&format!("附 {} 个文件", push.files.len()));
        }
        Self {
            title: push
                .source_session_title
                .clone()
                .unwrap_or_else(|| "micnext".to_owned()),
            body,
            url: format!("/s/{}", push.source_session_id.0),
        }
    }
}

pub(crate) fn client() -> reqwest::Client {
    reqwest::Client::builder()
        .timeout(WEB_PUSH_TIMEOUT)
        .build()
        .expect("TLS 后端已在进程启动时安装")
}

/// 跟随内核事件；只在运行期间镜像，启动时不回放、结果未知不重放。
pub(crate) async fn run(kernel: Kernel, http: reqwest::Client, stop: CancellationToken) {
    let mut events = kernel.subscribe();
    loop {
        let event = tokio::select! {
            () = stop.cancelled() => return,
            event = events.recv() => event,
        };
        match event {
            Ok(event) => {
                if let KernelEventKind::PushCreated { push_id } = event.kind {
                    tokio::spawn(mirror(kernel.clone(), http.clone(), push_id));
                }
            }
            Err(lagged) => tracing::warn!(%lagged, "web push missed events"),
        }
    }
}

async fn mirror(kernel: Kernel, http: reqwest::Client, push_id: PushId) {
    if let Err(error) = try_mirror(&kernel, &http, push_id).await {
        tracing::warn!(push = push_id.0, error = %crate::error::chain(&error), "web push failed");
    }
}

async fn try_mirror(
    kernel: &Kernel,
    http: &reqwest::Client,
    push_id: PushId,
) -> Result<(), mic_core::KernelError> {
    let (vapid, subs) = kernel
        .with_module_tx(|tx| Ok((vapid(tx)?, subscriptions(tx)?)))
        .await?;
    if subs.is_empty() {
        return Ok(());
    }
    let page = kernel.pushes(Some(PushId(push_id.0 + 1)), 1).await?;
    let Some(push) = page.pushes.into_iter().find(|p| p.id == push_id) else {
        return Ok(());
    };
    let payload = serde_json::to_vec(&Payload::of(&push)).expect("通知内容可序列化");
    for sub in subs {
        match deliver(http, &vapid, &sub, &payload).await {
            Delivery::Sent => {}
            Delivery::Gone => {
                let endpoint = sub.endpoint.clone();
                kernel
                    .with_module_tx(move |tx| remove(tx, &endpoint))
                    .await?;
            }
            Delivery::Failed(reason) => tracing::warn!(
                push = push_id.0,
                host = host(&sub.endpoint),
                reason,
                "web push failed"
            ),
        }
    }
    Ok(())
}

fn host(endpoint: &str) -> String {
    reqwest::Url::parse(endpoint)
        .ok()
        .and_then(|u| u.host_str().map(str::to_owned))
        .unwrap_or_default()
}

#[derive(Debug, PartialEq)]
enum Delivery {
    Sent,
    /// 404/410：订阅已失效，删掉。
    Gone,
    Failed(String),
}

async fn deliver(
    http: &reqwest::Client,
    vapid: &Vapid,
    sub: &Subscription,
    payload: &[u8],
) -> Delivery {
    let request = (|| -> Result<reqwest::Request, String> {
        let key = ES256KeyPair::from_bytes(&vapid.private_key).map_err(|e| e.to_string())?;
        let ua = PublicKey::from_sec1_bytes(&sub.p256dh).map_err(|e| e.to_string())?;
        let request = WebPushBuilder::new(
            sub.endpoint.parse().map_err(|e| format!("{e}"))?,
            ua,
            Auth::clone_from_slice(&sub.auth),
        )
        .with_vapid(&key, WEB_PUSH_CONTACT)
        .build(payload.to_vec())
        .map_err(|e| e.to_string())?;
        reqwest::Request::try_from(request).map_err(|e| e.to_string())
    })();
    let request = match request {
        Ok(r) => r,
        Err(reason) => return Delivery::Failed(reason),
    };
    match http.execute(request).await {
        Ok(res) if res.status().is_success() => Delivery::Sent,
        Ok(res) if matches!(res.status().as_u16(), 404 | 410) => Delivery::Gone,
        Ok(res) => {
            let status = res.status();
            let body = res.text().await.unwrap_or_default();
            Delivery::Failed(format!(
                "{status} {}",
                body.chars().take(200).collect::<String>()
            ))
        }
        Err(e) => Delivery::Failed(crate::error::chain(&e)),
    }
}

#[cfg(test)]
mod tests;
