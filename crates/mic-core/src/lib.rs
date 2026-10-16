//! 模块装配框架、内核句柄、模型 port 与执行主路径（调度、Agent 循环、崩溃收尾、实时事件）。
//! 不依赖具体 Channel。契约：docs/blueprints/mic-core-module.md。

mod assembly;
mod attempt;
mod channel_setup;
mod command;
mod compaction;
mod context_usage;
mod control;
mod error;
mod event;
mod input;
mod jobs;
mod kernel;
mod limits;
mod module;
mod provider;
mod recovery;
mod request;
mod run;
mod scheduler;
mod title;

pub use assembly::{Assembly, OnceOutcome, OneShot};
pub use channel_setup::{
    ChannelConnection, ChannelRestore, ChannelSetup, ChannelSetupError, ChannelSetupView,
    LinkedChannel, SetupAttempt, SetupAttemptId, SetupFailure, SetupProgress,
};
pub use command::{parse_command, session_commands, CommandParseError, CommandSpec, ParsedCommand};
pub use compaction::Occupancy;
pub use context_usage::{ContextBreakdown, ContextKind, ContextPart};
pub use error::{AssembleError, KernelError, RunError, WorkdirError};
pub use event::{EventReceiver, KernelEvent, KernelEventKind, Lagged};
pub use input::{IncomingPart, InputError};
pub use jobs::{JobAccessError, JobOutput, JobView, StopJobOutcome};
pub use kernel::{ContextOverview, EndpointDraft, Kernel, ModelDraft, SettingsBackup, StopOutcome};
pub use mic_media::MediaError;
pub use mic_store::SecretValue;
pub use mic_store::{JobId, JobState};
pub use module::{Activation, BoxError, Module, ModuleConfig, Registry, Service};
pub use provider::{
    BoxFuture, BoxStream, ConfigError, ModelEvent, ModelRequest, ModelResponse, ProbeError,
    Provider, ProviderError, ProviderFactory, ProviderKindView, ReasoningOptions, StopReason,
    ToolChoice,
};
pub use request::{PromptBlock, PromptBlockKind, PromptPart, PromptPartKind, SystemPrompt};
pub use title::TitleError;
