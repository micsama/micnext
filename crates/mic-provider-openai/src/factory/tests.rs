//! 模拟上游走完整链路：请求体、SSE 分帧、累积、错误分类、取消（provider-handwritten §六）。

use std::sync::Arc;
use std::time::Duration;

use futures_util::StreamExt;
use mic_message::{
    AssistantPhase, ContentPart, ImageData, ImageFormat, ImageId, ImageRef, Message, MessageBody,
    MessageId, Reasoning, ReplyBlock, SessionId,
};
use mic_store::Usage;
use serde_json::{json, Value};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::oneshot;

use super::*;
use crate::config::{Catalog, Dialect};
use mic_core::{ModelResponse, StopReason, ToolChoice};

struct Captured {
    head: String,
    body: Value,
}

/// 单连接上游：收下请求后回 `response`，关闭连接。
async fn upstream(response: String) -> (String, oneshot::Receiver<Captured>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (tx, rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        let captured = read_request(&mut sock).await;
        sock.write_all(response.as_bytes()).await.unwrap();
        sock.shutdown().await.ok();
        tx.send(captured).ok();
    });
    (url, rx)
}

async fn read_request(sock: &mut TcpStream) -> Captured {
    let mut buf = Vec::new();
    let mut chunk = [0u8; 4096];
    let split = loop {
        let n = sock.read(&mut chunk).await.unwrap();
        buf.extend_from_slice(&chunk[..n]);
        if let Some(i) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            break i + 4;
        }
    };
    let head = String::from_utf8(buf[..split].to_vec()).unwrap();
    let len = head
        .lines()
        .find_map(|l| {
            l.to_ascii_lowercase()
                .strip_prefix("content-length:")
                .map(|v| v.trim().parse::<usize>().unwrap())
        })
        .unwrap_or(0);
    while buf.len() < split + len {
        let n = sock.read(&mut chunk).await.unwrap();
        buf.extend_from_slice(&chunk[..n]);
    }
    let body = if len == 0 {
        Value::Null
    } else {
        serde_json::from_slice(&buf[split..split + len]).unwrap()
    };
    Captured { head, body }
}

fn sse(events: &[Value], done: bool) -> String {
    let mut body = String::from(
        "HTTP/1.1 200 OK\r\ncontent-type: text/event-stream\r\nconnection: close\r\n\r\n",
    );
    for e in events {
        body.push_str(&format!("data: {e}\n\n"));
    }
    if done {
        body.push_str("data: [DONE]\n\n");
    }
    body
}

fn status(code: u16, extra: &str, body: &str) -> String {
    format!(
        "HTTP/1.1 {code} X\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n{extra}\r\n{body}",
        body.len()
    )
}

fn install_crypto() {
    let _ = rustls::crypto::ring::default_provider().install_default();
}

fn provider(url: &str, protocol: Protocol, headers: HeaderMap) -> OpenAiProvider {
    install_crypto();
    OpenAiProvider {
        client: Client::build(url, headers).unwrap(),
        cfg: Resolved {
            model: "m".into(),
            protocol,
            max_tokens: Some(100),
            reasoning_effort: Some(ReasoningEffort::High),
        },
    }
}

fn msg(id: i64, body: MessageBody) -> Message {
    Message {
        id: MessageId(id),
        session_id: SessionId(1),
        body,
        created_at: 0,
        delivered_at: None,
    }
}

