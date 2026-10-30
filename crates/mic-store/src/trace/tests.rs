use mic_message::SessionId;

use super::*;

#[test]
fn queries_run_on_an_empty_database() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(empty_database());
}

async fn empty_database() {
    let store = Store::open_in_memory(&[]).await.unwrap();
    assert!(store.trace_sessions().await.unwrap().is_empty());
    let page = store.trace_runs(SessionId(1), None, 5).await.unwrap();
    assert!(page.runs.is_empty() && page.next_before.is_none());
    let peers = store.call_peers().await.unwrap();
    assert!(peers.models.is_empty() && peers.tools.is_empty());
    let board = store.dashboard(7).await.unwrap();
    assert!(board.days.is_empty() && board.models.is_empty() && board.tools.is_empty());
    let usage = store.session_usage(SessionId(1)).await.unwrap();
    assert_eq!((usage.own, usage.tree), (UsageTotal::default(), None));
}

#[test]
fn cost_is_unknown_without_a_basis() {
    let usage = Usage {
        input_tokens: 1_000_000,
        output_tokens: 1_000_000,
        cache_read_tokens: Some(0),
        cache_write_tokens: Some(0),
        reasoning_tokens: None,
    };
    let price = ModelPrice {
        input: Some(1.0),
        output: Some(2.0),
        ..Default::default()
    };
    assert_eq!(call_cost(Some(&usage), &price), Some(3.0));
    let cached = Usage {
        cache_read_tokens: Some(500_000),
        ..usage
    };
    assert_eq!(call_cost(Some(&cached), &price), None);
    assert_eq!(call_cost(None, &price), None);
    let written = Usage {
        cache_write_tokens: Some(500_000),
        ..usage
    };
    assert_eq!(call_cost(Some(&written), &price), Some(3.0));
    let unreported = Usage {
        cache_write_tokens: None,
        ..usage
    };
    assert_eq!(call_cost(Some(&unreported), &price), Some(3.0));
}

#[test]
fn run_stats_follow_the_trace_rules() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(run_stats_fixture());
}

