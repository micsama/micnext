use std::collections::HashSet;

use mic_message::{ContentPart, Message, MessageBody, PersonId, SessionId, ToolCallRef};
use rusqlite::{params, OptionalExtension, Row, Transaction};
use serde::Serialize;

use crate::messages::insert_message;
use crate::row::{self, input_kinds, HELD_INPUT, MESSAGE_COLS, MESSAGE_FROM, UNCLAIMED};
use crate::sessions::insert_session;
use crate::store::Store;
use crate::{NewSession, RunId, RunState, Session, SessionKind, StoreError, ToolScope};

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

/// 一次后台工具执行，或一个子 agent（子 session）的管理身份。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Job {
    pub id: JobId,
    pub owner_session_id: SessionId,
    pub person: PersonId,
    pub origin_run_id: RunId,
    pub origin_call: ToolCallRef,
    pub tool_name: String,
    pub description: String,
    pub created_at: i64,
    pub kind: JobKind,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum JobKind {
    Tool {
        /// `None` = 不限时。
        timeout_ms: Option<u64>,
        state: JobState,
        finished_at: Option<i64>,
    },
    /// 执行状态看子 session 的 run。
    Agent { child_session_id: SessionId },
}

impl Job {
    /// tool job 的状态；agent job 为 `None`。
    pub fn tool_state(&self) -> Option<JobState> {
        match self.kind {
            JobKind::Tool { state, .. } => Some(state),
            JobKind::Agent { .. } => None,
        }
    }
}

/// agent job 的执行概况，由 run / 消息 / 任务事实查出。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AgentActivity {
    pub running: bool,
    /// 最近一个已结束 run 的状态。
    pub last_run: Option<RunState>,
    pub last_active_at: i64,
}

/// 发起人取 `origin_run_id` 的首条输入。
pub struct NewToolJob {
    pub owner_session_id: SessionId,
    pub origin_run_id: RunId,
    pub origin_call: ToolCallRef,
    pub tool_name: String,
    pub description: String,
    pub timeout_ms: Option<u64>,
    pub created_at: i64,
}

/// 子 session 继承父的工作目录、人设、模型与思考档位；发起人取 `origin_run_id` 的首条输入。
pub struct NewAgentJob {
    pub owner_session_id: SessionId,
    pub origin_run_id: RunId,
    pub origin_call: ToolCallRef,
    pub tool_name: String,
    pub description: String,
    pub task: String,
    pub tool_scope: ToolScope,
    pub created_at: i64,
}

const JOB_COLS: &str = "j.id, j.owner_session_id, j.person_id, j.origin_run_id, j.origin_reply_id,
    j.tool_name, j.description, j.created_at, j.kind, j.timeout_ms, j.state, j.finished_at,
    j.child_session_id, j.origin_call_index";

fn job(row: &Row<'_>) -> rusqlite::Result<Job> {
    let kind = match row.get::<_, String>(8)?.as_str() {
        "tool" => JobKind::Tool {
            timeout_ms: row.get::<_, Option<i64>>(9)?.map(|ms| ms as u64),
            state: JobState::parse(&row.get::<_, String>(10)?),
            finished_at: row.get(11)?,
        },
        "agent" => JobKind::Agent {
            child_session_id: SessionId(row.get(12)?),
        },
        other => unreachable!("core_jobs.kind CHECK 约束外的值：{other}"),
    };
    Ok(Job {
        id: JobId(row.get(0)?),
        owner_session_id: SessionId(row.get(1)?),
        person: PersonId(row.get(2)?),
        origin_run_id: RunId(row.get(3)?),
        origin_call: ToolCallRef {
            reply: mic_message::MessageId(row.get(4)?),
            index: row.get(13)?,
        },
        tool_name: row.get(5)?,
        description: row.get(6)?,
        created_at: row.get(7)?,
        kind,
    })
}

/// `?1` 及其全部后代会话。
const SUBTREE: &str = "WITH RECURSIVE subtree(id) AS (
    SELECT ?1 UNION ALL
    SELECT s.id FROM core_sessions s JOIN subtree t ON s.parent_session_id = t.id
)";

/// run 首条输入的发起人。
pub(crate) fn run_person(tx: &Transaction<'_>, run: RunId) -> rusqlite::Result<PersonId> {
    tx.query_row(
        concat!(
            "SELECT person_id FROM core_messages WHERE run_id = ?1 AND kind IN ",
            input_kinds!(),
            " ORDER BY id LIMIT 1"
        ),
        [run.0],
        |r| Ok(PersonId(r.get(0)?)),
    )
}