/// 一轮：带图的用户输入、同模型的历史回复（推理 + 正文 + 工具调用）、工具结果。
fn history() -> ModelRequest {
    let image = ImageId(7);
    ModelRequest {
        tool_choice: ToolChoice::Auto,
        system: "sys".into(),
        messages: vec![
            msg(
                1,
                MessageBody::HarnessNote {
                    text: "note".into(),
                },
            ),
            msg(
                2,
                MessageBody::UserInput {
                    person: mic_message::PersonId(1),
                    parts: vec![
                        ContentPart::Text {
                            text: "看图".into(),
                        },
                        ContentPart::Image(ImageRef { id: image }),
                    ],
                },
            ),
            msg(
                3,
                MessageBody::Reply {
                    model: "m".into(),
                    blocks: vec![
                        ReplyBlock::Reasoning(Reasoning::Visible {
                            text: "想".into(),
                            signature: Some("enc".into()),
                        }),
                        ReplyBlock::Text {
                            text: "好".into(),
                            phase: Some(AssistantPhase::Commentary),
                        },
                        ReplyBlock::ToolCall {
                            id: "c1".into(),
                            name: "ls".into(),
                            args: json!({"p": 1}),
                        },
                    ],
                },
            ),
            msg(
                4,
                MessageBody::ToolResult {
                    tool_name: "ls".into(),
                    tool_call_id: "c1".into(),
                    outcome: mic_message::ToolResultOutcome::Terminal(
                        mic_message::ExecOutcome::Completed {
                            output: vec![ContentPart::Text { text: "out".into() }],
                        },
                    ),
                },
            ),
        ],
        tools: vec![mic_tool::ToolSpec {
            name: "ls".into(),
            description: "list".into(),
            parameters: json!({"type": "object"}),
        }],
        images: [(
            image,
            ImageData {
                format: ImageFormat::Png,
                bytes: Arc::from(&b"png"[..]),
            },
        )]
        .into(),
        cache_key: "ck".into(),
    }
}

type ResultCheck = fn(&Result<ModelResponse, ProviderError>) -> bool;
type ErrorCheck = fn(&ProviderError) -> bool;

struct Outcome {
    deltas: Vec<String>,
    result: Result<ModelResponse, ProviderError>,
}

async fn run(p: &OpenAiProvider, req: ModelRequest) -> Outcome {
    let mut s = p.stream(req);
    let mut deltas = Vec::new();
    while let Some(item) = s.next().await {
        match item {
            Ok(ModelEvent::TextDelta(t)) => deltas.push(format!("T:{t}")),
            Ok(ModelEvent::ReasoningDelta(r)) => deltas.push(format!("R:{r}")),
            Ok(ModelEvent::Finished(resp)) => {
                assert!(s.next().await.is_none(), "终态后仍有事件");
                return Outcome {
                    deltas,
                    result: Ok(resp),
                };
            }
            Err(e) => {
                assert!(s.next().await.is_none(), "错误后仍有事件");
                return Outcome {
                    deltas,
                    result: Err(e),
                };
            }
        }
    }
    panic!("流在终态前结束");
}

fn chunk(delta: Value, finish: Option<&str>) -> Value {
    json!({"choices": [{"index": 0, "delta": delta, "finish_reason": finish}]})
}

