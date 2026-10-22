use super::*;

#[test]
fn transport_diagnostics_keep_causes_without_request_urls() {
    let error = HttpClient::builder().user_agent("\n").build().unwrap_err();
    let mut causes = Vec::new();
    let mut source = error.source();
    while let Some(cause) = source {
        causes.push(cause.to_string());
        source = cause.source();
    }
    assert!(!causes.is_empty());
    let url = Url::parse("https://user:password@example.com/media?token=secret").unwrap();
    let error = network_error("cdn_download", error.with_url(url));
    let detail = error.diagnostic();
    assert_eq!(error.to_string(), "微信网络连接失败");
    assert!(detail.contains("endpoint=cdn_download"));
    for cause in causes {
        assert!(detail.contains(&cause));
    }
    for secret in ["password", "example.com", "token=secret"] {
        assert!(!detail.contains(secret), "{detail}");
    }
}
