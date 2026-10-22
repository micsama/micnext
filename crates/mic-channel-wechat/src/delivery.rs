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
use crate::limits::{ATTEMPT_ID_BYTES, TEXT_CHUNK_CHARS};
use crate::service::ConnectionExit;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Plan {
    chunks: Vec<String>,
}

enum Authentication {
    Current,
    Expired,
}
enum DeliveryResult {
    Complete(Authentication),
    Stopped,
}

/// 投递结果只记在 core；本模块只留引用要用的分段计划与每段发送事实。
/// `delivered` 发布已处理的出站前缀游标，供 typing 在回复送达后取消。
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
    settle_before(&kernel, &account, cursor).await?;
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
    if pending.target.channel != "wechat" {
        return Err(AccountError::Invalid("投递目标渠道不匹配").into());
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
    // 结果未知不重放：每段只发一次。
    for (chunk_index, chunk) in plan.chunks.iter().enumerate() {
        if stop.is_cancelled() {
            return Ok(DeliveryResult::Stopped);
        }
        let client_id = client_id()?;
        start_chunk(kernel, id, chunk_index, &client_id).await?;
        let sent = tokio::select! {
            biased;
            () = stop.cancelled() => return Ok(DeliveryResult::Stopped),
            result = client.send(&account.credentials, token, &client_id, chunk) => result,
        };
        let error = match sent {
            Ok(external_id) => {
                finish_chunk(
                    kernel,
                    id,
                    chunk_index,
                    Some(external_id.0.to_string()),
                    None,
                )
                .await?;
                continue;
            }
            Err(error) => error,
        };
        tracing::warn!(
            session = account.view.session_id.0,
            message_id = id.0,
            chunk_index,
            kind = failure(&error),
            %error,
            "wechat delivery failed"
        );
        finish_chunk(kernel, id, chunk_index, None, Some(failure(&error))).await?;
        if matches!(error, ClientError::Protocol(_)) {
            return Err(error.into());
        }
        let reason = if chunk_index == 0 {
            error.to_string()
        } else {
            format!("只送达前 {chunk_index} 段：{error}")
        };
        kernel.mark_delivery_failed(id, reason).await?;
        return Ok(DeliveryResult::Complete(
            if matches!(error, ClientError::SessionExpired) {
                Authentication::Expired
            } else {
                Authentication::Current
            },
        ));
    }
    kernel.mark_delivered(id).await?;
    tracing::debug!(message_id = id.0, "wechat delivery accepted");
    Ok(DeliveryResult::Complete(Authentication::Current))
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
                "SELECT d.plan, c.chunk_index
                 FROM wechat_delivery_chunk c
                 JOIN wechat_delivery d ON d.message_id = c.message_id
                 WHERE d.user_id = ?1 AND c.external_message_id = ?2
                 ORDER BY c.message_id, c.chunk_index LIMIT 1",
                params![user, external_id],
                |row| Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?)),
            )
            .optional()
        })
        .await?;
    raw.map(|(raw, chunk)| {
        let plan: Plan =
            serde_json::from_str(&raw).map_err(|_| AccountError::Invalid("投递计划字段不匹配"))?;
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
    kernel
        .with_module_tx(move |tx| {
            tx.execute(
                "INSERT INTO wechat_delivery(message_id, user_id, plan) VALUES (?1, ?2, ?3)",
                params![id.0, user, plan],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}

async fn start_chunk(
    kernel: &Kernel,
    id: MessageId,
    chunk: usize,
    client_id: &str,
) -> Result<(), AccountError> {
    let client_id = client_id.to_owned();
    kernel.with_module_tx(move |tx| {
        tx.execute("INSERT INTO wechat_delivery_chunk(message_id, chunk_index, client_id, started_at) VALUES (?1, ?2, ?3, ?4)", params![id.0, chunk as i64, client_id, now_ms()])?;
        Ok(())
    }).await?;
    Ok(())
}

async fn finish_chunk(
    kernel: &Kernel,
    id: MessageId,
    chunk: usize,
    external: Option<String>,
    failure: Option<&'static str>,
) -> Result<(), AccountError> {
    kernel.with_module_tx(move |tx| {
        tx.execute("UPDATE wechat_delivery_chunk SET finished_at = ?3, external_message_id = ?4, failure = ?5 WHERE message_id = ?1 AND chunk_index = ?2", params![id.0, chunk as i64, now_ms(), external, failure])?;
        Ok(())
    }).await?;
    Ok(())
}

const INTERRUPTED: &str = "投递中途中断，可能未送达";
const BEFORE_CONNECTION: &str = "连接恢复前的消息不补发";

fn failure(error: &ClientError) -> &'static str {
    match error {
        ClientError::Network => "network",
        ClientError::Timeout => "timeout",
        ClientError::Rejected(_) | ClientError::BusinessRejected { .. } => "rejected",
        ClientError::SessionExpired => "session_expired",
        ClientError::Protocol(_) => "protocol",
    }
}

fn client_id() -> Result<String, ClientError> {
    let mut bytes = [0; ATTEMPT_ID_BYTES];
    getrandom::fill(&mut bytes).map_err(|error| {
        tracing::error!(%error, "wechat random source failed");
        ClientError::Protocol("随机源不可用")
    })?;
    Ok(bytes.iter().map(|byte| format!("{byte:02x}")).collect())
}

/// 连接开始投递前收尾本会话切点及以前的待投递消息：已开始发送的结果未知，其余不补发。
/// 投递循环尚未启动，没有并发；逐条各自提交，中途退出下次再跑结果相同。
async fn settle_before(
    kernel: &Kernel,
    account: &Account,
    cut: Option<MessageId>,
) -> Result<(), BoxError> {
    let Some(cut) = cut else {
        return Ok(());
    };
    let session = account.view.session_id;
    for pending in kernel.pending_deliveries("wechat").await? {
        let id = pending.message.id;
        if pending.message.session_id != session || id > cut {
            continue;
        }
        let started = kernel
            .with_module_tx(move |tx| {
                tx.query_row(
                    "SELECT 1 FROM wechat_delivery WHERE message_id = ?1",
                    [id.0],
                    |_| Ok(()),
                )
                .optional()
            })
            .await?
            .is_some();
        let reason = if started {
            INTERRUPTED
        } else {
            BEFORE_CONNECTION
        };
        kernel.mark_delivery_failed(id, reason.to_owned()).await?;
    }
    kernel
        .with_module_tx(|tx| {
            tx.execute(
                "UPDATE wechat_delivery_chunk SET finished_at = ?1, failure = 'interrupted' WHERE finished_at IS NULL",
                [now_ms()],
            )?;
            Ok(())
        })
        .await?;
    Ok(())
}