#[tokio::test]
async fn chat_deepseek_round_trip() {
    let (url, rx) = upstream(sse(
        &[
            chunk(json!({"role": "assistant", "reasoning_content": "思"}), None),
            chunk(json!({"reasoning_content": "考", "content": ""}), None),
            chunk(json!({"content": "答"}), None),
            chunk(
                json!({"tool_calls": [{"index": 0, "id": "x", "type": "function", "function": {"name": "ls", "arguments": "{\"a\""}}]}),
                None,
            ),
            chunk(
                json!({"tool_calls": [{"index": 0, "function": {"arguments": ":1}"}}]}),
                Some("tool_calls"),
            ),
            json!({"choices": [], "usage": {"prompt_tokens": 10, "completion_tokens": 5, "prompt_cache_hit_tokens": 8, "completion_tokens_details": {"reasoning_tokens": 2}}}),
        ],
        true,
    ))
    .await;
    let p = provider(&url, Protocol::Chat(Dialect::DeepSeek), HeaderMap::new());
    let out = run(&p, history()).await;
    assert_eq!(out.deltas, ["R:思", "R:考", "T:答"]);
    let resp = out.result.unwrap();
    assert_eq!(resp.stop, StopReason::ToolUse);
    assert_eq!(
        resp.blocks,
        [
            ReplyBlock::Reasoning(Reasoning::Visible {
                text: "思考".into(),
                signature: None
            }),
            ReplyBlock::Text {
                text: "答".into(),
                phase: None
            },
            ReplyBlock::ToolCall {
                id: "x".into(),
                name: "ls".into(),
                args: json!({"a": 1})
            },
        ]
    );
    assert_eq!(
        resp.usage,
        Some(Usage {
            input_tokens: 10,
            output_tokens: 5,
            cache_read_tokens: Some(8),
            cache_write_tokens: None,
            reasoning_tokens: Some(2),
        })
    );

    let req = rx.await.unwrap();
    assert!(req.head.starts_with("POST /chat/completions "));
    let b = req.body;
    assert_eq!(b["model"], "m");
    assert_eq!(b["stream"], true);
    assert_eq!(b["stream_options"], json!({"include_usage": true}));
    assert_eq!(b["max_tokens"], 100);
    assert_eq!(b["reasoning_effort"], "high");
    assert!(b.get("tool_choice").is_none());
    assert_eq!(
        b["tools"],
        json!([{"type": "function", "function": {"name": "ls", "description": "list", "parameters": {"type": "object"}}}])
    );
    let m = &b["messages"];
    assert_eq!(m[0], json!({"role": "system", "content": "sys"}));
    assert_eq!(m[1]["role"], "user");
    assert!(m[1]["content"].is_string());
    assert_eq!(m[2]["content"][0]["type"], "text");
    assert_eq!(
        m[2]["content"][1],
        json!({"type": "image_url", "image_url": {"url": "data:image/png;base64,cG5n"}})
    );
    assert_eq!(
        m[3],
        json!({
            "role": "assistant",
            "content": "好",
            "tool_calls": [{"type": "function", "id": "c1", "function": {"name": "ls", "arguments": "{\"p\":1}"}}],
            "reasoning_content": "想",
        })
    );
    assert_eq!(
        m[4],
        json!({"role": "tool", "content": "out", "tool_call_id": "c1"})
    );
}

#[tokio::test]
async fn chat_dialects_and_termination() {
    // Ollama 推理字段、无 [DONE] 正常 EOF。
    let (url, _rx) = upstream(sse(
        &[
            chunk(
                json!({"reasoning": "r", "reasoning_content": "ignored"}),
                None,
            ),
            chunk(json!({"content": "t"}), Some("stop")),
        ],
        false,
    ))
    .await;
    let p = provider(&url, Protocol::Chat(Dialect::Ollama), HeaderMap::new());
    let out = run(&p, history()).await;
    assert_eq!(out.deltas, ["R:r", "T:t"]);
    let resp = out.result.unwrap();
    assert_eq!(resp.stop, StopReason::EndTurn);
    assert!(resp.usage.is_none());

    // Generic 缓存统计；不回传推理。
    let (url, rx) = upstream(sse(
        &[
            chunk(json!({"content": "t", "reasoning_content": "ignored"}), Some("length")),
            json!({"choices": [], "usage": {"prompt_tokens": 3, "completion_tokens": 1, "prompt_tokens_details": {"cached_tokens": 2}}}),
        ],
        true,
    ))
    .await;
    let p = provider(&url, Protocol::Chat(Dialect::Generic), HeaderMap::new());
    let out = run(&p, history()).await;
    assert_eq!(out.deltas, ["T:t"]);
    let resp = out.result.unwrap();
    assert_eq!(resp.stop, StopReason::MaxTokens);
    assert_eq!(resp.usage.unwrap().cache_read_tokens, Some(2));
    assert!(rx.await.unwrap().body["messages"][3]
        .get("reasoning_content")
        .is_none());

    // 缺 finish_reason → Transient；finish_reason 之后仍有增量 → Protocol；资源不足 → Transient。
    for (events, check) in [
        (
            vec![chunk(json!({"content": "t"}), None)],
            (|e: &ProviderError| matches!(e, ProviderError::Transient { .. }))
                as fn(&ProviderError) -> bool,
        ),
        (
            vec![
                chunk(json!({}), Some("stop")),
                chunk(json!({"content": "late"}), None),
            ],
            |e| matches!(e, ProviderError::Protocol { .. }),
        ),
        (
            vec![chunk(json!({}), Some("insufficient_system_resource"))],
            |e| matches!(e, ProviderError::Transient { .. }),
        ),
        (vec![chunk(json!({}), Some("weird"))], |e| {
            matches!(e, ProviderError::Protocol { .. })
        }),
    ] {
        let (url, _rx) = upstream(sse(&events, true)).await;
        let p = provider(&url, Protocol::Chat(Dialect::Generic), HeaderMap::new());
        let err = run(&p, history()).await.result.unwrap_err();
        assert!(check(&err), "{err:?}");
    }
}

