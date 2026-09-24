//! 内核事实的持久化：core_sessions / core_messages / core_runs / core_model_calls /
//! core_persons / core_person_identities，外加模块私有表的迁移与事务入口。
//! 契约：docs/blueprints/mic-store.md。

mod error;
mod limits;
mod row;
mod schema;
mod store;
mod types;

pub use rusqlite;

pub use error::StoreError;
pub use store::Store;
pub use types::{
    ContextWindow, DeliveryTarget, Identity, Migration, ModelCallId, ModelCallOutcome,
    NewModelCall, NewSession, PendingDelivery, Person, Run, RunId, RunState, Session,
    SessionCursor, SessionKind, SessionPage, SessionSummary, ToolScope, Usage,
};
