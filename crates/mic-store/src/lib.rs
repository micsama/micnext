//! 内核事实的持久化：core_sessions / core_messages / core_runs / core_model_calls /
//! core_persons / core_person_identities / core_personas / core_settings，外加模块私有表的迁移与事务入口。
//! 契约：docs/blueprints/mic-store.md。

mod endpoints;
mod error;
mod images;
mod limits;
mod models;
mod personas;
mod row;
mod schema;
mod secrets;
mod store;
mod types;

pub use rusqlite;

pub use error::{ModelSettingsError, SettingsError, StoreError};
pub use secrets::{SecretKeyFile, SecretValue};
pub use store::Store;
pub use types::{
    ClaimedModel, ClaimedRun, ContextWindow, CredentialWrite, DeliveryTarget, EndpointId,
    EndpointView, EndpointWrite, Identity, Migration, ModelCallId, ModelCallOutcome, ModelId,
    ModelView, ModelWrite, NewInputPart, NewModelCall, NewSession, PendingDelivery, Person,
    Persona, PersonaId, Run, RunId, RunSettings, RunState, Session, SessionCursor, SessionKind,
    SessionPage, SessionSummary, Settings, ToolScope, Usage,
};
