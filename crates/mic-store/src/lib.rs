//! 内核事实持久化与模块事务入口。
//! 契约：docs/blueprints/mic-store.md；表目录：schema.rs 的 TABLE_CATALOG。

mod backup;
mod context;
mod diagnostic;
mod endpoints;
mod error;
mod images;
mod jobs;
mod limits;
mod messages;
mod models;
mod people;
mod personas;
mod pushes;
mod row;
mod schedules;
mod scheduling;
mod schema;
mod search;
mod secrets;
mod sessions;
mod settings;
mod store;
mod tags;
mod title;
mod trace;
mod types;

pub use backup::{
    ExportEndpoint, ExportModel, ExportModelRef, ExportPersona, ExportPreferences, SettingsExport,
};
pub use rusqlite;

pub use diagnostic::{Cell, ColumnSchema, DiagnosticError, QueryResult, TableSchema};
pub use error::{ContextCommandError, ModelSettingsError, SettingsError, StoreError};
pub use jobs::{AgentActivity, Job, JobId, JobKind, JobState, NewAgentJob, NewToolJob};
pub use limits::{TAGS_PER_SESSION, TAG_CHARS};
pub use pushes::{NewPush, PushDelivery, PushDeliveryState, PushId, PushPage, PushRoute, PushView};
pub use schedules::{
    Fire, FireId, FireOutcome, FireResult, NewSchedule, Schedule, ScheduleEdit, ScheduleId,
    SCHEDULE_MODULE,
};
pub use search::{MessageHit, SearchPage, SearchTerms, SessionHit};
pub use secrets::{SecretKeyFile, SecretValue};
pub use store::Store;
pub use tags::{is_tag_separator, TagError, TagName};
pub use title::reply_answer;
pub use trace::{
    call_cost, CallRecord, CallStat, Dashboard, DayStat, ModelPeers, ModelStat, Peers, RunStats,
    SessionUsageView, ToolPeers, ToolStat, ToolTraceOutcome, TraceCall, TraceMessage, TraceRun,
    TraceRunPage, TraceSession, TraceSessionKind, TraceTool, TreeUsage, UsageTotal,
};
pub use types::{
    ClaimedModel, ClaimedRun, CompactionSettings, ContextUsage, ContextView, ContextWindow,
    CredentialWrite, DeliveryTarget, EndpointId, EndpointView, EndpointWrite, Identity,
    InputDisposition, InterruptedRun, MessagePage, ModelCallContext, ModelCallId, ModelCallOutcome,
    ModelCallPurpose, ModelId, ModelPrice, ModelView, ModelWrite, NewInputPart, NewModelCall,
    NewNotice, NewSession, PendingDelivery, Person, Persona, PersonaId, ReasoningEffort, Resumable,
    RevocableView, RevocationState, Run, RunId, RunInput, RunSettings, RunState, Schema,
    SecretColumn, SelectedModel, Session, SessionCursor, SessionKind, SessionPage, SessionSummary,
    Settings, StopReason, Tag, TagId, TitleCall, TitleModelSelection, TitleOutput, ToolScope,
    Usage,
};
