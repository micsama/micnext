//! 内核事实持久化与模块迁移事务入口。
//! 契约：docs/blueprints/mic-store.md；表目录：schema.rs 的 TABLE_CATALOG。

mod backup;
mod context;
mod diagnostic;
mod endpoints;
mod error;
mod images;
mod limits;
mod messages;
mod models;
mod people;
mod personas;
mod row;
mod scheduling;
mod schema;
mod secrets;
mod sessions;
mod settings;
mod store;
mod title;
mod trace;
mod types;

pub use backup::{
    ExportEndpoint, ExportModel, ExportModelRef, ExportPersona, ExportPreferences, SettingsExport,
};
pub use rusqlite;

pub use diagnostic::{Cell, ColumnSchema, DiagnosticError, QueryResult, TableSchema};
pub use error::{ContextCommandError, ModelSettingsError, SettingsError, StoreError};
pub use secrets::{SecretKeyFile, SecretValue};
pub use store::Store;
pub use title::reply_answer;
pub use trace::{
    call_cost, CallRecord, CallStat, ChatRunStats, Dashboard, DayStat, ModelPeers, ModelStat,
    Peers, RunStats, ToolPeers, ToolStat, ToolTraceOutcome, TraceCall, TraceMessage, TraceRun,
    TraceRunPage, TraceSession, TraceTool,
};
pub use types::{
    ClaimedModel, ClaimedRun, CompactionSettings, CompactionState, CompactionView, ContextUsage,
    ContextView, ContextWindow, CredentialWrite, DeliveryTarget, EndpointId, EndpointView,
    EndpointWrite, Identity, InputDisposition, MessagePage, Migration, ModelCallContext,
    ModelCallId, ModelCallOutcome, ModelCallPurpose, ModelId, ModelPrice, ModelView, ModelWrite,
    NewInputPart, NewModelCall, NewNotice, NewSession, PendingDelivery, Person, Persona, PersonaId,
    ReasoningEffort, Run, RunId, RunInput, RunSettings, RunState, SecretColumn, Session,
    SessionCursor, SessionKind, SessionPage, SessionSummary, Settings, StopReason, TitleCall,
    TitleModelSelection, ToolScope, Usage,
};