async fn run_stats_fixture() {
    use crate::{CredentialWrite, EndpointWrite, ModelWrite, NewSession, SessionKind, ToolScope};
    use mic_message::{ContentPart, ReplyBlock};

    let store = Store::open_in_memory(&[]).await.unwrap();
    let person = store.ensure_person("test", 1).await.unwrap();
    let endpoint = store
        .create_endpoint(
            EndpointWrite {
                name: "e".into(),
                kind: "openai".into(),
                config_json: "{}".into(),
                credential: CredentialWrite::Clear,
            },
            1,
        )
        .await
        .unwrap();
    let model = store
        .create_model(
            ModelWrite {
                starred: false,
                endpoint_id: endpoint,
                name: "m".into(),
                config_json: "{}".into(),
                context_window: None,
                images: true,
                price: ModelPrice {
                    input: Some(1.0),
                    cache_read: Some(0.5),
                    cache_write: None,
                    output: Some(2.0),
                },
            },
            1,
        )
        .await
        .unwrap();
    let session = store
        .create_session(NewSession {
            kind: SessionKind::Root {
                channel: "web".into(),
                chat: "c".into(),
            },
            parent_session_id: None,
            delivery_target: None,
            pwd: "/tmp".into(),
            tool_scope: ToolScope::All,
            created_at: 0,
            persona: None,
            model: None,
            reasoning_effort: None,
        })
        .await
        .unwrap()
        .id;
    let reply = MessageBody::Reply {
        stats: mic_message::ReplyStats {
            model_ms: 0,
            first_output_ms: None,
            usage: None,
        },
        model: "m".into(),
        blocks: vec![ReplyBlock::Text {
            text: "ok".into(),
            phase: None,
        }],
    };
    let tool = |started_at| MessageBody::ToolResult {
        call: mic_message::ToolCallRef {
            reply: mic_message::MessageId(2),
            index: 0,
        },
        tool_name: "shell".into(),
        tool_call_id: "t".into(),
        outcome: ToolResultOutcome::Terminal(ExecOutcome::Completed { output: vec![] }),
        started_at: Some(started_at),
    };
    let input = MessageBody::UserInput {
        person,
        parts: vec![ContentPart::Text { text: "hi".into() }],
    };
    let model_id = model.0;
    store
        .call(move |conn| {
            let run = |id: i64, state: &str, finished: Option<i64>| {
                conn.execute(
                    "INSERT INTO core_runs (id, session_id, state, created_at, finished_at) VALUES (?1, ?2, ?3, 0, ?4)",
                    params![id, session.0, state, finished],
                )
            };
            run(1, "completed", Some(10_000))?;
            run(2, "completed", Some(1_000))?;
            run(3, "executing", None)?;
            run(4, "stopped", Some(900))?;
            // (id, run, model_id, error, stop, started, first, finished, input, output, cache)
            let calls = [
                (1, 1, None, Some("boom"), None, 0, None, 100, None, None, None),
                (2, 1, None, None, None, 1_100, Some(1_300), 2_100, Some(100), Some(40), Some(50)),
                (3, 1, None, None, None, 4_200, Some(4_400), 5_200, Some(200), Some(60), None),
                (4, 2, Some(model_id), None, None, 0, Some(100), 500, Some(1_000_000), Some(1_000_000), Some(0)),
                (5, 4, Some(model_id), Some("net"), None, 0, Some(100), 300, Some(1_000_000), Some(1_000_000), Some(0)),
                (6, 4, Some(model_id), None, Some("stopped"), 400, Some(500), 900, Some(1_000), Some(10), None),
            ];
            for (id, run, model, error, stop, started, first, finished, i, o, c) in calls {
                conn.execute(
                    "INSERT INTO core_model_calls (id, session_id, run_id, purpose, model, model_id, error, stop,
                       input_tokens, output_tokens, cache_read_tokens, started_at, first_output_at, finished_at)
                     VALUES (?1, ?2, ?3, 'conversation', 'm', ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12)",
                    params![id, session.0, run, model, error, stop, i, o, c, started, first, finished],
                )?;
            }
            // (id, run, call, body, created_at)
            let messages = [
                (1, 1, None, &input, 0),
                (2, 1, Some(2), &reply, 2_100),
                (3, 1, None, &tool(2_100), 3_100),
                (4, 1, None, &tool(2_100), 4_100),
                (5, 1, Some(3), &reply, 5_200),
                (6, 2, None, &input, 0),
                (7, 2, Some(4), &reply, 500),
                (8, 3, None, &input, 0),
            ];
            for (id, run, call, body, at) in messages {
                conn.execute(
                    "INSERT INTO core_messages (id, session_id, run_id, model_call_id, payload, created_at)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
                    params![id, session.0, run, call, row::payload(body).unwrap(), at],
                )?;
            }
            Ok(())
        })
        .await
        .unwrap();

    let trace = store.trace_runs(session, None, 5).await.unwrap();
    let all = &trace.runs;
    let s = &all[0].stats;
    assert_eq!((s.steps, s.requests, s.tools), (2, 3, 2), "重试合并为一步");
    assert_eq!(s.model_ms, 2_100);
    assert_eq!(s.tool_ms, 2_000, "并行工具取墙钟");
    assert_eq!(all[1].stats.total_ms, Some(1_000));

    // 已知部分一律计入，不论成败；缓存未上报只是不参与合计。
    let usage = store.session_usage(session).await.unwrap().own;
    let want = UsageTotal {
        calls: 6,
        input_tokens: 2_001_300,
        output_tokens: 2_000_110,
        cache_read_tokens: 50,
        cache_write_tokens: 0,
        reasoning_tokens: None,
        unknown_usage: 1,
        cost: 6.0,
        unknown_cost: 4,
    };
    assert_eq!(usage, want);
    let stat = store
        .call(move |conn| {
            let mut stat = CallStat::default();
            for c in &load_usage(conn, "c.session_id = ?1", session.0)? {
                stat.add(c);
            }
            Ok(stat)
        })
        .await
        .unwrap();
    assert_eq!((stat.failed_calls, stat.stopped_calls), (2, 1));
    assert_eq!(mean(&stat.ok_ms), Some(833), "平均耗时只取成功调用");
    assert_eq!(stat.usage, want);
}

