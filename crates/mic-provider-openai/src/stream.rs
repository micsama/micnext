//! 两协议共用的流驱动：打开、逐事件、空闲超时、EOF；丢弃流即取消（SDK 读流任务随接收端关闭退出）。

use std::future::Future;
use std::pin::Pin;

use async_openai::error::OpenAIError;
use async_openai::types::stream::StreamResponse;
use futures_util::stream::{self, StreamExt};
use mic_core::{BoxStream, ModelEvent, ModelResponse, ProviderError, StopReason};

use crate::error::{self, transient};
use crate::limits::STREAM_IDLE_TIMEOUT;

/// 协议侧的增量累积。
pub(crate) trait Accumulate: Send + 'static {
    type Event: Send + 'static;
    fn push(&mut self, event: Self::Event) -> Result<Step, ProviderError>;
    /// 上游正常结束而 `push` 未给出终态。
    fn eof(self) -> Result<ModelResponse, ProviderError>;
}

pub(crate) enum Step {
    Deltas(Vec<ModelEvent>),
    Finished(ModelResponse),
}

pub(crate) type Open<E> =
    Pin<Box<dyn Future<Output = Result<StreamResponse<E>, OpenAIError>> + Send>>;

type Item = Result<ModelEvent, ProviderError>;

enum State<A: Accumulate> {
    Open(Open<A::Event>, A),
    Streaming(StreamResponse<A::Event>, A),
    Done,
}

pub(crate) fn drive<A: Accumulate>(open: Open<A::Event>, acc: A) -> BoxStream<Item> {
    Box::pin(
        stream::unfold(State::Open(open, acc), |state| async move {
            match state {
                State::Done => None,
                State::Open(open, acc) => Some(match idle(open).await {
                    Ok(Ok(s)) => (Vec::new(), State::Streaming(s, acc)),
                    Ok(Err(e)) => fail(error::provider(e)),
                    Err(e) => fail(e),
                }),
                State::Streaming(mut s, mut acc) => Some(match idle(s.next()).await {
                    Ok(Some(Ok(event))) => match acc.push(event) {
                        Ok(Step::Deltas(events)) => (
                            events.into_iter().map(Ok).collect(),
                            State::Streaming(s, acc),
                        ),
                        Ok(Step::Finished(resp)) => finish(Ok(resp)),
                        Err(e) => fail(e),
                    },
                    Ok(Some(Err(e))) => fail(error::provider(e)),
                    Ok(None) => finish(acc.eof()),
                    Err(e) => fail(e),
                }),
            }
        })
        .flat_map(stream::iter),
    )
}

/// 正常结束却没有任何内容视为上游故障，交给 core 重试。
fn finish<A: Accumulate>(resp: Result<ModelResponse, ProviderError>) -> (Vec<Item>, State<A>) {
    match resp {
        Ok(resp) if resp.stop == StopReason::EndTurn && resp.blocks.is_empty() => {
            fail(transient("上游返回了空回复"))
        }
        Ok(resp) => (vec![Ok(ModelEvent::Finished(resp))], State::Done),
        Err(e) => fail(e),
    }
}

fn fail<A: Accumulate>(e: ProviderError) -> (Vec<Item>, State<A>) {
    (vec![Err(e)], State::Done)
}

/// 单次等待超过空闲上限 → `Transient`。
async fn idle<F: Future>(f: F) -> Result<F::Output, ProviderError> {
    tokio::time::timeout(STREAM_IDLE_TIMEOUT, f)
        .await
        .map_err(|_| transient(format!("{} 秒没有收到数据", STREAM_IDLE_TIMEOUT.as_secs())))
}
