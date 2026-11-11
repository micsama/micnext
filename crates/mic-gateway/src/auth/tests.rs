use super::*;

const KEY: &[u8] = &[7u8; KEY_BYTES];

fn value(h: &HeaderValue) -> String {
    let s = h.to_str().unwrap();
    let (pair, _) = s.split_once(';').unwrap();
    pair.strip_prefix("micnext_session=").unwrap().to_owned()
}

#[test]
fn cookie_verifies_until_expiry_and_rejects_tampering() {
    let now = 1_000_000;
    let v = value(&issue(KEY, now));
    assert_eq!(verify(KEY, &v, now), Some(now + ttl_ms()));
    assert_eq!(verify(KEY, &v, now + ttl_ms()), None);
    assert_eq!(verify(&[8u8; KEY_BYTES], &v, now), None);
    let (exp, sig) = v.split_once('.').unwrap();
    let later = exp.parse::<i64>().unwrap() + 1;
    assert_eq!(verify(KEY, &format!("{later}.{sig}"), now), None);
    assert_eq!(verify(KEY, "garbage", now), None);
}

#[test]
fn renews_once_a_day() {
    let day = SESSION_RENEW_AFTER.as_millis() as i64;
    let expires_at = 5 * day + ttl_ms();
    assert!(!needs_renewal(expires_at, 5 * day + day - 1));
    assert!(needs_renewal(expires_at, 6 * day));
}

#[test]
fn cookie_is_read_among_others() {
    let mut h = HeaderMap::new();
    h.append(
        header::COOKIE,
        HeaderValue::from_static("a=1; micnext_session=x.y; b=2"),
    );
    assert_eq!(session_cookie(&h).as_deref(), Some("x.y"));
}

#[test]
fn failures_block_within_window_only() {
    let f = LoginFailures::default();
    let t0 = Instant::now();
    for _ in 0..LOGIN_FAILURES {
        assert!(!f.blocked(t0));
        f.record(t0);
    }
    assert!(f.blocked(t0));
    assert!(!f.blocked(t0 + LOGIN_WINDOW));
}

#[test]
fn password_change_rotates_key() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(async {
            let store = mic_store::Store::open_in_memory(crate::schema::SCHEMAS)
                .await
                .unwrap();
            assert!(store.with_module_tx(|tx| load(tx)).await.unwrap().is_none());
            assert_eq!(NewSecret::new("short").err().unwrap(), "新密码至少 8 位");
            let first = NewSecret::new("password-1").unwrap();
            store
                .with_module_tx(move |tx| first.save(tx))
                .await
                .unwrap();
            let a = store.with_module_tx(|tx| load(tx)).await.unwrap().unwrap();
            assert!(password_matches(&a.password_hash, "password-1"));
            assert!(!password_matches(&a.password_hash, "password-2"));
            let second = NewSecret::new("password-2").unwrap();
            store
                .with_module_tx(move |tx| second.save(tx))
                .await
                .unwrap();
            let b = store.with_module_tx(|tx| load(tx)).await.unwrap().unwrap();
            assert!(password_matches(&b.password_hash, "password-2"));
            let old = value(&issue(&a.session_key, 0));
            assert_eq!(verify(&b.session_key, &old, 0), None);
        });
}