/// 真实库副本压测：`MICNEXT_PERF_DB` 指向已转换到当前版本、`key_cipher` 已清空的副本。
#[test]
#[ignore]
fn real_database_reads_stay_bounded() {
    tokio::runtime::Builder::new_current_thread()
        .build()
        .unwrap()
        .block_on(real_database());
}

async fn real_database() {
    use std::time::Instant;
    let path = std::path::PathBuf::from(std::env::var("MICNEXT_PERF_DB").unwrap());
    let key = crate::SecretKeyFile(path.with_extension("key"));
    let store = Store::open(&path, &[], key).await.unwrap();
    let (session, schedule) = store
        .call(|conn| {
            Ok(conn.query_row(
                "SELECT m.session_id, (SELECT id FROM core_schedules s WHERE s.session_id = m.session_id LIMIT 1)
                 FROM core_messages m GROUP BY m.session_id ORDER BY count(*) DESC LIMIT 1",
                [],
                |r| Ok((SessionId(r.get(0)?), r.get::<_, Option<i64>>(1)?)),
            )?)
        })
        .await
        .unwrap();
    macro_rules! timed {
        ($name:literal, $e:expr) => {{
            let at = Instant::now();
            $e.await.unwrap();
            let ms = at.elapsed().as_millis();
            println!("{:<16} {ms:>5}ms", $name);
            assert!(ms < 300, "{} 用时 {ms}ms", $name);
        }};
    }
    timed!("context_window", store.context_window(session));
    timed!("context_usage", store.context_usage(session));
    timed!("last_message_id", store.last_message_id(session));
    timed!("trace_sessions", store.trace_sessions());
    timed!("trace_runs", store.trace_runs(session, None, 5));
    timed!("session_usage", store.session_usage(session));
    if let Some(id) = schedule {
        timed!(
            "schedule_usage",
            store.schedule_usage(crate::ScheduleId(id))
        );
    }
    timed!("dashboard_30", store.dashboard(30));
    timed!("dashboard_366", store.dashboard(366));
}

