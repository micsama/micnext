//! 一次模型调用尝试的观测：首次输出、结束时刻与用量在流上采集，成败与用量独立。

use std::future::poll_fn;

use mic_message::SessionId;
use mic_store::{
    ModelCallContext, ModelCallOutcome, ModelCallPurpose, ModelId, NewModelCall, ReasoningEffort,
    RunId, StopReason, Usage,
};

use crate::run::now_ms;
use crate::{ModelEvent, ModelRequest, ModelResponse, Provider, ProviderError};

pub(crate) struct Facts {
    pub started_at: i64,
    pub first_output_at: Option<i64>,
    pub finished_at: i64,
    pub usage: Option<Usage>,
}

impl Facts {
    pub(crate) fn start() -> Self {
        let now = now_ms();
        Self {
            started_at: now,
            first_output_at: None,
            finished_at: now,
            usage: None,
        }
    }

    /// 调用行；`stop` 只在成功时有。
    pub(crate) fn row(
        &self,
        by: &Caller,
        purpose: ModelCallPurpose,
        context: ModelCallContext,
        stop: Option<StopReason>,
        outcome: ModelCallOutcome,
    ) -> NewModelCall {
        NewModelCall {
            purpose,
            context,
            session_id: by.session_id,
            run_id: by.run_id,
            model: by.model.clone(),
            model_id: by.model_id,
            reasoning_effort: by.effort,
            started_at: self.started_at,
            first_output_at: self.first_output_at,
            finished_at: self.finished_at,
            stop,
            usage: self.usage,
            outcome,
        }
    }
}

/// 调用行里"谁发起、用哪个模型"的部分。
pub(crate) struct Caller {
    pub session_id: SessionId,
    pub run_id: Option<RunId>,
    pub model: String,
    pub model_id: Option<ModelId>,
    pub effort: Option<ReasoningEffort>,
}

impl Caller {
    pub(crate) fn new(
        session_id: SessionId,
        run_id: Option<RunId>,
        provider: &dyn Provider,
        model_id: Option<ModelId>,
        effort: Option<ReasoningEffort>,
    ) -> Self {
        Self {
            session_id,
            run_id,
            model: provider.model().to_owned(),
            model_id,
            effort,
        }
    }
}

/// 驱动一次流：`forward` 收到正文与推理增量；结束时刻取在 Finished/Err 的那一刻。
/// 事实写进 `facts`，调用方即使丢弃本 future（超时）也能读到已观测部分。
pub(crate) async fn stream_attempt(
    provider: &dyn Provider,
    req: ModelRequest,
    facts: &mut Facts,
    mut forward: impl FnMut(&ModelEvent),
) -> Result<ModelResponse, ProviderError> {
    let mut stream = provider.stream(req);
    let result = loop {
        match poll_fn(|cx| stream.as_mut().poll_next(cx)).await {
            Some(Ok(event)) => match event {
                ModelEvent::Usage(usage) => facts.usage = Some(usage),
                ModelEvent::Finished(response) => break Ok(response),
                ModelEvent::TextDelta(ref t) | ModelEvent::ReasoningDelta(ref t) => {
                    if !t.is_empty() {
                        facts.first_output_at.get_or_insert_with(now_ms);
                    }
                    forward(&event);
                }
                ModelEvent::ToolCallDelta => {
                    facts.first_output_at.get_or_insert_with(now_ms);
                }
            },
            Some(Err(error)) => break Err(error),
            None => {
                break Err(ProviderError::Protocol {
                    message: "stream ended without Finished".into(),
                })
            }
        }
    };
    facts.finished_at = now_ms();
    result
}
