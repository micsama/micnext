use mic_message::{Message, MessageBody, PersonId, SessionId};
use rusqlite::{params, OptionalExtension, Row};
use serde::Serialize;

use crate::messages::insert_message;
use crate::store::Store;
use crate::{RunId, StoreError};

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize)]
#[serde(transparent)]
pub struct JobId(pub i64);

/// 后台工具任务的状态；`Running` 之外均为终态。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum JobState {
    Running,
    Completed,
    Failed,
    TimedOut,
    Cancelled,
    Interrupted,
}

impl JobState {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::TimedOut => "timed_out",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }

    fn parse(s: &str) -> Self {
        match s {
            "running" => Self::Running,
            "completed" => Self::Completed,
            "failed" => Self::Failed,
            "timed_out" => Self::TimedOut,
            "cancelled" => Self::Cancelled,
            "interrupted" => Self::Interrupted,
            other => unreachable!("core_jobs.state CHECK 约束外的值：{other}"),
        }
    }
}

/// 一次后台工具执行。agent job 随 P2 加入。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: JobId,
    pub owner_session_id: SessionId,
    pub person: PersonId,
    pub origin_run_id: RunId,
    pub origin_tool_call_id: String,
    pub tool_name: String,
    pub description: String,
    pub created_at: i64,
    /// `None` = 不限时。
    pub timeout_ms: Option<u64>,
    pub state: JobState,
    pub finished_at: Option<i64>,
}

/// 发起人取 `origin_run_id` 的首条输入。
pub struct NewToolJob {
    pub owner_session_id: SessionId,
    pub origin_run_id: RunId,
    pub origin_tool_call_id: String,
    pub tool_name: String,
    pub description: String,
    pub timeout_ms: Option<u64>,
    pub created_at: i64,
}

const JOB_COLS: &str = "id, owner_session_id, person_id, origin_run_id, origin_tool_call_id,
    tool_name, description, created_at, timeout_ms, state, finished_at";

fn job(row: &Row<'_>) -> rusqlite::Result<Job> {
    Ok(Job {
        id: JobId(row.get(0)?),
        owner_session_id: SessionId(row.get(1)?),
        person: PersonId(row.get(2)?),
        origin_run_id: RunId(row.get(3)?),
        origin_tool_call_id: row.get(4)?,
        tool_name: row.get(5)?,
        description: row.get(6)?,
        created_at: row.get(7)?,
        timeout_ms: row.get::<_, Option<i64>>(8)?.map(|ms| ms as u64),
        state: JobState::parse(&row.get::<_, String>(9)?),
        finished_at: row.get(10)?,
    })
}

/// `?1` 及其全部后代会话。
const SUBTREE: &str = "WITH RECURSIVE subtree(id) AS (
    SELECT ?1 UNION ALL
    SELECT s.id FROM core_sessions s JOIN subtree t ON s.parent_session_id = t.id
)";