/// 会写入测试会话；环境变量必须指向已转换、清空凭据的可丢弃副本。
#[test]
#[ignore]
fn real_database_mixed_load() {
    use std::sync::{Arc, Barrier};
    use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

    fn runtime() -> tokio::runtime::Runtime {
        tokio::runtime::Builder::new_current_thread()
            .build()
            .unwrap()
    }
    let path = std::path::PathBuf::from(std::env::var("MICNEXT_PERF_WRITABLE_DB").unwrap());
    let rt = runtime();
    let store = rt
        .block_on(Store::open(
            &path,
            &[],
            crate::SecretKeyFile(path.with_extension("key")),
        ))
        .unwrap();
    let session = rt.block_on(store.call(|conn| {
        Ok(SessionId(conn.query_row(
            "SELECT session_id FROM core_messages GROUP BY session_id ORDER BY count(*) DESC LIMIT 1",
            [], |r| r.get(0),
        )?))
    })).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let scratch = rt
        .block_on(store.create_session(crate::NewSession {
            kind: crate::SessionKind::Root {
                channel: "perf".into(),
                chat: now.to_string(),
            },
            parent_session_id: None,
            delivery_target: None,
            pwd: "/tmp".into(),
            tool_scope: crate::ToolScope::All,
            created_at: now,
            persona: None,
            model: None,
            reasoning_effort: None,
        }))
        .unwrap()
        .id;
    let mut reports = Vec::new();
    let mut writes = 0;
    for (phase, operations) in [
        ("baseline", vec!["settings", "append"]),
        (
            "mixed",
            vec!["settings", "append", "dashboard", "trace_runs", "search"],
        ),
        ("recovery", vec!["settings", "append"]),
    ] {
        let barrier = Arc::new(Barrier::new(operations.len()));
        let results = std::thread::scope(|scope| {
            let handles: Vec<_> = operations
                .into_iter()
                .map(|operation| {
                    let store = store.clone();
                    let barrier = barrier.clone();
                    scope.spawn(move || {
                        let rt = runtime();
                        let mut samples = Vec::new();
                        let mut errors = 0;
                        barrier.wait();
                        let start = Instant::now();
                        while start.elapsed() < Duration::from_secs(3) {
                            let at = Instant::now();
                            let result = rt.block_on(async {
                                match operation {
                                    "settings" => store.settings().await.map(|_| ()),
                                    "append" => store
                                        .append(
                                            scratch,
                                            None,
                                            MessageBody::Notification {
                                                source: "perf".into(),
                                                text: "storage mixed-load probe".into(),
                                                files: Vec::new(),
                                            },
                                            now + start.elapsed().as_millis() as i64,
                                        )
                                        .await
                                        .map(|_| ()),
                                    "dashboard" => store.dashboard(366).await.map(|_| ()),
                                    "trace_runs" => {
                                        store.trace_runs(session, None, 20).await.map(|_| ())
                                    }
                                    "search" => store
                                        .search_sessions(
                                            crate::SearchTerms::parse(
                                                "micnext_perf_absent_827ea91d",
                                            )
                                            .unwrap(),
                                            None,
                                            std::num::NonZeroU32::new(30).unwrap(),
                                        )
                                        .await
                                        .map(|_| ()),
                                    _ => unreachable!(),
                                }
                            });
                            samples.push(at.elapsed().as_micros() as u64);
                            errors += usize::from(result.is_err());
                            std::thread::sleep(
                                Duration::from_millis(10).saturating_sub(at.elapsed()),
                            );
                        }
                        (operation, samples, errors, start.elapsed().as_secs_f64())
                    })
                })
                .collect();
            handles
                .into_iter()
                .map(|h| h.join().unwrap())
                .collect::<Vec<_>>()
        });
        for (operation, samples, errors, seconds) in results {
            assert!(!samples.is_empty());
            let mut sorted = samples.clone();
            sorted.sort_unstable();
            let percentile =
                |p: usize| sorted[(sorted.len() * p).div_ceil(100) - 1] as f64 / 1000.0;
            let report = serde_json::json!({
                "phase": phase, "operation": operation, "samples": samples.len(),
                "errors": errors, "over_300ms": samples.iter().filter(|&&v| v >= 300_000).count(),
                "p50_ms": percentile(50), "p95_ms": percentile(95), "p99_ms": percentile(99),
                "max_ms": sorted.last().unwrap().to_owned() as f64 / 1000.0,
                "seconds": seconds, "requests_per_second": samples.len() as f64 / seconds,
            });
            println!("{report}");
            reports.push(serde_json::json!({"summary": report, "samples_us": samples}));
            if operation == "append" {
                writes += sorted.len() - errors;
            }
        }
    }
    rt.block_on(store.call(move |conn| {
        let count: i64 = conn.query_row(
            "SELECT count(*) FROM core_messages WHERE session_id = ?1",
            [scratch.0],
            |r| r.get(0),
        )?;
        assert_eq!(count, writes as i64, "已成功返回的消息写入必须全部存在");
        let check: String = conn.query_row("PRAGMA quick_check", [], |r| r.get(0))?;
        assert_eq!(check, "ok");
        assert!(conn
            .prepare("PRAGMA foreign_key_check")?
            .query([])?
            .next()?
            .is_none());
        Ok(())
    }))
    .unwrap();
    std::fs::write(
        path.with_extension("mixed-results.json"),
        serde_json::to_vec_pretty(&reports).unwrap(),
    )
    .unwrap();
    assert!(
        reports.iter().all(|r| r["summary"]["errors"] == 0),
        "部分请求失败，见统计结果"
    );
}
