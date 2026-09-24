use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures_util::stream::{self, Stream, StreamExt};
use mic_core::{BoxError, BoxStream, ModelEvent, ModelRequest, Provider, ProviderError};
use reqwest::header::{HeaderMap, RETRY_AFTER};

use crate::config::Resolved;
use crate::limits::{CONNECT_TIMEOUT, STREAM_IDLE_TIMEOUT};
use crate::request;
use crate::response::{http_error, protocol, transient, Accumulator};
use crate::wire::Chunk;

pub(crate) struct OpenAiProvider {
    client: reqwest::Client,
    cfg: Arc<Resolved>,
}

impl OpenAiProvider {
    pub(crate) fn new(cfg: Resolved) -> Result<Self, BoxError> {
        let client = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .default_headers(cfg.headers.clone())
            .build()?;
        Ok(Self {
            client,
            cfg: Arc::new(cfg),
        })
    }
}

type Sse = Pin<Box<dyn Stream<Item = Result<Event, EventStreamError<reqwest::Error>>> + Send>>;

enum State {
    Start {
        client: reqwest::Client,
        cfg: Arc<Resolved>,
        body: Vec<u8>,
    },
    Streaming {
        sse: Sse,
        acc: Accumulator,
    },
    Done,
}

type Item = Result<ModelEvent, ProviderError>;

impl Provider for OpenAiProvider {
    fn model(&self) -> &str {
        &self.cfg.model
    }

    fn stream(&self, req: ModelRequest) -> BoxStream<Item> {
        let body = match request::build(&self.cfg, &req) {
            Ok(r) => serde_json::to_vec(&r).expect("请求体可序列化"),
            Err(e) => return Box::pin(stream::once(async { Err(e) })),
        };
        let init = State::Start {
            client: self.client.clone(),
            cfg: self.cfg.clone(),
            body,
        };
        // 丢弃流即丢弃在途的 reqwest future / 响应体，连接随之关闭。
        Box::pin(
            stream::unfold(init, |state| async move {
                match state {
                    State::Done => None,
                    State::Start { client, cfg, body } => Some(connect(client, &cfg, body).await),
                    State::Streaming { sse, acc } => Some(next(sse, acc).await),
                }
            })
            .flat_map(stream::iter),
        )
    }
}

async fn connect(client: reqwest::Client, cfg: &Resolved, body: Vec<u8>) -> (Vec<Item>, State) {
    let send = client
        .post(&cfg.url)
        .header(reqwest::header::CONTENT_TYPE, "application/json")
        .body(body)
        .send();
    let resp = match idle(send).await {
        Ok(Ok(resp)) => resp,
        Ok(Err(e)) => return fail(transient(format!("请求失败：{e}"), None)),
        Err(e) => return fail(e),
    };
    let status = resp.status();
    if !status.is_success() {
        let retry_after = retry_after(resp.headers());
        let body = match idle(resp.text()).await {
            Ok(Ok(body)) => body,
            Ok(Err(e)) => format!("（读取错误体失败：{e}）"),
            Err(e) => return fail(e),
        };
        return fail(http_error(status, retry_after, &body));
    }
    let sse: Sse = Box::pin(resp.bytes_stream().eventsource());
    (
        Vec::new(),
        State::Streaming {
            sse,
            acc: Accumulator::new(cfg.dialect),
        },
    )
}

async fn next(mut sse: Sse, mut acc: Accumulator) -> (Vec<Item>, State) {
    let event = match idle(sse.next()).await {
        Ok(Some(Ok(event))) => event,
        Ok(Some(Err(EventStreamError::Transport(e)))) => {
            return fail(transient(format!("流中途断开：{e}"), None))
        }
        Ok(Some(Err(e))) => return fail(protocol(format!("SSE 分帧失败：{e}"))),
        Ok(None) => return fail(transient("未收到 [DONE] 流就结束了".into(), None)),
        Err(e) => return fail(e),
    };
    if event.data == "[DONE]" {
        return match acc.finish() {
            Ok(resp) => (vec![Ok(ModelEvent::Finished(resp))], State::Done),
            Err(e) => fail(e),
        };
    }
    let chunk = match serde_json::from_str::<Chunk>(&event.data) {
        Ok(chunk) => chunk,
        Err(e) => {
            let raw: String = event
                .data
                .chars()
                .take(crate::limits::ERROR_BODY_CHARS)
                .collect();
            return fail(protocol(format!("分片不是合法的 JSON（{e}）：{raw}")));
        }
    };
    match acc.push(chunk) {
        Ok(events) => (
            events.into_iter().map(Ok).collect(),
            State::Streaming { sse, acc },
        ),
        Err(e) => fail(e),
    }
}

fn fail(e: ProviderError) -> (Vec<Item>, State) {
    (vec![Err(e)], State::Done)
}

/// 单次等待超过空闲上限 → `Transient`。
async fn idle<F: std::future::Future>(f: F) -> Result<F::Output, ProviderError> {
    tokio::time::timeout(STREAM_IDLE_TIMEOUT, f)
        .await
        .map_err(|_| {
            transient(
                format!("{} 秒没有收到数据", STREAM_IDLE_TIMEOUT.as_secs()),
                None,
            )
        })
}

/// 只认秒数形式；HTTP 日期形式视为未给。
fn retry_after(headers: &HeaderMap) -> Option<Duration> {
    let secs = headers
        .get(RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse()
        .ok()?;
    Some(Duration::from_secs(secs))
}
