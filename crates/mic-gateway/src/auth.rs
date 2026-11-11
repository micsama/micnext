//! 登录制鉴权：单管理员，密码存 Argon2id，会话是 `session_key` 签名的无状态 cookie。
//! 契约：docs/blueprints/web-login.md。

use std::collections::VecDeque;
use std::path::Path;
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use argon2::password_hash::phc::PasswordHash;
use argon2::password_hash::{PasswordHasher, PasswordVerifier};
use argon2::Argon2;
use axum::extract::rejection::JsonRejection;
use axum::extract::{Request, State};
use axum::http::{header, HeaderMap, HeaderValue, StatusCode};
use axum::middleware::Next;
use axum::response::{IntoResponse, Response};
use axum::Json;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use mic_core::BoxError;
use ring::hmac;
use rusqlite::{params, Connection, OptionalExtension};
use serde::Deserialize;

use crate::api::json;
use crate::error::ApiError;
use crate::limits::{
    ADMIN_USERNAME, LOGIN_FAILURES, LOGIN_WINDOW, PASSWORD_MIN_CHARS, SESSION_RENEW_AFTER,
    SESSION_TTL,
};
use crate::service::App;

const COOKIE: &str = "micnext_session";
const KEY_BYTES: usize = 32;
const NO_PASSWORD: &str = "尚未设置密码：在服务器上运行 micnext --set-password";

pub(crate) struct Admin {
    password_hash: String,
    session_key: Vec<u8>,
}

fn load(conn: &Connection) -> rusqlite::Result<Option<Admin>> {
    conn.query_row(
        "SELECT password_hash, session_key FROM web_admin WHERE id = 1",
        [],
        |row| {
            Ok(Admin {
                password_hash: row.get(0)?,
                session_key: row.get(1)?,
            })
        },
    )
    .optional()
}

/// 新密码的哈希与新会话 key；写入即令旧 cookie 全部失效。
struct NewSecret {
    password_hash: String,
    session_key: Vec<u8>,
}

impl NewSecret {
    /// 校验长度并做 Argon2 哈希；耗 CPU，调用方放到阻塞线程。
    fn new(password: &str) -> Result<Self, String> {
        if password.chars().count() < PASSWORD_MIN_CHARS {
            return Err(format!("新密码至少 {PASSWORD_MIN_CHARS} 位"));
        }
        let password_hash = Argon2::default()
            .hash_password(password.as_bytes())
            .expect("Argon2 默认参数与系统随机源可用")
            .to_string();
        let mut session_key = vec![0u8; KEY_BYTES];
        getrandom::fill(&mut session_key).expect("系统随机源不可用");
        Ok(Self {
            password_hash,
            session_key,
        })
    }

    fn save(&self, conn: &Connection) -> rusqlite::Result<()> {
        conn.execute(
            "INSERT INTO web_admin (id, password_hash, session_key) VALUES (1, ?1, ?2)
             ON CONFLICT(id) DO UPDATE SET password_hash = excluded.password_hash,
                                           session_key = excluded.session_key",
            params![self.password_hash, self.session_key],
        )?;
        Ok(())
    }
}

fn password_matches(hash: &str, password: &str) -> bool {
    PasswordHash::new(hash).is_ok_and(|h| {
        Argon2::default()
            .verify_password(password.as_bytes(), &h)
            .is_ok()
    })
}

async fn blocking<R: Send + 'static>(f: impl FnOnce() -> R + Send + 'static) -> R {
    tokio::task::spawn_blocking(f)
        .await
        .expect("密码哈希任务 panic")
}

/// CLI `--set-password`：直接写库，服务运行中也可执行。库须由服务启动过一次建好。
pub fn set_password(db: &Path, password: &str) -> Result<(), BoxError> {
    if !db.is_file() {
        return Err(format!("数据库 {} 不存在：先启动一次服务，再设置密码", db.display()).into());
    }
    let secret = NewSecret::new(password)?;
    let conn = Connection::open(db)?;
    conn.busy_timeout(std::time::Duration::from_secs(5))?;
    let ready = conn
        .query_row(
            "SELECT 1 FROM sqlite_schema WHERE type = 'table' AND name = 'web_admin'",
            [],
            |_| Ok(()),
        )
        .optional()?;
    if ready.is_none() {
        return Err("数据库里还没有登录表：先用新版本启动一次服务，再设置密码".into());
    }
    secret.save(&conn)?;
    Ok(())
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("系统时钟早于 1970")
        .as_millis() as i64
}

fn ttl_ms() -> i64 {
    SESSION_TTL.as_millis() as i64
}

fn signature(key: &[u8], expires_at: i64) -> hmac::Tag {
    hmac::sign(
        &hmac::Key::new(hmac::HMAC_SHA256, key),
        expires_at.to_string().as_bytes(),
    )
}

