//! DeepSeek 联网搜索：不取正文，不重试。契约：docs/blueprints/web-search.md。

use mic_core::{SearchError, SearchHit};
use reqwest::header::HeaderMap;
use serde::{Deserialize, Serialize};

use crate::client::{Client, Failure};
use crate::error;
use crate::limits::{SEARCH_COUNT, SEARCH_TIMEOUT};

#[derive(Serialize)]
struct Request<'a> {
    query: &'a str,
    count: u32,
}

#[derive(Deserialize)]
struct Response {
    results: Vec<Item>,
}

#[derive(Deserialize)]
struct Item {
    title: String,
    url: String,
    site_name: Option<String>,
    date: Option<String>,
    /// 面向模型的长摘要；缺失时退回短摘要。
    summary: Option<String>,
    snippet: Option<String>,
}

pub(crate) async fn search(client: Client, query: String) -> Result<Vec<SearchHit>, SearchError> {
    let request = async {
        let body = client
            .post(
                "/v1/tools/search",
                &Request {
                    query: &query,
                    count: SEARCH_COUNT,
                },
                HeaderMap::new(),
            )
            .await
            .map_err(failure)?
            .bytes()
            .await
            .map_err(|e| failure(Failure::Network(e)))?;
        serde_json::from_slice::<Response>(&body)
            .map_err(|e| SearchError(format!("unexpected response: {e}")))
    };
    let response = tokio::time::timeout(SEARCH_TIMEOUT, request)
        .await
        .map_err(|_| SearchError(format!("timed out after {}s", SEARCH_TIMEOUT.as_secs())))??;
    Ok(response
        .results
        .into_iter()
        .map(|item| SearchHit {
            title: item.title,
            url: item.url,
            site_name: item.site_name.filter(|s| !s.is_empty()),
            date: item.date.filter(|s| !s.is_empty()),
            summary: item.summary.or(item.snippet).unwrap_or_default(),
        })
        .collect())
}

fn failure(e: Failure) -> SearchError {
    SearchError(match e {
        Failure::Http(f) => match f.error {
            Some(e) => format!("HTTP {}: {}", f.status.as_u16(), e.message),
            None => format!("HTTP {}", f.status.as_u16()),
        },
        Failure::Network(e) => error::chain(&e.without_url()),
    })
}
