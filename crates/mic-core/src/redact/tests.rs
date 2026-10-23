use super::*;

fn r(text: &str) -> String {
    redact(&[], text).into_owned()
}

#[test]
fn masks_credentials_and_keeps_ordinary_text() {
    let cases = [
        (
            "key sk-proj-AbCdEfGh1234567890XyZw done",
            "key sk-p***XyZw done",
        ),
        (
            "Authorization: Bearer abc123def456",
            "Authorization: Bearer ***",
        ),
        (r#"token = "1234567890""#, r#"token = "***""#),
        (
            r#"{"OPENAI_API_KEY": "x9y8z7w6v5u4t3s2r1q0p"}"#,
            r#"{"OPENAI_API_KEY": "x9y8***1q0p"}"#,
        ),
        (
            "postgres://app:hunter2@db:5432/x",
            "postgres://app:***@db:5432/x",
        ),
        (
            "-----BEGIN PRIVATE KEY-----\nMIIEv\n-----END PRIVATE KEY-----",
            "-----BEGIN PRIVATE KEY-----***-----END PRIVATE KEY-----",
        ),
        // 不误伤：代码表达式、用量字段、哈希、普通 ID
        ("api_key = config.api_key", "api_key = config.api_key"),
        ("max_tokens: 40960000", "max_tokens: 40960000"),
        (
            "commit 19eb43a0c2f1e6d7b8a9c0d1e2f3a4b5c6d7e8f9",
            "commit 19eb43a0c2f1e6d7b8a9c0d1e2f3a4b5c6d7e8f9",
        ),
    ];
    for (input, want) in cases {
        assert_eq!(r(input), want, "{input}");
    }
}

#[test]
fn masks_master_key_forms_and_own_key() {
    let redactor = Redactor::new(&[0xab; 32]);
    let known: Vec<&str> = redactor.known.iter().map(SecretValue::expose).collect();
    let hex = "ab".repeat(32);
    let b64 = STANDARD.encode([0xab; 32]);
    assert_eq!(redact(&known, &hex), "abab***abab");
    assert_eq!(redact(&known, &format!("cat: {b64}")), "cat: q6ur***q6s=");
    assert_eq!(redact(&["plainkey"], "use plainkey now"), "use *** now");
}

#[test]
fn request_keeps_protocol_fields() {
    let secret = "ghp_0123456789abcdefghijKLMN";
    let mut req = ModelRequest {
        tool_choice: crate::provider::ToolChoice::Auto,
        system: format!("env {secret}"),
        messages: vec![mic_message::Message {
            id: mic_message::MessageId(1),
            session_id: mic_message::SessionId(1),
            body: MessageBody::Reply {
                stats: mic_message::ReplyStats {
                    model_ms: 0,
                    first_output_ms: None,
                    usage: None,
                },
                model: "m".into(),
                blocks: vec![ReplyBlock::ToolCall {
                    id: "call_1".into(),
                    name: "bash".into(),
                    args: serde_json::json!({ "command": format!("curl -H 'x: {secret}'") }),
                }],
            },
            created_at: 0,
            delivered_at: None,
        }],
        tools: Vec::new(),
        images: Default::default(),
        cache_key: "c".into(),
    };
    redact_request(&[], &mut req);
    assert_eq!(req.system, "env ghp_***KLMN");
    let MessageBody::Reply { blocks, .. } = &req.messages[0].body else {
        unreachable!()
    };
    let ReplyBlock::ToolCall { id, args, .. } = &blocks[0] else {
        unreachable!()
    };
    assert_eq!(id, "call_1");
    assert_eq!(args["command"], "curl -H 'x: ghp_***KLMN'");
}
