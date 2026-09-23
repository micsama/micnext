//! 内核事实的持久化：sessions / session_entries / queries / persons /
//! person_identities / model_calls，外加模块私有表的迁移与事务入口。
//! 契约：docs/blueprints/mic-store.md。

mod error;
mod row;
mod schema;
mod store;
mod types;

pub use rusqlite;

pub use error::StoreError;
pub use store::Store;
pub use types::{
    BoundaryInput, CancelReason, CompletionInput, ContextWindow, DeliveryTarget, FailureReason,
    Identity, Migration, ModelCallInput, ModelCallOutcome, ModelCallPurpose, NewSession,
    OutputInput, PendingDelivery, Person, Query, QueryId, QueryState, Session, SessionKind,
    ToolScope, Usage, UserInput,
};
