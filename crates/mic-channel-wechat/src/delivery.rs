use std::sync::Arc;

use mic_core::{BoxError, EventReceiver, Kernel, KernelEventKind};
use mic_message::{MessageBody, MessageId, ReplyBlock};
use mic_store::rusqlite::{params, OptionalExtension};
use mic_store::PendingDelivery;
use serde::{Deserialize, Serialize};
use tokio::sync::watch;
use tokio_util::sync::CancellationToken;

use crate::account::{now_ms, Account, AccountError, WechatTarget};
use crate::client::{Client, ClientError};
use crate::limits::{ATTEMPT_ID_BYTES, SEND_ATTEMPTS, SEND_RETRY, TEXT_CHUNK_CHARS};
use crate::service::ConnectionExit;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    chunks: Vec<String>,
}

enum Outcome {
    Sending,
    Sent,
    Skipped,
    Interrupted,
}
enum Authentication {
    Current,
    Expired,
}
enum DeliveryResult {
    Complete(Authentication),
    Stopped,
}

/// `delivered` 发布已处理（送达、跳过或此前已有结果）的出站前缀游标，供 typing 在回复送达后取消。
pub(crate) async fn run(
    kernel: Kernel,
    account: Arc<Account>,
    mut events: EventReceiver,
    delivered: watch::Sender<Option<MessageId>>,
    mut context: watch::Receiver<Option<String>>,
    stop: CancellationToken,
) -> Result<ConnectionExit, BoxError> {
    let client = Client::new()?;
    let mut cursor = *delivered.borrow();
    loop {
        let event = tokio::select! {
            biased;
            () = stop.cancelled() => return Ok(ConnectionExit::Stopped),
            event = events.recv() => event,
        };
        let scan = match event {
            Ok(event) if event.session_id == account.view.session_id => {
                matches!(event.kind, KernelEventKind::MessageAppended(message)
                        if matches!(message.body, MessageBody::Reply { .. } | MessageBody::Notification { .. }))
            }
            Ok(_) => false,
            Err(_) => true,
        };
        if !scan {
            continue;
        }
        for pending in kernel.pending_deliveries("wechat").await? {
            if stop.is_cancelled() {
                return Ok(ConnectionExit::Stopped);
            }
            if pending.message.session_id != account.view.session_id
                || cursor.is_some_and(|cursor| pending.message.id <= cursor)
            {
                continue;
            }
            let id = pending.message.id;
            if let Some(outcome) = stored(&kernel, id).await? {
                match outcome {
                    Outcome::Sending | Outcome::Sent | Outcome::Skipped | Outcome::Interrupted => {}
                }
                cursor = Some(id);
                delivered.send_replace(cursor);
                continue;
            }
            let token = loop {
                if let Some(token) = context.borrow_and_update().clone() {
                    break token;
                }
                tokio::select! {
                    biased;
                    () = stop.cancelled() => return Ok(ConnectionExit::Stopped),
                    changed = context.changed() => changed.map_err(|_| AccountError::Invalid("入站任务提前退出"))?,
                }
            };
            let authentication =
                match deliver(&kernel, &client, &account, &token, pending, &stop).await? {
                    DeliveryResult::Complete(authentication) => authentication,
                    DeliveryResult::Stopped => return Ok(ConnectionExit::Stopped),
                };
            cursor = Some(id);
            delivered.send_replace(cursor);
            if matches!(authentication, Authentication::Expired) {
                return Ok(ConnectionExit::NeedsLogin);
            }
        }
    }
}

