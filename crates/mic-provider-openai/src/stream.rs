//! 两协议共用的流驱动：打开、SSE 分帧、逐事件、空闲超时、EOF；丢弃流即断开连接。

use std::future::Future;
use std::pin::Pin;

use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::stream::{self, Stream, StreamExt};
use mic_core::{BoxStream, ModelEvent, ModelResponse, ProviderError};
use mic_store::Usage;
use reqwest::Response;
use serde::de::DeserializeOwned;

use crate::error::{protocol, transient};
use crate::limits::STREAM_IDLE_TIMEOUT;

/// 协议侧的增量累积。
pub(crate) trait Accumulate: Send + 'static {
    type Event: DeserializeOwned;
    fn push(&mut self, event: Self::Event) -> Result<Step, ProviderError>;
    /// 上游正常结束（EOF 或 `[DONE]`）而 `push` 未给出终态。
    fn eof(self) -> Result<ModelResponse, ProviderError>;
}

pub(crate) enum Step {
    Deltas(Vec<ModelEvent>),
    /// 未知事件类型：忽略并告警。
    Unknown,
    /// 终态；用量先于 `Finished` 作为 `Usage` 事件交出。
    Finished(ModelResponse, Option<Usage>),
    /// 上游报告失败；已收到的用量照常交出。
    Failed(ProviderError, Option<Usage>),
}

pub(crate) type Open = Pin<Box<dyn Future<Output = Result<Response, ProviderError>> + Send>>;

type Events = Pin<Box<dyn Stream<Item = Result<Event, EventStreamError<reqwest::Error>>> + Send>>;

type Item = Result<ModelEvent, ProviderError>;

enum State<A> {
    Open(Open, A),
    Streaming(Events, A),
    Done,
}

pub(crate) fn drive<A: Accumulate>(open: Open, acc: A) -> BoxStream<Item> {
    Box::pin(
        stream::unfold(State::Open(open, acc), |state| async move {
            match state {
                State::Done => None,
                State::Open(open, acc) => Some(match idle(open).await.and_then(|r| r) {
                    Ok(resp) => (
                        Vec::new(),
                        State::Streaming(Box::pin(resp.bytes_stream().eventsource()), acc),
                    ),
                    Err(e) => fail(e),
                }),
                State::Streaming(mut s, mut acc) => Some(match idle(s.next()).await {
                    Ok(Some(Ok(event))) if event.event == "keepalive" => {
                        (Vec::new(), State::Streaming(s, acc))
                    }
                    Ok(Some(Ok(event))) if event.data == "[DONE]" => finish(acc.eof(), None),
                    Ok(Some(Ok(event))) => match parse(&event.data).and_then(|e| acc.push(e)) {
                        Ok(Step::Deltas(events)) => (
                            events.into_iter().map(Ok).collect(),
                            State::Streaming(s, acc),
                        ),
                        Ok(Step::Unknown) => {
                            tracing::warn!(
                                sse_event = event.event.as_str(),
                                kind = event_type(&event.data),
                                "ignored unknown upstream stream event"
                            );
                            (Vec::new(), State::Streaming(s, acc))
                        }
                        Ok(Step::Finished(resp, usage)) => finish(Ok(resp), usage),
                        Ok(Step::Failed(e, usage)) => fail_with(e, usage),
                        Err(e) => fail(e),
                    },
                    Ok(Some(Err(e))) => fail(match e {
                        EventStreamError::Transport(e) => {
                            crate::error::provider(crate::client::Failure::Network(e))
                        }
                        e => transient(format!("流中途断开：{}", crate::error::chain(&e))),
                    }),
                    Ok(None) => finish(acc.eof(), None),
                    Err(e) => fail(e),
                }),
            }
        })
        .flat_map(stream::iter),
    )
}

fn parse<E: DeserializeOwned>(data: &str) -> Result<E, ProviderError> {
    serde_json::from_str(data).map_err(|e| protocol(format!("上游数据无法解析：{e}")))
}

/// 只取 `type`，不记原文。
fn event_type(data: &str) -> Option<String> {
    #[derive(serde::Deserialize)]
    struct Tagged {
        r#type: String,
    }
    serde_json::from_str::<Tagged>(data).ok().map(|t| t.r#type)
}

fn finish<A>(
    resp: Result<ModelResponse, ProviderError>,
    usage: Option<Usage>,
) -> (Vec<Item>, State<A>) {
    match resp {
        Ok(resp) => (
            usage
                .map(|u| Ok(ModelEvent::Usage(u)))
                .into_iter()
                .chain([Ok(ModelEvent::Finished(resp))])
                .collect(),
            State::Done,
        ),
        Err(e) => fail_with(e, usage),
    }
}

fn fail_with<A>(e: ProviderError, usage: Option<Usage>) -> (Vec<Item>, State<A>) {
    (
        usage
            .map(|u| Ok(ModelEvent::Usage(u)))
            .into_iter()
            .chain([Err(e)])
            .collect(),
        State::Done,
    )
}

fn fail<A>(e: ProviderError) -> (Vec<Item>, State<A>) {
    (vec![Err(e)], State::Done)
}

/// 单次等待超过空闲上限 → `Stalled`。
async fn idle<F: Future>(f: F) -> Result<F::Output, ProviderError> {
    tokio::time::timeout(STREAM_IDLE_TIMEOUT, f)
        .await
        .map_err(|_| ProviderError::Stalled {
            message: format!("{} 秒没有收到数据", STREAM_IDLE_TIMEOUT.as_secs()),
        })
}