fn item_done(index: u32, item: Value) -> Value {
    json!({"type": "response.output_item.done", "output_index": index, "item": item})
}

fn usage() -> Value {
    json!({"input_tokens": 9, "input_tokens_details": {"cached_tokens": 4}, "output_tokens": 3, "output_tokens_details": {"reasoning_tokens": 1}, "total_tokens": 12})
}

#[tokio::test]
async fn responses_codex_round_trip() {
    let (url, rx) = upstream(sse(
        &[
            json!({"type": "response.created", "response": {"id": "r"}}),
            json!({"type": "response.some_future_event", "x": 1}),
            json!({"type": "response.reasoning_summary_text.delta", "delta": "想"}),
            json!({"type": "response.output_text.delta", "delta": "答"}),
            item_done(2, json!({"type": "function_call", "id": "fc", "call_id": "c2", "name": "ls", "arguments": "{}", "status": "completed"})),
            item_done(1, json!({"type": "message", "id": "m", "role": "assistant", "status": "completed", "phase": "final_answer", "content": [{"type": "output_text", "text": "答", "annotations": []}]})),
            item_done(0, json!({"type": "reasoning", "id": "rs", "summary": [{"type": "summary_text", "text": "a"}, {"type": "summary_text", "text": "b"}], "encrypted_content": "E"})),
            json!({"type": "response.completed", "response": {"id": "r", "output": [], "usage": usage()}}),
        ],
        false,
    ))
    .await;
    let mut headers = HeaderMap::new();
    headers.insert("chatgpt-account-id", HeaderValue::from_static("acc"));
    let p = provider(&url, Protocol::Responses(Backend::Codex), headers);
    let out = run(&p, history()).await;
    assert_eq!(out.deltas, ["R:想", "T:答"]);
    let resp = out.result.unwrap();
    assert_eq!(resp.stop, StopReason::ToolUse);
    assert_eq!(
        resp.blocks,
        [
            ReplyBlock::Reasoning(Reasoning::Visible {
                text: "a\n\nb".into(),
                signature: Some("E".into())
            }),
            ReplyBlock::Text {
                text: "答".into(),
                phase: Some(AssistantPhase::FinalAnswer)
            },
            ReplyBlock::ToolCall {
                id: "c2".into(),
                name: "ls".into(),
                args: json!({})
            },
        ]
    );
    assert_eq!(
        resp.usage,
        Some(Usage {
            input_tokens: 9,
            output_tokens: 3,
            cache_read_tokens: Some(4),
            cache_write_tokens: None,
            reasoning_tokens: Some(1),
        })
    );

    let req = rx.await.unwrap();
    assert!(req.head.starts_with("POST /responses "));
    let head = req.head.to_ascii_lowercase();
    assert!(head.contains("\r\nsession_id: ck\r\n"));
    assert!(head.contains("\r\nchatgpt-account-id: acc\r\n"));
    let b = req.body;
    assert_eq!(b["instructions"], "sys");
    assert_eq!(b["store"], false);
    assert_eq!(b["stream"], true);
    assert_eq!(b["prompt_cache_key"], "ck");
    assert_eq!(b["include"], json!(["reasoning.encrypted_content"]));
    assert_eq!(b["reasoning"], json!({"effort": "high", "summary": "auto"}));
    assert_eq!(b["max_output_tokens"], 100);
    assert_eq!(
        b["tools"],
        json!([{"type": "function", "name": "ls", "description": "list", "parameters": {"type": "object"}, "strict": false}])
    );
    let i = &b["input"];
    assert_eq!(i[0]["type"], "message");
    assert_eq!(i[0]["role"], "user");
    assert!(i[0]["content"].is_string());
    assert_eq!(
        i[1]["content"][1],
        json!({"type": "input_image", "image_url": "data:image/png;base64,cG5n", "detail": "auto"})
    );
    assert_eq!(
        i[2],
        json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "想"}], "encrypted_content": "enc"})
    );
    assert_eq!(
        i[3],
        json!({"type": "message", "role": "assistant", "content": "好", "phase": "commentary"})
    );
    assert_eq!(
        i[4],
        json!({"type": "function_call", "call_id": "c1", "name": "ls", "arguments": "{\"p\":1}"})
    );
    assert_eq!(
        i[5],
        json!({"type": "function_call_output", "call_id": "c1", "output": "out"})
    );
}

