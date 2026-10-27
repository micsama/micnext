//! 会话搜索：按最近活跃倒序逐个会话扫标题与对话正文，凑满一页即停。契约：docs/blueprints/archive/session-search.md。

use std::num::NonZeroU32;

use mic_message::{MessageId, SessionId};
use rusqlite::params;

use crate::limits::SEARCH_HITS_SHOWN;
use crate::sessions::{row_summary, summaries};
use crate::store::Store;
use crate::{SessionCursor, SessionSummary, StoreError};

/// 已在边界拆好的检索词：非空，ASCII 已转小写（与 SQLite `lower` 一致）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchTerms(Vec<String>);

impl SearchTerms {
    /// 按空白拆词并去重；没有词时为 `None`。
    pub fn parse(query: &str) -> Option<Self> {
        let mut terms: Vec<String> = Vec::new();
        for term in query.split_whitespace().map(str::to_ascii_lowercase) {
            if !terms.contains(&term) {
                terms.push(term);
            }
        }
        (!terms.is_empty()).then_some(Self(terms))
    }

    pub fn terms(&self) -> &[String] {
        &self.0
    }

    /// `column` 同时含全部词的 SQL 条件，词从 `?{first}` 起依次绑定。
    fn condition(&self, column: &str, first: usize) -> String {
        (0..self.0.len())
            .map(|i| format!("instr(lower({column}), ?{}) > 0", first + i))
            .collect::<Vec<_>>()
            .join(" AND ")
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct SearchPage {
    pub items: Vec<SessionHit>,
    /// 还有没扫到的会话时给出，原样传回接着扫。
    pub next: Option<SessionCursor>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct SessionHit {
    pub summary: SessionSummary,
    pub title_hit: bool,
    /// 最新的若干条命中，新在前。
    pub hits: Vec<MessageHit>,
    pub hit_count: u32,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MessageHit {
    pub id: MessageId,
    pub created_at: i64,
    pub from_user: bool,
    /// 这条消息的全部正文片段，换行连接。
    pub text: String,
}

impl Store {
    /// 在 Root 会话（含已归档）的标题、用户输入文字与回复正文里找同时含全部词的会话。
    pub async fn search_sessions(
        &self,
        terms: SearchTerms,
        before: Option<SessionCursor>,
        limit: NonZeroU32,
    ) -> Result<SearchPage, StoreError> {
        let limit = limit.get() as usize;
        self.call(move |conn| {
            let mut candidates = conn.prepare(
                "SELECT id, act, title FROM (
                   SELECT s.id, s.title,
                     COALESCE((SELECT m.created_at FROM core_messages m
                                WHERE m.session_id = s.id ORDER BY m.id DESC LIMIT 1),
                              s.created_at) AS act
                   FROM core_sessions s
                   WHERE s.kind = 'root'
                     AND EXISTS (SELECT 1 FROM core_messages m
                                  WHERE m.session_id = s.id AND m.kind IN ('UserInput', 'Command')))
                 WHERE ?1 IS NULL OR (act, id) < (?1, ?2)
                 ORDER BY act DESC, id DESC",
            )?;
            // NOTE: 正文依赖 ContentPart / ReplyBlock 的 serde 外部标签形状 {"Text":{"text":..}}。
            let mut messages = conn.prepare(&format!(
                "SELECT m.id, m.created_at, m.kind = 'UserInput',
                        group_concat(json_extract(p.value, '$.Text.text'), char(10)) AS body
                 FROM core_messages m,
                      json_each(m.payload, CASE m.kind WHEN 'UserInput' THEN '$.parts' ELSE '$.blocks' END) p
                 WHERE m.session_id = ?1 AND m.kind IN ('UserInput', 'Reply')
                   AND json_type(p.value, '$.Text') IS NOT NULL
                 GROUP BY m.id HAVING {}
                 ORDER BY m.id DESC",
                terms.condition("body", 2)
            ))?;
            let title_match = format!("SELECT {}", terms.condition("?1", 2));
            let mut summary = conn.prepare(&summaries("s.id = ?1"))?;

            let mut rows = candidates.query(params![
                before.map(|c| c.last_activity_at),
                before.map(|c| c.session_id.0)
            ])?;
            let mut items = Vec::new();
            let mut last = None;
            while let Some(row) = rows.next()? {
                if items.len() == limit {
                    return Ok(SearchPage {
                        items,
                        next: last,
                    });
                }
                let id = SessionId(row.get(0)?);
                last = Some(SessionCursor {
                    last_activity_at: row.get(1)?,
                    session_id: id,
                });
                let title: Option<String> = row.get(2)?;
                let title_hit = match &title {
                    Some(title) => conn.query_row(
                        &title_match,
                        rusqlite::params_from_iter(
                            std::iter::once(title).chain(terms.terms()),
                        ),
                        |r| r.get(0),
                    )?,
                    None => false,
                };
                let mut hits = messages
                    .query_map(
                        rusqlite::params_from_iter(
                            std::iter::once(&id.0 as &dyn rusqlite::ToSql)
                                .chain(terms.terms().iter().map(|t| t as &dyn rusqlite::ToSql)),
                        ),
                        |r| {
                            Ok(MessageHit {
                                id: MessageId(r.get(0)?),
                                created_at: r.get(1)?,
                                from_user: r.get(2)?,
                                text: r.get(3)?,
                            })
                        },
                    )?
                    .collect::<rusqlite::Result<Vec<_>>>()?;
                if !title_hit && hits.is_empty() {
                    continue;
                }
                let hit_count = hits.len() as u32;
                hits.truncate(SEARCH_HITS_SHOWN);
                items.push(SessionHit {
                    summary: summary.query_row([id.0], row_summary)?,
                    title_hit,
                    hits,
                    hit_count,
                });
            }
            Ok(SearchPage { items, next: None })
        })
        .await
    }
}

#[cfg(test)]
mod tests;
