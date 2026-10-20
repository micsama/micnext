//! 推送：发给 person 的消息，按 Channel 投递。契约：docs/blueprints/scheduled-tasks.md P1。

use std::any::Any;
use std::collections::HashMap;
use std::sync::Arc;

use mic_message::{ContentPart, PersonId, SessionId};
use mic_store::{NewPush, PushId, PushPage, PushRoute, RunId, Store, StoreError, WEB_CHANNEL};
use mic_tool::{Tool, ToolContext, ToolError, ToolHandle, ToolOutput};
use schemars::JsonSchema;
use serde::Deserialize;

use crate::event::{Events, KernelEventKind};
use crate::jobs::JobPort;
use crate::kernel::session_channel;
use crate::run::now_ms;
use crate::{ChannelConnection, ChannelSetup, ChannelSetupError};

pub(crate) struct Pushes {
    store: Store,
    events: Events,
    channel_setups: Arc<HashMap<&'static str, Arc<dyn ChannelSetup>>>,
    owner: PersonId,
}

/// 推送的发起方。
pub(crate) struct PushSource {
    pub(crate) session: SessionId,
    pub(crate) run: Option<RunId>,
}

#[derive(Debug, thiserror::Error)]
pub enum PushError {
    #[error("没有名为「{0}」的推送渠道")]
    UnknownChannel(String),
    #[error("推送渠道不可用：{}", .0.iter().map(|(c, r)| format!("{c}（{r}）")).collect::<Vec<_>>().join("、"))]
    Unavailable(Vec<(String, String)>),
    #[error("推送正文为空")]
    EmptyText,
    #[error(transparent)]
    Store(#[from] StoreError),
    #[error(transparent)]
    Setup(#[from] ChannelSetupError),
}

/// 一个 Channel 当前能否投递。
enum Route {
    Ready(PushRoute),
    Down(String),
}

impl Pushes {
    pub(crate) fn new(
        store: Store,
        events: Events,
        channel_setups: Arc<HashMap<&'static str, Arc<dyn ChannelSetup>>>,
        owner: PersonId,
    ) -> Self {
        Self {
            store,
            events,
            channel_setups,
            owner,
        }
    }

    /// `web` 加各个有登录流程的 Channel；IM 发往其绑定账号的私聊会话。
    pub(crate) fn channels(&self) -> Vec<&'static str> {
        let mut names: Vec<&'static str> = self.channel_setups.keys().copied().collect();
        names.sort_unstable();
        names.insert(0, WEB_CHANNEL);
        names
    }

    /// 用户未设时：有可用的 IM 就用它们，否则 Web。
    pub(crate) async fn default_channels(&self) -> Result<Vec<String>, PushError> {
        let chosen = self.store.push_channels().await?;
        if !chosen.is_empty() {
            return Ok(chosen);
        }
        let mut ready = Vec::new();
        for channel in self.channels() {
            if channel != WEB_CHANNEL && matches!(self.route(channel).await?, Route::Ready(_)) {
                ready.push(channel.to_owned());
            }
        }
        if ready.is_empty() {
            ready.push(WEB_CHANNEL.to_owned());
        }
        Ok(ready)
    }

    pub(crate) async fn set_default_channels(
        &self,
        channels: Vec<String>,
    ) -> Result<(), PushError> {
        let known = self.channels();
        if let Some(bad) = channels.iter().find(|c| !known.contains(&c.as_str())) {
            return Err(PushError::UnknownChannel(bad.clone()));
        }
        Ok(self.store.set_push_channels(channels).await?)
    }

    /// agent 推送：`via` 中任一 Channel 不可用即整条不发。返回实际去向。
    pub(crate) async fn send(
        &self,
        source: PushSource,
        text: String,
        via: Option<Vec<String>>,
    ) -> Result<Vec<String>, PushError> {
        if text.trim().is_empty() {
            return Err(PushError::EmptyText);
        }
        let mut via = match via {
            Some(via) => via,
            None => self.default_channels().await?,
        };
        via.dedup();
        let known = self.channels();
        let mut routes = Vec::new();
        let mut down = Vec::new();
        for channel in &via {
            if !known.contains(&channel.as_str()) {
                return Err(PushError::UnknownChannel(channel.clone()));
            }
            match self.route(channel).await? {
                Route::Ready(route) => routes.push(route),
                Route::Down(reason) => down.push((channel.clone(), reason)),
            }
        }
        if !down.is_empty() {
            return Err(PushError::Unavailable(down));
        }
        self.create(source, text, routes).await?;
        Ok(via)
    }

    pub(crate) async fn page(
        &self,
        before: Option<PushId>,
        limit: u32,
    ) -> Result<PushPage, StoreError> {
        self.store.pushes(self.owner, before, limit).await
    }

    pub(crate) async fn mark_read(&self, ids: Vec<PushId>) -> Result<(), StoreError> {
        self.store.mark_pushes_read(self.owner, ids, now_ms()).await
    }

    async fn route(&self, channel: &str) -> Result<Route, PushError> {
        if channel == WEB_CHANNEL {
            return Ok(Route::Ready(PushRoute::Web));
        }
        let setup = &self.channel_setups[channel];
        let status = match setup.status().await {
            Ok(status) => status,
            Err(ChannelSetupError::Unavailable) => return Ok(Route::Down("未运行".into())),
            Err(e) => return Err(e.into()),
        };
        let Some(account) = status.account else {
            return Ok(Route::Down("未登录".into()));
        };
        Ok(match account.connection {
            ChannelConnection::Connected => Route::Ready(PushRoute::Session {
                channel: channel.to_owned(),
                session: account.session_id,
            }),
            ChannelConnection::NeedsLogin => Route::Down("需要重新登录".into()),
            ChannelConnection::Faulted => Route::Down("连接故障".into()),
        })
    }

    async fn create(
        &self,
        source: PushSource,
        text: String,
        routes: Vec<PushRoute>,
    ) -> Result<PushId, PushError> {
        let session = self
            .store
            .session(source.session)
            .await?
            .expect("推送来源会话存在（调用方保证）");
        let channel = session_channel(&self.store, &session).await?;
        let title = self
            .store
            .session_summary(source.session)
            .await?
            .and_then(|s| s.title);
        let source_label = match title {
            Some(title) => format!("push from session {} ({title})", source.session.0),
            None => format!("push from session {}", source.session.0),
        };
        let publish = self.events.publisher().await;
        let (id, notices) = self
            .store
            .create_push(
                NewPush {
                    person: self.owner,
                    source_session: source.session,
                    source_run: source.run,
                    source_label,
                    text,
                    routes,
                },
                now_ms(),
            )
            .await?;
        for notice in notices {
            let target = self
                .store
                .session(notice.session_id)
                .await?
                .expect("私聊会话存在（刚写入）");
            let target_channel = session_channel(&self.store, &target).await?;
            publish.appended(notice.session_id, &target_channel, notice);
        }
        drop(publish);
        self.events.emit(
            source.session,
            &channel,
            KernelEventKind::PushCreated { push_id: id },
        );
        Ok(id)
    }
}

pub(crate) fn handle(pushes: &Arc<Pushes>) -> ToolHandle {
    ToolHandle::new(PushTool(pushes.clone()))
}

struct PushTool(Arc<Pushes>);

#[derive(Deserialize, JsonSchema)]
#[serde(deny_unknown_fields)]
struct PushArgs {
    /// The message for the user, self-contained: they may read it away from this conversation.
    text: String,
    /// Channels to send through: "web" (the Web notification center) or a connected IM such as "wechat". Omit to use the user's default channels.
    via: Option<Vec<String>>,
}

impl Tool for PushTool {
    type Args = PushArgs;

