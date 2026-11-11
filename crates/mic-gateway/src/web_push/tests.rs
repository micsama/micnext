use std::sync::{Arc, Mutex};

use axum::body::Bytes;
use axum::extract::State;
use axum::http::StatusCode;
use axum::routing::post;
use axum::Router;

use super::*;

type Received = Arc<Mutex<Vec<Vec<u8>>>>;

/// 收下请求体、按预设状态码应答的推送服务。
async fn mock(status: StatusCode) -> (String, Received) {
    let received = Arc::new(Mutex::new(Vec::new()));
    let app = Router::new()
        .route(
            "/push",
            post(
                |State((status, seen)): State<(StatusCode, Received)>, body: Bytes| async move {
                    seen.lock().unwrap().push(body.to_vec());
                    status
                },
            ),
        )
        .with_state((status, received.clone()));
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
    (format!("http://{addr}/push"), received)
}

fn keys() -> (Vapid, SecretKey, Vec<u8>) {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let conn = Connection::open_in_memory().unwrap();
    conn.execute_batch(include_str!("../schema.sql")).unwrap();
    let keys = vapid(&conn).unwrap();
    assert_eq!(vapid(&conn).unwrap().private_key, keys.private_key);
    let ua = SecretKey::from_slice(&ES256KeyPair::generate().to_bytes()).unwrap();
    (keys, ua, vec![7u8; 16])
}

fn subscription(endpoint: String, ua: &SecretKey, auth: &[u8]) -> Subscription {
    Subscription {
        endpoint,
        p256dh: ua.public_key().to_encoded_point(false).as_bytes().to_vec(),
        auth: auth.to_vec(),
    }
}

#[tokio::test]
async fn payload_decrypts_on_device() {
    let (endpoint, received) = mock(StatusCode::CREATED).await;
    let (vapid, ua, auth) = keys();
    let sub = subscription(endpoint, &ua, &auth);
    let http = client();
    assert_eq!(
        deliver(&http, &vapid, &sub, b"{\"title\":\"t\"}").await,
        Delivery::Sent
    );
    let body = received.lock().unwrap().pop().unwrap();
    let plain = web_push_native::decrypt(body, &ua, &Auth::clone_from_slice(&auth)).unwrap();
    assert_eq!(plain, b"{\"title\":\"t\"}");
}

#[tokio::test]
async fn gone_and_failure_are_distinguished() {
    let (vapid, ua, auth) = keys();
    let http = client();
    for (status, expected) in [
        (StatusCode::GONE, Some(Delivery::Gone)),
        (StatusCode::NOT_FOUND, Some(Delivery::Gone)),
        (StatusCode::TOO_MANY_REQUESTS, None),
    ] {
        let (endpoint, _) = mock(status).await;
        let got = deliver(&http, &vapid, &subscription(endpoint, &ua, &auth), b"x").await;
        match expected {
            Some(e) => assert_eq!(got, e),
            None => assert!(matches!(got, Delivery::Failed(r) if r.starts_with("429"))),
        }
    }
}

#[test]
fn subscription_boundary() {
    let ua = SecretKey::from_slice(&ES256KeyPair::generate().to_bytes()).unwrap();
    let p256dh = URL_SAFE_NO_PAD.encode(ua.public_key().to_encoded_point(false).as_bytes());
    let auth = URL_SAFE_NO_PAD.encode([1u8; 16]);
    let body = |endpoint: &str, p256dh: &str, auth: &str| SubscriptionBody {
        endpoint: endpoint.to_owned(),
        keys: SubscriptionKeys {
            p256dh: p256dh.to_owned(),
            auth: auth.to_owned(),
        },
    };
    assert!(body("https://web.push.apple.com/abc", &p256dh, &auth)
        .parse()
        .is_ok());
    for bad in [
        body("http://web.push.apple.com/abc", &p256dh, &auth),
        body("https://1.2.3.4/abc", &p256dh, &auth),
        body(
            &format!("https://a.com/{}", "x".repeat(2048)),
            &p256dh,
            &auth,
        ),
        body("https://a.com/x", "AAAA", &auth),
        body("https://a.com/x", &p256dh, "AAAA"),
    ] {
        assert!(bad.parse().is_err());
    }
}
