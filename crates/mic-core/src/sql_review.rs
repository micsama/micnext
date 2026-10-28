//! 开发者 SQL 写入前的把关：试运行取影响行数与 DDL，再由标题模型评估风险。
//! 契约：docs/blueprints/sql-write.md。

use mic_message::{Message, MessageBody, MessageId, ReplyBlock, SessionId};
use mic_store::Store;

use crate::attempt::{stream_attempt, Facts};
use crate::limits::SQL_REVIEW_TIMEOUT;
use crate::provider::Factories;
use crate::run::now_ms;
use crate::{KernelError, ModelRequest, StopReason, ToolChoice};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SqlRisk {
    Low,
    Medium,
    High,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SqlReview {
    pub changes: u64,
    pub tables: Vec<String>,
    /// AI 评估结果；`Err` 是未评估的原因。
    pub verdict: Result<(SqlRisk, String), String>,
}

pub(crate) async fn review(
    store: &Store,
    factories: &Factories,
    sql: String,
) -> Result<SqlReview, KernelError> {
    let dry = store.diagnostic_dry_run(sql.clone()).await?;
    let verdict = match crate::title::build_provider(factories, store.title_model_snapshot().await?)
    {
        Ok(built) => {
            let request = ModelRequest {
                tool_choice: ToolChoice::None,
                system: "你负责在开发者直接修改生产数据库前把关。".into(),
                messages: vec![Message {
                    id: MessageId(0),
                    session_id: SessionId(0),
                    body: MessageBody::HarnessNote {
                        text: prompt(&sql, &dry),
                    },
                    created_at: now_ms(),
                    delivered_at: None,
                }],
                tools: Vec::new(),
                images: Default::default(),
                cache_key: "sql-review".into(),
                plain_after: None,
            };
            let mut facts = Facts::start();
            let attempt = stream_attempt(built.provider.as_ref(), request, &mut facts, |_| {});
            match tokio::time::timeout(SQL_REVIEW_TIMEOUT, attempt).await {
                Err(_) => Err(format!("AI 超过 {} 秒未答复", SQL_REVIEW_TIMEOUT.as_secs())),
                Ok(Err(error)) => {
                    tracing::warn!(purpose = "sql_review", model = built.provider.model(), %error, "sql review failed");
                    Err(error.to_string())
                }
                Ok(Ok(response)) if response.stop != StopReason::EndTurn => {
                    Err("AI 输出未正常完成".into())
                }
                Ok(Ok(response)) => parse(&response.blocks),
            }
        }
        Err(error) => Err(error.to_string()),
    };
    Ok(SqlReview {
        changes: dry.changes,
        tables: dry.tables,
        verdict,
    })
}

fn prompt(sql: &str, dry: &mic_store::DryRun) -> String {
    format!(
        "micnext 是单用户的个人 agent，用 SQLite 存会话、消息、模型调用与设置。开发者要在运行中的主库上直接执行下面这条 SQL，执行前会自动备份整库。\n\n\
         ```sql\n{sql}\n```\n\n\
         试运行（已回滚）：语句直接改动 {changes} 行，不含触发器；涉及表：{tables}。\n\n\
         涉及表的结构：\n```sql\n{ddl}```\n\n\
         请评估这次执行的风险：改动范围是否与语句看起来的意图相符（如缺少 WHERE、条件过宽），\
         是否会破坏表间引用、JSON payload 格式、生成列或应用依赖的其他不变量，是否删改了难以恢复的历史。\n\
         第一行只输出风险等级：低、中、高之一。第二行起用一两句中文说明理由和要留意的地方，不用 Markdown。",
        changes = dry.changes,
        tables = if dry.tables.is_empty() { "无".into() } else { dry.tables.join("、") },
        ddl = dry.ddl,
    )
}

fn parse(blocks: &[ReplyBlock]) -> Result<(SqlRisk, String), String> {
    let text: String = blocks
        .iter()
        .filter_map(|b| match b {
            ReplyBlock::Text { text, .. } => Some(text.as_str()),
            _ => None,
        })
        .collect();
    let mut lines = text.trim().lines().map(str::trim).filter(|l| !l.is_empty());
    let level = lines.next().unwrap_or_default();
    let risk = match level.trim_matches(|c: char| !c.is_alphanumeric()) {
        "低" => SqlRisk::Low,
        "中" => SqlRisk::Medium,
        "高" => SqlRisk::High,
        _ => return Err(format!("AI 输出的风险等级无法识别：{level}")),
    };
    Ok((risk, lines.collect::<Vec<_>>().join("\n")))
}
