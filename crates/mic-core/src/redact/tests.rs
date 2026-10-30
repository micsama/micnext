use super::*;

fn scrub(known: &[(&str, &str)], rules: bool, text: &str) -> String {
    let mut s = text.to_owned();
    Scrub { known, rules }.text(&mut s);
    s
}

fn r(text: &str) -> String {
    scrub(&[], true, text)
}

#[test]
fn masks_credentials_and_keeps_ordinary_text() {
    let cases = [
        (
            "key sk-proj-AbCdEfGh1234567890XyZw done",
            "key sk-pr…XyZw done",
        ),
        (
            "jwt eyJhbGciOiJSUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.c2lnbmF0dXJlX3h4eA end",
            "jwt eyJhb…h4eA end",
        ),
        ("auth Bearer abc123def456ghi789jk", "auth Bearer ab…89jk"),
        ("Authorization: Bearer abc123def456", "Authorization: ***"),
        (
            "curl -H 'Cookie: SESSDATA=abc; bili_jct=def' https://x",
            "curl -H 'Cookie: ***' https://x",
        ),
        (
            r#"{"Set-Cookie": "sid=1; Path=/"}"#,
            r#"{"Set-Cookie": "***"}"#,
        ),
        (
            "SESSDATA=1a2b3c4d%2C1712345678%2Cabcde%2A51CjBxyz&b=1",
            "SESSDATA=1a…Bxyz&b=1",
        ),
        ("raw 1a2b3c4d,1712345678,abcde*51CjBxyz", "raw 1a…Bxyz"),
        (
            "?access_key=0123456789abcdef0123&ts=1",
            "?access_key=01…0123&ts=1",
        ),
        (
            r#"{"OPENAI_API_KEY": "x9y8z7w6v5u4t3s2r1q0p"}"#,
            r#"{"OPENAI_API_KEY": "x9…1q0p"}"#,
        ),
        (r#"token = "1234567890""#, r#"token = "1234567890""#),
        (
            "postgres://app:hunter2@db:5432/x",
            "postgres://app:***@db:5432/x",
        ),
        (
            "-----BEGIN PRIVATE KEY-----\nMIIEv\n-----END PRIVATE KEY-----",
            "-----BEGIN PRIVATE KEY-----***-----END PRIVATE KEY-----",
        ),
        // 不误伤：代码表达式、用量字段、哈希、普通 ID、图片 base64、作者名
        ("api_key = config.api_key", "api_key = config.api_key"),
        ("max_tokens: 40960000", "max_tokens: 40960000"),
        (
            "commit 19eb43a0c2f1e6d7b8a9c0d1e2f3a4b5c6d7e8f9",
            "commit 19eb43a0c2f1e6d7b8a9c0d1e2f3a4b5c6d7e8f9",
        ),
        (
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ",
            "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAYAAAAfFcSJ",
        ),
        (r#"author: "Jane Doe""#, r#"author: "Jane Doe""#),
    ];
    for (input, want) in cases {
        assert_eq!(r(input), want, "{input}");
    }
}

#[test]
fn exact_secrets_become_names_even_when_rules_off() {
    let redactor = Redactor::new(&[0xab; 32], Default::default());
    let known: Vec<(&str, &str)> = redactor
        .master_key
        .iter()
        .map(|k| (k.expose(), MASTER_KEY))
        .collect();
    let b64 = STANDARD.encode([0xab; 32]);
    assert_eq!(scrub(&known, false, &"ab".repeat(32)), MASTER_KEY);
    assert_eq!(
        scrub(&known, false, &format!("cat: {b64}")),
        "cat: <MASTER_KEY>"
    );
    assert_eq!(
        scrub(&[("plainkey", API_KEY)], false, "use plainkey now"),
        "use <API_KEY> now"
    );
    let token = "api_key = \"a1b2c3d4e5f6g7h8i9\"";
    assert_eq!(scrub(&[], false, token), token);
}

fn message(id: i64, body: MessageBody) -> mic_message::Message {
    mic_message::Message {
        id: mic_message::MessageId(id),
        session_id: mic_message::SessionId(1),
        body,
        created_at: 0,
        delivered_at: None,
    }
}

fn user(id: i64, text: &str) -> mic_message::Message {
    message(
        id,
        MessageBody::UserInput {
            person: mic_message::PersonId(1),
            parts: vec![ContentPart::Text { text: text.into() }],
        },
    )
}

#[test]
fn request_keeps_protocol_fields_and_honors_plain_after() {
    let secret = "ghp_0123456789abcdefghijKLMN";
    let mut req = ModelRequest {
        tool_choice: crate::provider::ToolChoice::Auto,
        system: format!("env {secret}"),
        messages: vec![
            message(
                1,
                MessageBody::Reply {
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
            ),
            user(2, &format!("new {secret} known")),
        ],
        tools: Vec::new(),
        images: Default::default(),
        cache_key: "c".into(),
        plain_after: Some(mic_message::MessageId(1)),
    };
    request(&[("known", API_KEY)], &mut req);
    assert_eq!(req.system, "env ghp_01…KLMN");
    let MessageBody::Reply { blocks, .. } = &req.messages[0].body else {
        unreachable!()
    };
    let ReplyBlock::ToolCall { id, args, .. } = &blocks[0] else {
        unreachable!()
    };
    assert_eq!(id, "call_1");
    assert_eq!(args["command"], "curl -H 'x: ghp_01…KLMN'");
    let MessageBody::UserInput { parts, .. } = &req.messages[1].body else {
        unreachable!()
    };
    assert_eq!(
        parts[0],
        ContentPart::Text {
            text: format!("new {secret} <API_KEY>")
        }
    );
}

#[derive(Default)]
struct Capture(std::sync::Mutex<Vec<ModelRequest>>);
impl Provider for Capture {
    fn model(&self) -> &str {
        "test"
    }
    fn stream(&self, req: ModelRequest) -> BoxStream<Result<ModelEvent, ProviderError>> {
        self.0.lock().unwrap().push(req);
        Box::pin(Empty)
    }
}
struct Empty;
impl futures_core::Stream for Empty {
    type Item = Result<ModelEvent, ProviderError>;
    fn poll_next(
        self: std::pin::Pin<&mut Self>,
        _: &mut std::task::Context<'_>,
    ) -> std::task::Poll<Option<Self::Item>> {
        std::task::Poll::Ready(None)
    }
}

#[tokio::test]
async fn env_vars_reject_bad_input_and_reach_the_model_masked() {
    let store = mic_store::Store::open_in_memory(&[]).await.unwrap();
    let env = Arc::new(EnvVars::default());
    for (name, value) in [
        ("DINGTALK_TOKEN", "short"),
        ("BASH_ENV", "long-enough-value"),
        ("PATH", "long-enough-value"),
        ("1BAD", "long-enough-value"),
        ("DINGTALK_TOKEN", "long-enough\0value"),
    ] {
        let set = env.set(&store, name.into(), SecretValue::new(value.into()));
        assert!(set.await.is_err(), "{name}");
    }
    let secret = "dt-0123456789";
    env.set(
        &store,
        "DINGTALK_TOKEN".into(),
        SecretValue::new(secret.into()),
    )
    .await
    .unwrap();

    let inner = Arc::new(Capture::default());
    let model = Arc::new(Redactor::new(&[0xab; 32], env)).wrap(inner.clone(), None);
    let mut req = ModelRequest {
        tool_choice: crate::provider::ToolChoice::Auto,
        system: String::new(),
        messages: vec![user(1, &format!("echo printed {secret}"))],
        tools: Vec::new(),
        images: Default::default(),
        cache_key: "c".into(),
        plain_after: Some(mic_message::MessageId(1)),
    };
    drop(model.stream(req.clone()));
    req = inner.0.lock().unwrap().pop().unwrap();
    assert_eq!(
        req.messages[0].body,
        user(1, "echo printed <DINGTALK_TOKEN>").body
    );
}