impl Store {
    /// 登记即提交：返回后任务已存在，`(origin_run_id, origin_tool_call_id)` 唯一。
    pub async fn create_tool_job(&self, new: NewToolJob) -> Result<Job, StoreError> {
        self.call(move |conn| {
            conn.execute(
                "INSERT INTO core_jobs (kind, owner_session_id, person_id, origin_run_id,
                   origin_tool_call_id, tool_name, description, created_at, timeout_ms, state)
                 VALUES ('tool', ?1,
                   (SELECT person_id FROM core_messages
                    WHERE run_id = ?2 AND kind IN ('UserInput', 'Completion', 'Command')
                    ORDER BY id LIMIT 1),
                   ?2, ?3, ?4, ?5, ?6, ?7, 'running')",
                params![
                    new.owner_session_id.0,
                    new.origin_run_id.0,
                    new.origin_tool_call_id,
                    new.tool_name,
                    new.description,
                    new.created_at,
                    new.timeout_ms.map(|ms| ms as i64),
                ],
            )?;
            Ok(conn.query_row(
                &format!("SELECT {JOB_COLS} FROM core_jobs WHERE id = ?1"),
                [conn.last_insert_rowid()],
                job,
            )?)
        })
        .await
    }

    /// 仅当仍为 `running` 时转为终态 `state`，同事务写入回注（Completion，写到 owner）并记下其 id。
    /// 返回 `None` 表示已被别处收尾；`Some(report)` 为本次写入的回注。
    pub async fn finish_job(
        &self,
        id: JobId,
        state: JobState,
        report: Option<MessageBody>,
        at: i64,
    ) -> Result<Option<Option<Message>>, StoreError> {
        assert!(state != JobState::Running, "收尾必须是终态");
        assert!(
            report
                .as_ref()
                .is_none_or(|b| matches!(b, MessageBody::Completion { .. })),
            "回注只能是 Completion"
        );
        self.call(move |conn| {
            let tx = conn.transaction()?;
            let changed = tx.execute(
                "UPDATE core_jobs SET state = ?2, finished_at = ?3
                 WHERE id = ?1 AND state = 'running'",
                params![id.0, state.as_str(), at],
            )?;
            if changed == 0 {
                return Ok(None);
            }
            let report = match report {
                Some(body) => {
                    let owner: i64 = tx.query_row(
                        "SELECT owner_session_id FROM core_jobs WHERE id = ?1",
                        [id.0],
                        |r| r.get(0),
                    )?;
                    let session_id = SessionId(owner);
                    let message_id = insert_message(&tx, session_id, None, &body, at)?;
                    tx.execute(
                        "UPDATE core_jobs SET report_message_id = ?2 WHERE id = ?1",
                        params![id.0, message_id.0],
                    )?;
                    Some(Message {
                        id: message_id,
                        session_id,
                        body,
                        created_at: at,
                        delivered_at: None,
                    })
                }
                None => None,
            };
            tx.commit()?;
            Ok(Some(report))
        })
        .await
    }

    pub async fn job(&self, id: JobId) -> Result<Option<Job>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {JOB_COLS} FROM core_jobs WHERE id = ?1"),
                    [id.0],
                    job,
                )
                .optional()?)
        })
        .await
    }

    /// 某 run 中某工具调用登记的任务；恢复时据此把悬空调用闭合为 `Dispatched`。
    pub async fn job_by_origin(
        &self,
        run: RunId,
        tool_call_id: &str,
    ) -> Result<Option<JobId>, StoreError> {
        let tool_call_id = tool_call_id.to_owned();
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM core_jobs WHERE origin_run_id = ?1 AND origin_tool_call_id = ?2",
                    params![run.0, tool_call_id],
                    |r| Ok(JobId(r.get(0)?)),
                )
                .optional()?)
        })
        .await
    }

    /// 全部 `running` 的任务，按 id 升序；启动时即上次遗留的。
    pub async fn running_jobs(&self) -> Result<Vec<Job>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {JOB_COLS} FROM core_jobs WHERE state = 'running' ORDER BY id"
                ))?
                .query_map([], job)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// `session` 及其后代名下的任务：运行中在前，其余按最近活动、id 倒序；
    /// `active_since` 给出时略去在此之前结束的任务。
    pub async fn subtree_jobs(
        &self,
        session: SessionId,
        active_since: Option<i64>,
    ) -> Result<Vec<Job>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "{SUBTREE}
                     SELECT {JOB_COLS} FROM core_jobs
                     WHERE owner_session_id IN (SELECT id FROM subtree)
                       AND (?2 IS NULL OR state = 'running' OR finished_at >= ?2)
                     ORDER BY state = 'running' DESC, COALESCE(finished_at, created_at) DESC, id DESC"
                ))?
                .query_map(params![session.0, active_since], job)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// `session` 是否为 `ancestor` 本身或其后代。
    pub async fn in_subtree(
        &self,
        ancestor: SessionId,
        session: SessionId,
    ) -> Result<bool, StoreError> {
        self.call(move |conn| {
            Ok(conn.query_row(
                &format!("{SUBTREE} SELECT EXISTS(SELECT 1 FROM subtree WHERE id = ?2)"),
                params![ancestor.0, session.0],
                |r| r.get(0),
            )?)
        })
        .await
    }
}
