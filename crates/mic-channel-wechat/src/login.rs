use mic_core::{SetupAttemptId, SetupFailure, SetupProgress};
use reqwest::Url;
use tokio::sync::{mpsc, watch};
use tokio::time::{timeout_at, Instant};
use tokio_util::sync::CancellationToken;

use crate::account::{Credentials, Token};
use crate::client::{Client, ClientError, LoginStatus};
use crate::limits::{API_BASE, NETWORK_RETRY, POLL_INTERVAL, QR_LIFETIME};

pub(crate) struct Progress {
    pub id: SetupAttemptId,
    pub progress: SetupProgress,
}

pub(crate) enum Outcome {
    Connected(Credentials),
    Expired,
    Failed(SetupFailure),
    Cancelled,
}

pub(crate) async fn run(
    id: SetupAttemptId,
    tokens: Vec<Token>,
    mut code: watch::Receiver<Option<String>>,
    progress: mpsc::UnboundedSender<Progress>,
    stop: CancellationToken,
) -> Result<Outcome, ClientError> {
    let deadline = Instant::now() + QR_LIFETIME;
    let flow = async {
        let client = Client::new()?;
        let qr = match client.qr(&tokens).await {
            Ok(qr) => qr,
            Err(ClientError::Protocol(reason)) => return Err(ClientError::Protocol(reason)),
            Err(error) => {
                tracing::warn!(error = %error, "wechat QR fetch failed");
                return Ok(Outcome::Failed(SetupFailure::Network));
            }
        };
        let emit = |state| {
            let _ = progress.send(Progress {
                id: id.clone(),
                progress: state,
            });
        };
        emit(SetupProgress::Waiting {
            qr_content: qr.content.clone(),
        });
        let mut base = Url::parse(API_BASE).expect("固定 API 地址合法");
        let mut verify_code: Option<String> = None;
        loop {
            let status = match client.poll(&base, &qr, verify_code.as_deref()).await {
                Ok(status) => status,
                Err(ClientError::Timeout) => continue,
                Err(
                    error @ (ClientError::Network
                    | ClientError::Rejected(_)
                    | ClientError::BusinessRejected
                    | ClientError::SessionExpired),
                ) => {
                    tracing::warn!(error = %error, "wechat QR polling network failure");
                    tokio::time::sleep(NETWORK_RETRY).await;
                    continue;
                }
                Err(error @ ClientError::Protocol(_)) => return Err(error),
            };
            match status {
                LoginStatus::Waiting => {}
                LoginStatus::Scanned => {
                    verify_code = None;
                    emit(SetupProgress::Scanned {
                        qr_content: qr.content.clone(),
                    });
                }
                LoginStatus::Redirect(url) => {
                    base = url;
                    emit(SetupProgress::Scanned {
                        qr_content: qr.content.clone(),
                    });
                }
                LoginStatus::NeedsCode => {
                    code.borrow_and_update();
                    emit(SetupProgress::NeedsCode {
                        qr_content: qr.content.clone(),
                    });
                    if code.changed().await.is_err() {
                        return Ok(Outcome::Cancelled);
                    }
                    verify_code = code.borrow_and_update().clone();
                    continue;
                }
                LoginStatus::VerificationBlocked => {
                    return Ok(Outcome::Failed(SetupFailure::VerificationBlocked))
                }
                LoginStatus::ExistingBinding => {
                    return Ok(Outcome::Failed(SetupFailure::ExistingBinding))
                }
                LoginStatus::Expired => return Ok(Outcome::Expired),
                LoginStatus::Confirmed(credentials) => return Ok(Outcome::Connected(credentials)),
            }
            tokio::time::sleep(POLL_INTERVAL).await;
        }
    };
    tokio::select! {
        biased;
        () = stop.cancelled() => Ok(Outcome::Cancelled),
        result = timeout_at(deadline, flow) => match result {
            Ok(result) => result,
            Err(_) => Ok(Outcome::Expired),
        },
    }
}