fn read_job(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Job> {
    conn.query_row(
        &format!("SELECT {JOB_COLS} FROM core_jobs j WHERE j.id = ?1"),
        [id],
        job,
    )
}

fn task_input(person: PersonId, task: String) -> MessageBody {
    MessageBody::UserInput {
        person,
        parts: vec![ContentPart::Text { text: task }],
    }
}

fn message(
    session_id: SessionId,
    id: mic_message::MessageId,
    body: MessageBody,
    at: i64,
) -> Message {
    Message {
        id,
        session_id,
        body,
        created_at: at,
        delivered_at: None,
    }
}

impl Store {
    /// 登记即提交：返回后任务已存在，`origin_call` 唯一。
    pub async fn create_tool_job(&self, new: NewToolJob) -> Result<Job, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            crate::sessions::ensure_active(&tx, new.owner_session_id)?;
            let person = run_person(&tx, new.origin_run_id)?;
            tx.execute(
                "INSERT INTO core_jobs (kind, owner_session_id, person_id, origin_run_id,
                   origin_reply_id, tool_name, description, created_at, timeout_ms, state, origin_call_index)
                 VALUES ('tool', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, 'running', ?9)",
                params![
                    new.owner_session_id.0,
                    person.0,
                    new.origin_run_id.0,
                    new.origin_call.reply.0,
                    new.tool_name,
                    new.description,
                    new.created_at,
                    new.timeout_ms.map(|ms| ms as i64),
                    new.origin_call.index,
                ],
            )?;
            let job = read_job(&tx, tx.last_insert_rowid())?;
            tx.commit()?;
            Ok(job)
        })
        .await
    }

    /// 同一事务建子 session、agent job 与首条输入（任务说明）；返回后即已提交。
    pub async fn create_agent_job(
        &self,
        new: NewAgentJob,
    ) -> Result<(Job, Session, Message), StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            crate::sessions::ensure_active(&tx, new.owner_session_id)?;
            let person = run_person(&tx, new.origin_run_id)?;
            let (pwd, persona, model, effort) = tx.query_row(
                "SELECT pwd, persona_id, model_id, reasoning_effort FROM core_sessions WHERE id = ?1",
                [new.owner_session_id.0],
                |r| {
                    Ok((
                        r.get::<_, String>(0)?,
                        crate::PersonaId(r.get(1)?),
                        r.get::<_, Option<i64>>(2)?.map(crate::ModelId),
                        r.get::<_, Option<crate::ReasoningEffort>>(3)?,
                    ))
                },
            )?;
            let child = insert_session(
                &tx,
                NewSession {
                    kind: SessionKind::Task,
                    parent_session_id: Some(new.owner_session_id),
                    delivery_target: None,
                    pwd,
                    tool_scope: new.tool_scope,
                    created_at: new.created_at,
                    persona: Some(persona),
                    model,
                    reasoning_effort: effort,
                },
            )?;
            tx.execute(
                "INSERT INTO core_jobs (kind, owner_session_id, person_id, origin_run_id,
                   origin_reply_id, tool_name, description, created_at, child_session_id, origin_call_index)
                 VALUES ('agent', ?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)",
                params![
                    new.owner_session_id.0,
                    person.0,
                    new.origin_run_id.0,
                    new.origin_call.reply.0,
                    new.tool_name,
                    new.description,
                    new.created_at,
                    child.id.0,
                    new.origin_call.index,
                ],
            )?;
            let job = read_job(&tx, tx.last_insert_rowid())?;
            let body = task_input(person, new.task);
            let id = insert_message(&tx, child.id, None, &body, new.created_at)?;
            tx.commit()?;
            let input = message(child.id, id, body, new.created_at);
            Ok((job, child, input))
        })
        .await
    }

    /// 追问：任务说明写成子 session 的输入，发起人取 `origin_run` 的首条输入。
    pub async fn follow_up_agent(
        &self,
        child: SessionId,
        origin_run: RunId,
        task: String,
        at: i64,
    ) -> Result<Message, StoreError> {
        self.call(move |conn| {
            let tx = conn.transaction()?;
            crate::sessions::ensure_active(&tx, child)?;
            let body = task_input(run_person(&tx, origin_run)?, task);
            let id = insert_message(&tx, child, None, &body, at)?;
            tx.commit()?;
            Ok(message(child, id, body, at))
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
                    Some(message(session_id, message_id, body, at))
                }
                None => None,
            };
            tx.commit()?;
            Ok(Some(report))
        })
        .await
    }

    pub async fn job(&self, id: JobId) -> Result<Option<Job>, StoreError> {
        self.call(move |conn| Ok(read_job(conn, id.0).optional()?))
            .await
    }

    /// 以 `child` 为子 session 的 agent job。
    pub async fn agent_job(&self, child: SessionId) -> Result<Option<Job>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    &format!("SELECT {JOB_COLS} FROM core_jobs j WHERE j.child_session_id = ?1"),
                    [child.0],
                    job,
                )
                .optional()?)
        })
        .await
    }

    /// 某 run 中某工具调用登记的任务；恢复时据此把悬空调用闭合为 `Dispatched`。
    pub async fn job_by_origin(&self, call: ToolCallRef) -> Result<Option<JobId>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    "SELECT id FROM core_jobs WHERE origin_reply_id = ?1 AND origin_call_index = ?2",
                    params![call.reply.0, call.index],
                    |r| Ok(JobId(r.get(0)?)),
                )
                .optional()?)
        })
        .await
    }

    /// `running` 的 tool job，按 id 升序；`owner` 给出时只取其名下。启动时不带 `owner` 即上次遗留的。
    pub async fn running_jobs(&self, owner: Option<SessionId>) -> Result<Vec<Job>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .prepare(&format!(
                    "SELECT {JOB_COLS} FROM core_jobs j
                     WHERE j.state = 'running' AND (?1 IS NULL OR j.owner_session_id = ?1)
                     ORDER BY j.id"
                ))?
                .query_map([owner.map(|s| s.0)], job)?
                .collect::<rusqlite::Result<Vec<_>>>()?)
        })
        .await
    }

    /// agent 子 session 最近一次交付（某个子 run 的回注）。
    pub async fn latest_agent_report(
        &self,
        child: SessionId,
    ) -> Result<Option<Message>, StoreError> {
        self.call(move |conn| {
            Ok(conn
                .query_row(
                    &format!(
                        "SELECT {MESSAGE_COLS} FROM {MESSAGE_FROM}
                         WHERE m.id = (SELECT report_message_id FROM core_runs
                                       WHERE session_id = ?1 AND report_message_id IS NOT NULL
                                       ORDER BY id DESC LIMIT 1)"
                    ),
                    [child.0],
                    row::message,
                )
                .optional()?)
        })
        .await
    }

    /// `session` 及其后代名下的任务：活跃在前，其余按最近活动、id 倒序。`active_since` 给出时略去
    /// 此前已结束的 tool job，以及此后无活动、休眠且名下无运行中任务的 agent job。
    pub async fn subtree_jobs(
        &self,
        session: SessionId,
        active_since: Option<i64>,
    ) -> Result<Vec<(Job, Option<AgentActivity>)>, StoreError> {
        self.call(move |conn| {
            let jobs = conn
                .prepare(&format!(
                    "{SUBTREE}
                     SELECT {JOB_COLS} FROM core_jobs j
                     WHERE j.owner_session_id IN (SELECT id FROM subtree)"
                ))?
                .query_map([session.0], job)?
                .collect::<rusqlite::Result<Vec<_>>>()?;
            let busy_owners: HashSet<SessionId> = jobs
                .iter()
                .filter(|j| j.tool_state() == Some(JobState::Running))
                .map(|j| j.owner_session_id)
                .collect();
            let mut listed = Vec::with_capacity(jobs.len());
            for j in jobs {
                let activity = match j.kind {
                    JobKind::Tool { .. } => None,
                    JobKind::Agent { child_session_id } => {
                        Some(agent_activity(conn, child_session_id, j.created_at)?)
                    }
                };
                let (active, last) = match (&j.kind, &activity) {
                    (
                        JobKind::Tool {
                            state, finished_at, ..
                        },
                        _,
                    ) => (
                        *state == JobState::Running,
                        finished_at.unwrap_or(j.created_at),
                    ),
                    (JobKind::Agent { child_session_id }, Some(a)) => (
                        a.running || busy_owners.contains(child_session_id),
                        a.last_active_at,
                    ),
                    (JobKind::Agent { .. }, None) => unreachable!("agent 必有执行概况"),
                };
                if active || active_since.is_none_or(|since| last >= since) {
                    listed.push((active, last, j, activity));
                }
            }
            listed.sort_by_key(|&(active, last, ref j, _)| std::cmp::Reverse((active, last, j.id)));
            Ok(listed.into_iter().map(|(_, _, j, a)| (j, a)).collect())
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

    /// 把会话已排队、未认领的输入登记为 held（停 agent 后不再执行）；返回登记条数。
    pub async fn hold_session_inputs(
        &self,
        session: SessionId,
        at: i64,
    ) -> Result<usize, StoreError> {
        self.call(move |conn| {
            Ok(conn.execute(
                &format!(
                    "INSERT INTO core_input_holds (message_id, held_at)
                     SELECT m.id, ?2 FROM core_messages m
                     WHERE m.session_id = ?1 AND {UNCLAIMED} AND NOT ({HELD_INPUT})"
                ),
                params![session.0, at],
            )?)
        })
        .await
    }
}

fn agent_activity(
    conn: &rusqlite::Connection,
    child: SessionId,
    created_at: i64,
) -> rusqlite::Result<AgentActivity> {
    let running: bool = conn.query_row(
        "SELECT EXISTS(SELECT 1 FROM core_runs WHERE session_id = ?1 AND state = 'executing')",
        [child.0],
        |r| r.get(0),
    )?;
    let last_run = conn
        .query_row(
            "SELECT id, session_id, state, created_at, finished_at FROM core_runs
             WHERE session_id = ?1 AND state != 'executing' ORDER BY id DESC LIMIT 1",
            [child.0],
            row::run,
        )
        .optional()?
        .map(|r| r.state);
    let last_message: Option<i64> = conn.query_row(
        "SELECT MAX(created_at) FROM core_messages WHERE session_id = ?1",
        [child.0],
        |r| r.get(0),
    )?;
    Ok(AgentActivity {
        running,
        last_run,
        last_active_at: last_message.unwrap_or(created_at).max(created_at),
    })
}