async fn deliver(
    kernel: &Kernel,
    client: &Client,
    account: &Account,
    token: &str,
    pending: PendingDelivery,
    stop: &CancellationToken,
) -> Result<DeliveryResult, BoxError> {
    if pending.target.channel != "wechat" || pending.target.version != 1 {
        return Err(AccountError::Invalid("投递目标版本或渠道不匹配").into());
    }
    let target: WechatTarget = serde_json::from_str(&pending.target.payload)
        .map_err(|_| AccountError::Invalid("投递目标字段不匹配"))?;
    if target.user_id != account.credentials.user_id {
        return Err(AccountError::Invalid("投递目标归属不一致").into());
    }
    let text = match pending.message.body {
        MessageBody::Reply { blocks, .. } => blocks
            .into_iter()
            .filter_map(|block| match block {
                ReplyBlock::Text { text, .. } => Some(text),
                _ => None,
            })
            .collect::<String>(),
        MessageBody::Notification { text, .. } => text,
        _ => unreachable!("pending_deliveries 只返回 Reply/Notification"),
    };
    let plan = plan(text);
    let id = pending.message.id;
    begin(kernel, id, &account.credentials.user_id.0, &plan).await?;
    let mut authentication = Authentication::Current;
    for (chunk_index, chunk) in plan.chunks.iter().enumerate() {
        let client_id = client_id()?;
        for attempt in 1..=SEND_ATTEMPTS {
            if stop.is_cancelled() {
                return Ok(DeliveryResult::Stopped);
            }
            start_attempt(kernel, id, chunk_index, attempt, &client_id).await?;
            let sent = tokio::select! {
                biased;
                () = stop.cancelled() => return Ok(DeliveryResult::Stopped),
                result = client.send(&account.credentials, token, &client_id, chunk) => result,
            };
            match sent {
                Ok(external_id) => {
                    finish_attempt(
                        kernel,
                        id,
                        chunk_index,
                        attempt,
                        Some(external_id.0.to_string()),
                        None,
                    )
                    .await?;
                    break;
                }
                Err(error) => {
                    if matches!(error, ClientError::SessionExpired) {
                        authentication = Authentication::Expired;
                    }
                    finish_attempt(
                        kernel,
                        id,
                        chunk_index,
                        attempt,
                        None,
                        Some(failure(&error)),
                    )
                    .await?;
                    if matches!(error, ClientError::Protocol(_)) {
                        return Err(error.into());
                    }
                    if attempt == SEND_ATTEMPTS {
                        finish(kernel, id, Outcome::Skipped).await?;
                        tracing::warn!(message_id = id.0, chunk_index, "wechat delivery skipped");
                        return Ok(DeliveryResult::Complete(authentication));
                    }
                    tokio::select! {
                        biased;
                        () = stop.cancelled() => return Ok(DeliveryResult::Stopped),
                        () = tokio::time::sleep(SEND_RETRY) => {},
                    }
                }
            }
        }
    }
    kernel.mark_delivered(id).await?;
    finish(kernel, id, Outcome::Sent).await?;
    tracing::debug!(message_id = id.0, "wechat delivery accepted");
    Ok(DeliveryResult::Complete(authentication))
}

fn plan(text: String) -> Plan {
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut chars = 0;
    for ch in text.chars() {
        current.push(ch);
        chars += 1;
        if chars == TEXT_CHUNK_CHARS {
            chunks.push(std::mem::take(&mut current));
            chars = 0;
        }
    }
    if !current.is_empty() {
        chunks.push(current);
    }
    Plan { chunks }
}

fn disk_plan(version: u32, raw: &str) -> Result<Plan, AccountError> {
    if version != 1 {
        return Err(AccountError::Invalid("投递计划版本不匹配"));
    }
    let plan: Plan =
        serde_json::from_str(raw).map_err(|_| AccountError::Invalid("投递计划字段不匹配"))?;
    if plan
        .chunks
        .iter()
        .any(|chunk| chunk.is_empty() || chunk.chars().count() > TEXT_CHUNK_CHARS)
    {
        return Err(AccountError::Invalid("投递计划分段无效"));
    }
    Ok(plan)
}

fn outcome(raw: &str) -> Result<Outcome, AccountError> {
    match raw {
        "sending" => Ok(Outcome::Sending),
        "sent" => Ok(Outcome::Sent),
        "skipped" => Ok(Outcome::Skipped),
        "interrupted" => Ok(Outcome::Interrupted),
        _ => Err(AccountError::Invalid("投递状态未知")),
    }
}

async fn stored(kernel: &Kernel, id: MessageId) -> Result<Option<Outcome>, AccountError> {
    let raw = kernel
        .with_module_tx(move |tx| {
            tx.query_row(
                "SELECT outcome, plan_version, plan FROM wechat_delivery WHERE message_id = ?1",
                [id.0],
                |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, u32>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                },
            )
            .optional()
        })
        .await?;
    raw.map(|(raw, version, plan)| {
        disk_plan(version, &plan)?;
        outcome(&raw)
    })
    .transpose()
}