#[tokio::test]
async fn responses_terminal_states() {
    let refusal = item_done(
        0,
        json!({"type": "message", "id": "m", "role": "assistant", "status": "completed", "content": [{"type": "refusal", "refusal": "不"}]}),
    );
    let cases: Vec<(Vec<Value>, ResultCheck)> = vec![
        (
            vec![
                refusal.clone(),
                json!({"type": "response.completed", "response": {}}),
            ],
            |r| matches!(r, Ok(resp) if resp.stop == StopReason::ContentFilter),
        ),
        (
            vec![
                refusal,
                json!({"type": "response.incomplete", "response": {"incomplete_details": {"reason": "max_output_tokens"}, "usage": usage()}}),
            ],
            |r| matches!(r, Ok(resp) if resp.stop == StopReason::MaxTokens && resp.usage.is_some()),
        ),
        (
            vec![
                json!({"type": "response.incomplete", "response": {"incomplete_details": {"reason": "other"}}}),
            ],
            |r| matches!(r, Err(ProviderError::Protocol { .. })),
        ),
        (
            vec![
                json!({"type": "response.failed", "response": {"error": {"code": "insufficient_quota", "message": "m"}}}),
            ],
            |r| matches!(r, Err(ProviderError::Account { .. })),
        ),
        (
            vec![json!({"type": "error", "code": "rate_limit_exceeded", "message": "m"})],
            |r| matches!(r, Err(ProviderError::Transient { .. })),
        ),
        (
            vec![json!({"type": "error", "code": null, "message": "m"})],
            |r| matches!(r, Err(ProviderError::Rejected { .. })),
        ),
        (
            vec![
                item_done(0, json!({"type": "web_search_call", "id": "w"})),
                json!({"type": "response.completed", "response": {}}),
            ],
            |r| matches!(r, Err(ProviderError::Protocol { .. })),
        ),
        (
            vec![json!({"type": "response.output_text.delta", "delta": "t"})],
            |r| matches!(r, Err(ProviderError::Transient { .. })),
        ),
        (
            vec![
                json!({"type": "response.completed", "response": {"usage": {"input_tokens": 1, "input_tokens_details": {"cached_tokens": 0, "cache_write_tokens": -1}, "output_tokens": 1, "output_tokens_details": {"reasoning_tokens": 0}}}}),
            ],
            |r| matches!(r, Err(ProviderError::Protocol { .. })),
        ),
    ];
    for (events, check) in cases {
        let (url, _rx) = upstream(sse(&events, false)).await;
        let p = provider(&url, Protocol::Responses(Backend::OpenAi), HeaderMap::new());
        let result = run(&p, history()).await.result;
        assert!(check(&result), "{events:?} → {:?}", result.map(|r| r.stop));
    }
}

