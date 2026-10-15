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
    assert!(page.runs.is_empty() && page.next_after.is_none());
    let peers = store.call_peers().await.unwrap();
    assert!(peers.models.is_empty() && peers.tools.is_empty());
    let board = store.dashboard(7).await.unwrap();
    assert!(board.days.is_empty() && board.models.is_empty() && board.tools.is_empty());
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