pub(crate) async fn quoted_text(
    kernel: &Kernel,
    user: &crate::account::WechatUserId,
    external_id: crate::wire::MessageId,
) -> Result<Option<String>, AccountError> {
    let user = user.0.clone();
    let external_id = external_id.0.to_string();
    let raw = kernel
        .with_module_tx(move |tx| {
            tx.query_row(
                "SELECT d.plan_version, d.plan, a.chunk_index
                 FROM wechat_delivery_attempt a
                 JOIN wechat_delivery d ON d.message_id = a.message_id
                 WHERE d.user_id = ?1 AND a.external_message_id = ?2
                 ORDER BY a.message_id, a.chunk_index, a.attempt_no LIMIT 1",
                params![user, external_id],
                |row| {
                    Ok((
                        row.get::<_, u32>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, i64>(2)?,
                    ))
                },
            )
            .optional()
        })
        .await?;
    raw.map(|(version, raw, chunk)| {
        let plan = disk_plan(version, &raw)?;
        let chunk =
            usize::try_from(chunk).map_err(|_| AccountError::Invalid("引用发送段位置不合法"))?;
        plan.chunks
            .into_iter()
            .nth(chunk)
            .ok_or(AccountError::Invalid("引用发送段不存在"))
    })
    .transpose()
}

async fn begin(
    kernel: &Kernel,
    id: MessageId,
    user: &str,
    plan: &Plan,
) -> Result<(), AccountError> {
    let plan = serde_json::to_string(plan).expect("分段计划可序列化");
    let user = user.to_owned();
    kernel.with_module_tx(move |tx| {
        tx.execute("INSERT INTO wechat_delivery(message_id, user_id, plan_version, plan, outcome) VALUES (?1, ?2, 1, ?3, 'sending')", params![id.0, user, plan])?;
        Ok(())
    }).await?;
    Ok(())
}

async fn start_attempt(
    kernel: &Kernel,
    id: MessageId,
    chunk: usize,
    attempt: u32,
    client_id: &str,
) -> Result<(), AccountError> {
    let client_id = client_id.to_owned();
    kernel.with_module_tx(move |tx| {
        tx.execute("INSERT INTO wechat_delivery_attempt(message_id, chunk_index, attempt_no, client_id, started_at) VALUES (?1, ?2, ?3, ?4, ?5)", params![id.0, chunk as i64, attempt, client_id, now_ms()])?;
        Ok(())
    }).await?;
    Ok(())
}

async fn finish_attempt(
    kernel: &Kernel,
    id: MessageId,
    chunk: usize,
    attempt: u32,
    external: Option<String>,
    failure: Option<&'static str>,
) -> Result<(), AccountError> {
    kernel.with_module_tx(move |tx| {
        tx.execute("UPDATE wechat_delivery_attempt SET finished_at = ?4, external_message_id = ?5, failure = ?6 WHERE message_id = ?1 AND chunk_index = ?2 AND attempt_no = ?3", params![id.0, chunk as i64, attempt, now_ms(), external, failure])?;
        Ok(())
    }).await?;
    Ok(())
}

async fn finish(kernel: &Kernel, id: MessageId, outcome: Outcome) -> Result<(), AccountError> {
    let outcome = match outcome {
        Outcome::Sent => "sent",
        Outcome::Skipped => "skipped",
        _ => unreachable!("运行期只收尾 sent/skipped"),
    };
    kernel
        .with_module_tx(move |tx| {
            tx.execute(
                "UPDATE wechat_delivery SET outcome = ?2, finished_at = ?3 WHERE message_id = ?1",
                params![id.0, outcome, now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

fn failure(error: &ClientError) -> &'static str {
    match error {
        ClientError::Network => "network",
        ClientError::Timeout => "timeout",
        ClientError::Rejected(_) | ClientError::BusinessRejected => "rejected",
        ClientError::SessionExpired => "session_expired",
        ClientError::Protocol(_) => "protocol",
    }
}

fn client_id() -> Result<String, ClientError> {
    let mut bytes = [0; ATTEMPT_ID_BYTES];
    getrandom::fill(&mut bytes).map_err(|_| ClientError::Protocol("随机源不可用"))?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

pub(crate) async fn recover(kernel: &Kernel) -> Result<(), AccountError> {
    let plans = kernel
        .with_module_tx(|tx| {
            tx.prepare("SELECT plan_version, plan FROM wechat_delivery WHERE outcome = 'sending'")?
                .query_map([], |row| {
                    Ok((row.get::<_, u32>(0)?, row.get::<_, String>(1)?))
                })?
                .collect::<Result<Vec<_>, _>>()
        })
        .await?;
    for (version, plan) in plans {
        disk_plan(version, &plan)?;
    }
    kernel.with_module_tx(|tx| {
        tx.execute("UPDATE wechat_delivery_attempt SET finished_at = ?1, failure = 'interrupted' WHERE finished_at IS NULL", [now_ms()])?;
        tx.execute("UPDATE wechat_delivery SET outcome = 'interrupted', finished_at = ?1 WHERE outcome = 'sending'", [now_ms()])?;
        Ok(())
    }).await?;
    Ok(())
}