/// 签发一张从 `now` 起有效 30 天的 cookie。
fn issue(key: &[u8], now: i64) -> HeaderValue {
    let expires_at = now + ttl_ms();
    let sig = URL_SAFE_NO_PAD.encode(signature(key, expires_at));
    let max_age = SESSION_TTL.as_secs();
    HeaderValue::from_str(&format!(
        "{COOKIE}={expires_at}.{sig}; HttpOnly; SameSite=Strict; Path=/; Max-Age={max_age}"
    ))
    .expect("cookie 只含 ASCII")
}

fn cleared() -> HeaderValue {
    HeaderValue::from_static("micnext_session=; HttpOnly; SameSite=Strict; Path=/; Max-Age=0")
}

/// 签名相符且未过期时返回到期时刻。
fn verify(key: &[u8], value: &str, now: i64) -> Option<i64> {
    let (expires_at, sig) = value.split_once('.')?;
    let sig = URL_SAFE_NO_PAD.decode(sig).ok()?;
    hmac::verify(
        &hmac::Key::new(hmac::HMAC_SHA256, key),
        expires_at.as_bytes(),
        &sig,
    )
    .ok()?;
    let expires_at: i64 = expires_at.parse().ok()?;
    (expires_at > now).then_some(expires_at)
}

/// 签发已超过 `SESSION_RENEW_AFTER`。
fn needs_renewal(expires_at: i64, now: i64) -> bool {
    expires_at - ttl_ms() + SESSION_RENEW_AFTER.as_millis() as i64 <= now
}

fn session_cookie(headers: &HeaderMap) -> Option<String> {
    headers
        .get_all(header::COOKIE)
        .iter()
        .filter_map(|v| v.to_str().ok())
        .flat_map(|v| v.split(';'))
        .find_map(|pair| {
            let (name, value) = pair.trim().split_once('=')?;
            (name == COOKIE).then(|| value.to_owned())
        })
}

/// 进程内的全局登录失败计数：窗口内满额后拒绝登录，成功不清零。
#[derive(Default)]
pub(crate) struct LoginFailures(Mutex<VecDeque<Instant>>);

impl LoginFailures {
    fn blocked(&self, now: Instant) -> bool {
        let mut q = self.0.lock().expect("登录计数锁不中毒");
        while q
            .front()
            .is_some_and(|t| now.duration_since(*t) >= LOGIN_WINDOW)
        {
            q.pop_front();
        }
        q.len() >= LOGIN_FAILURES
    }

    fn record(&self, now: Instant) {
        self.0.lock().expect("登录计数锁不中毒").push_back(now);
    }
}

pub(crate) async fn authorize(
    State(app): State<Arc<App>>,
    req: Request,
    next: Next,
) -> Result<Response, ApiError> {
    let value = session_cookie(req.headers()).ok_or(ApiError::Unauthorized)?;
    let admin = app
        .kernel
        .with_module_tx(|tx| load(tx))
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let now = now_ms();
    let expires_at = verify(&admin.session_key, &value, now).ok_or(ApiError::Unauthorized)?;
    let mut res = next.run(req).await;
    if needs_renewal(expires_at, now) {
        res.headers_mut()
            .append(header::SET_COOKIE, issue(&admin.session_key, now));
    }
    Ok(res)
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct LoginBody {
    username: String,
    password: String,
}

pub(crate) async fn login(
    State(app): State<Arc<App>>,
    body: Result<Json<LoginBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let body = json(body)?;
    if app.login_failures.blocked(Instant::now()) {
        return Err(ApiError::TooManyRequests("登录尝试过多，请稍后再试"));
    }
    let admin = app
        .kernel
        .with_module_tx(|tx| load(tx))
        .await?
        .ok_or_else(|| ApiError::Conflict(NO_PASSWORD.into()))?;
    let hash = admin.password_hash;
    let ok = body.username == ADMIN_USERNAME
        && blocking(move || password_matches(&hash, &body.password)).await;
    if !ok {
        app.login_failures.record(Instant::now());
        return Err(ApiError::LoginFailed);
    }
    Ok((
        StatusCode::NO_CONTENT,
        [(header::SET_COOKIE, issue(&admin.session_key, now_ms()))],
    )
        .into_response())
}

pub(crate) async fn logout() -> Response {
    (StatusCode::NO_CONTENT, [(header::SET_COOKIE, cleared())]).into_response()
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct PasswordBody {
    current: String,
    new: String,
}

pub(crate) async fn change_password(
    State(app): State<Arc<App>>,
    body: Result<Json<PasswordBody>, JsonRejection>,
) -> Result<Response, ApiError> {
    let body = json(body)?;
    let admin = app
        .kernel
        .with_module_tx(|tx| load(tx))
        .await?
        .ok_or(ApiError::Unauthorized)?;
    let secret = blocking(move || {
        if !password_matches(&admin.password_hash, &body.current) {
            return Err("当前密码不正确".to_owned());
        }
        NewSecret::new(&body.new)
    })
    .await
    .map_err(ApiError::BadRequest)?;
    let cookie = issue(&secret.session_key, now_ms());
    app.kernel.with_module_tx(move |tx| secret.save(tx)).await?;
    Ok((StatusCode::NO_CONTENT, [(header::SET_COOKIE, cookie)]).into_response())
}

#[cfg(test)]
mod tests;