#[tokio::test]
async fn http_failures_are_classified() {
    let cases: Vec<(String, ErrorCheck)> = vec![
        (
            status(401, "", r#"{"error":{"message":"bad key","code":null}}"#),
            |e| matches!(e, ProviderError::Account { message } if message.contains("bad key")),
        ),
        (
            status(402, "", "{}"),
            |e| matches!(e, ProviderError::Account { message } if message.contains("充值")),
        ),
        (
            status(429, "retry-after: 7\r\n", "not json"),
            |e| matches!(e, ProviderError::Transient { retry_after: Some(d), .. } if *d == Duration::from_secs(7)),
        ),
        (
            status(
                429,
                "",
                r#"{"error":{"message":"q","code":"insufficient_quota"}}"#,
            ),
            |e| matches!(e, ProviderError::Account { message } if message.contains("insufficient_quota")),
        ),
        (status(503, "", ""), |e| {
            matches!(
                e,
                ProviderError::Transient {
                    retry_after: None,
                    ..
                }
            )
        }),
        (
            status(
                400,
                "",
                r#"{"error":{"message":"too long","type":"invalid_request_error"}}"#,
            ),
            |e| matches!(e, ProviderError::Rejected { message } if message.contains("too long")),
        ),
    ];
    for (response, check) in cases {
        let (url, _rx) = upstream(response).await;
        let p = provider(&url, Protocol::Chat(Dialect::Generic), HeaderMap::new());
        let err = run(&p, history()).await.result.unwrap_err();
        assert!(check(&err), "{err:?}");
    }
}

#[tokio::test]
async fn dropping_the_stream_closes_the_connection() {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let (closed_tx, closed_rx) = oneshot::channel();
    tokio::spawn(async move {
        let (mut sock, _) = listener.accept().await.unwrap();
        read_request(&mut sock).await;
        let head = sse(&[chunk(json!({"content": "t"}), None)], false);
        sock.write_all(head.as_bytes()).await.unwrap();
        let mut buf = [0u8; 64];
        // 客户端丢弃流后读到 EOF。
        while sock.read(&mut buf).await.unwrap() > 0 {}
        closed_tx.send(()).ok();
    });
    let p = provider(&url, Protocol::Chat(Dialect::Generic), HeaderMap::new());
    let mut s = p.stream(history());
    assert!(matches!(s.next().await, Some(Ok(ModelEvent::TextDelta(_)))));
    drop(s);
    tokio::time::timeout(Duration::from_secs(5), closed_rx)
        .await
        .expect("丢弃流后连接未关闭")
        .unwrap();
}

#[tokio::test]
async fn probe_lists_models() {
    install_crypto();
    let (url, rx) = upstream(status(
        200,
        "",
        r#"{"object":"list","data":[{"id":"a"},{"id":"b","owned_by":"x"}]}"#,
    ))
    .await;
    let client = Client::build(&url, HeaderMap::new()).unwrap();
    let names = probe::list_models(client, Catalog::Standard).await.unwrap();
    assert_eq!(names, ["a", "b"]);
    assert!(rx.await.unwrap().head.starts_with("GET /models HTTP"));

    let (url, rx) = upstream(status(200, "", r#"{"models":[{"slug":"gpt"}]}"#)).await;
    let client = Client::build(&url, HeaderMap::new()).unwrap();
    let names = probe::list_models(client, Catalog::Codex).await.unwrap();
    assert_eq!(names, ["gpt"]);
    assert!(rx.await.unwrap().head.starts_with(&format!(
        "GET /models?client_version={} HTTP",
        crate::limits::CODEX_CLIENT_VERSION
    )));

    let (url, _rx) = upstream(status(200, "", r#"{"models":[]}"#)).await;
    let client = Client::build(&url, HeaderMap::new()).unwrap();
    let err = probe::list_models(client, Catalog::Codex)
        .await
        .unwrap_err();
    assert!(matches!(err, ProbeError::Unexpected(m) if m.contains("CODEX_CLIENT_VERSION")));

    let (url, _rx) = upstream(status(403, "", "")).await;
    let client = Client::build(&url, HeaderMap::new()).unwrap();
    let err = probe::list_models(client, Catalog::Standard)
        .await
        .unwrap_err();
    assert!(matches!(err, ProbeError::Auth));

    let (url, _rx) = upstream(status(200, "", "<html>")).await;
    let client = Client::build(&url, HeaderMap::new()).unwrap();
    let err = probe::list_models(client, Catalog::Standard)
        .await
        .unwrap_err();
    assert!(matches!(err, ProbeError::Unexpected(m) if m == "返回内容不是模型列表"));
}
