use mic_message::{ExecFailureKind, PersonId};

use super::*;

fn message(id: i64, body: MessageBody) -> Message {
    Message {
        id: MessageId(id),
        session_id: SessionId(1),
        body,
        created_at: 0,
        delivered_at: None,
    }
}

fn reply(id: i64) -> Message {
    message(
        id,
        MessageBody::Reply {
            stats: mic_message::ReplyStats {
                model_ms: 0,
                first_output_ms: None,
                usage: None,
            },
            model: "m".into(),
            blocks: vec![ReplyBlock::ToolCall {
                id: id.to_string(),
                name: "read".into(),
                args: serde_json::json!({}),
            }],
        },
    )
}

fn result(id: i64, outcome: ExecOutcome) -> Message {
    message(
        id,
        MessageBody::ToolResult {
            tool_name: "read".into(),
            tool_call_id: (id - 1).to_string(),
            outcome: ToolResultOutcome::Terminal(outcome),
            started_at: None,
        },
    )
}

fn output(chars: usize) -> ExecOutcome {
    ExecOutcome::Completed {
        output: vec![ContentPart::Text {
            text: "x".repeat(chars),
        }],
    }
}

fn input(id: i64) -> Message {
    message(
        id,
        MessageBody::UserInput {
            person: PersonId(1),
            parts: vec![ContentPart::Text { text: "hi".into() }],
        },
    )
}

#[test]
fn newest_step_is_kept_even_over_budget_and_earlier_steps_only_whole() {
    let history = vec![
        input(1),
        reply(2),
        result(3, output(300)),
        input(4),
        reply(5),
        result(6, output(30_000)),
    ];
    let r = retain(&history, &[], 1);
    assert_eq!(r.keep_from, Some(MessageId(5)));
    assert_eq!(r.kept_replies, 1);
    assert!(r.elided.is_empty(), "最新一步不替换");
    assert_eq!(r.quoted, [MessageId(1), MessageId(4)]);

    let history = vec![
        input(1),
        reply(2),
        result(3, output(30_000)),
        reply(4),
        result(
            5,
            ExecOutcome::Failed {
                kind: ExecFailureKind::Input,
                message: "e".repeat(30_000),
            },
        ),
        reply(6),
        result(7, output(300)),
    ];
    // 第一步替换后很小，整步放得下；失败结果不替换，第二步放不下。
    let r = retain(&history, &[], 10);
    assert_eq!(r.keep_from, Some(MessageId(6)));
    let r = retain(&history, &[], 100);
    assert_eq!(r.keep_from, Some(MessageId(2)));
    assert_eq!(r.elided, [MessageId(3)]);
    assert_eq!(
        r.elide.iter().map(|m| m.id).collect::<Vec<_>>(),
        [MessageId(3)]
    );
    assert_eq!(r.quoted, [MessageId(1)]);
}

#[test]
fn required_files_only_read_from_the_trailing_block() {
    let response = |text: &str| ModelResponse {
        blocks: vec![ReplyBlock::Text {
            text: text.into(),
            phase: None,
        }],
        stop: StopReason::EndTurn,
    };
    let text = "提到 `<required_files>` 块。\n## 纪要\n正文\n<required_files>\na.rs\nb.rs\na.rs\n</required_files>\n";
    let (summary, paths) = parse_summary(&response(text)).unwrap();
    assert_eq!(summary, "提到 `<required_files>` 块。\n## 纪要\n正文");
    assert_eq!(paths, ["a.rs", "b.rs"]);

    let (summary, paths) = parse_summary(&response("正文里提到 <required_files> 而已")).unwrap();
    assert_eq!(summary, "正文里提到 <required_files> 而已");
    assert!(paths.is_empty());
}