    fn name(&self) -> &str {
        "push"
    }

    fn description(&self) -> &str {
        "Proactively notify the user on their own channels (e.g. WeChat, the Web notification center). Use it only when the user may not be watching this conversation and should be told now, such as a long task they asked to be reminded about has finished. Ordinary replies already reach the user here; do not push them."
    }

    fn prompt_hint(&self) -> Option<&str> {
        None
    }

    async fn execute(&self, args: PushArgs, ctx: &ToolContext) -> Result<ToolOutput, ToolError> {
        let port = (ctx.background() as &dyn Any)
            .downcast_ref::<JobPort>()
            .ok_or_else(|| ToolError::business("push is not available here"))?;
        let origin = port.origin();
        let source = PushSource {
            session: origin.session,
            run: Some(origin.run),
        };
        let via = self
            .0
            .send(source, args.text, args.via)
            .await
            .map_err(|e| match e {
                PushError::UnknownChannel(c) => ToolError::input(format!(
                    "unknown channel \"{c}\"; available: {}",
                    self.0.channels().join(", ")
                )),
                PushError::EmptyText => ToolError::input("text must not be empty"),
                PushError::Unavailable(down) => ToolError::business(format!(
                    "not sent; unavailable channels: {}",
                    down.iter()
                        .map(|(c, r)| format!("{c} ({r})"))
                        .collect::<Vec<_>>()
                        .join(", ")
                )),
                PushError::Store(e) => ToolError::dependency(e.to_string()),
                PushError::Setup(e) => ToolError::dependency(e.to_string()),
            })?;
        Ok(ToolOutput::Completed(vec![ContentPart::Text {
            text: format!("Pushed via {}.", via.join(", ")),
        }]))
    }
}
