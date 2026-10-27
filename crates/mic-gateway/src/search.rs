//! 会话搜索接口。契约：docs/blueprints/archive/session-search.md。

use std::sync::Arc;

use axum::extract::rejection::QueryRejection;
use axum::extract::{Query, State};
use axum::Json;
use mic_message::MessageId;
use mic_store::{MessageHit, SearchTerms};
use serde::{Deserialize, Serialize};

use crate::api::{cursor, item, Cursor, SessionItem};
use crate::error::ApiError;
use crate::limits::{
    SEARCH_PAGE, SEARCH_QUERY_CHARS, SEARCH_TERMS, SNIPPET_CHARS, SNIPPET_LEAD_CHARS,
};
use crate::service::App;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub(crate) struct SearchQuery {
    q: String,
    before_at: Option<i64>,
    before_id: Option<i64>,
}

#[derive(Serialize)]
pub(crate) struct SearchPage {
    items: Vec<SessionHitView>,
    next: Option<Cursor>,
}

#[derive(Serialize)]
struct SessionHitView {
    session: SessionItem,
    title_hit: bool,
    hits: Vec<HitView>,
    hit_count: u32,
}

#[derive(Serialize)]
struct HitView {
    message_id: MessageId,
    created_at: i64,
    from_user: bool,
    snippet: String,
}

pub(crate) async fn search(
    State(app): State<Arc<App>>,
    query: Result<Query<SearchQuery>, QueryRejection>,
) -> Result<Json<SearchPage>, ApiError> {
    let Query(q) = query.map_err(|e| ApiError::BadRequest(format!("查询参数不合法：{e}")))?;
    if q.q.chars().count() > SEARCH_QUERY_CHARS {
        return Err(ApiError::BadRequest(format!(
            "搜索内容请控制在 {SEARCH_QUERY_CHARS} 字以内"
        )));
    }
    let terms =
        SearchTerms::parse(&q.q).ok_or(ApiError::BadRequest("请输入要搜索的内容".into()))?;
    if terms.terms().len() > SEARCH_TERMS {
        return Err(ApiError::BadRequest(format!(
            "最多同时搜 {SEARCH_TERMS} 个词"
        )));
    }
    let before = cursor(q.before_at, q.before_id)?;
    let page = app
        .kernel
        .search_sessions(terms.clone(), before, SEARCH_PAGE)
        .await?;
    Ok(Json(SearchPage {
        items: page
            .items
            .into_iter()
            .map(|h| SessionHitView {
                session: item(h.summary).expect("search_sessions 只返回 Root 会话"),
                title_hit: h.title_hit,
                hits: h.hits.into_iter().map(|m| hit(m, &terms)).collect(),
                hit_count: h.hit_count,
            })
            .collect(),
        next: page.next.map(Cursor::from),
    }))
}

fn hit(m: MessageHit, terms: &SearchTerms) -> HitView {
    HitView {
        message_id: m.id,
        created_at: m.created_at,
        from_user: m.from_user,
        snippet: snippet(&m.text, terms),
    }
}

/// 空白折叠成单个空格后，从最早的命中词前 `SNIPPET_LEAD_CHARS` 字起截 `SNIPPET_CHARS` 字，截断处补省略号。
fn snippet(text: &str, terms: &SearchTerms) -> String {
    let chars: Vec<char> = text
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .chars()
        .collect();
    let lower: Vec<char> = chars.iter().map(char::to_ascii_lowercase).collect();
    let first = terms
        .terms()
        .iter()
        .filter_map(|t| {
            let t: Vec<char> = t.chars().collect();
            lower.windows(t.len()).position(|w| w == t.as_slice())
        })
        .min()
        .unwrap_or(0);
    let start = first
        .saturating_sub(SNIPPET_LEAD_CHARS)
        .min(chars.len().saturating_sub(SNIPPET_CHARS));
    let end = (start + SNIPPET_CHARS).min(chars.len());
    let mut out = String::new();
    if start > 0 {
        out.push('…');
    }
    out.extend(&chars[start..end]);
    if end < chars.len() {
        out.push('…');
    }
    out
}

#[cfg(test)]
mod tests;
