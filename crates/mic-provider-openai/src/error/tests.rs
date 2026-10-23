use super::*;

#[derive(Debug)]
struct Transport(std::io::Error);

impl std::fmt::Display for Transport {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("transport failed")
    }
}

impl std::error::Error for Transport {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.0)
    }
}

#[test]
fn includes_transport_root_cause() {
    let error = Transport(std::io::Error::from_raw_os_error(61));
    let message = chain(&error);
    assert!(message.starts_with("transport failed: "));
    assert!(message.contains(&error.0.to_string()));
}

#[tokio::test]
async fn network_error_omits_secret_url() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let error = reqwest::Client::new()
        .get("http://user:password@localhost:99999/private?token=secret")
        .send()
        .await
        .unwrap_err();
    let error = provider(Failure::Network(error)).to_string();
    assert!(!error.contains("password"));
    assert!(!error.contains("token=secret"));
    assert!(!error.contains("/private"));
    assert!(error.contains("invalid port number"));
}
