//! 内核事实的持久化：core_sessions / core_messages / core_runs / core_model_calls /
//! core_persons / core_person_identities / core_personas / core_settings，外加模块私有表的迁移与事务入口。
//! 契约：docs/blueprints/mic-store.md。

mod error;
mod limits;
mod personas;
mod row;
mod schema;
mod store;
mod types;

pub use rusqlite;

pub use error::{SettingsError, StoreError};
pub use store::Store;
pub use types::{
    ContextWindow, DeliveryTarget, Identity, Migration, ModelCallId, ModelCallOutcome,
    NewModelCall, NewSession, PendingDelivery, Person, Persona, PersonaId, Run, RunId, RunSettings,
    RunState, Session, SessionCursor, SessionKind, SessionPage, SessionSummary, Settings,
    ToolScope, Usage,
};
