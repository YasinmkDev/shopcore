//! Errors for the `core` crate.

use crate::pin::PinError;
use crate::search::SearchError;

/// Errors from the core use cases.
#[derive(Debug, thiserror::Error)]
pub enum CoreError {
    /// A database error (actor, migration, or query).
    #[error("database error: {0}")]
    Db(#[from] shopcore_db::DbError),

    /// A PIN error (verification, lockout, manager approval).
    #[error("pin error: {0}")]
    Pin(#[from] PinError),

    /// A search/lookup error.
    #[error("search error: {0}")]
    Search(#[from] SearchError),

    /// The requested row does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// The caller passed an invalid argument.
    #[error("invalid input: {0}")]
    InvalidInput(String),

    /// The audit hash chain is broken at this row (a field was changed
    /// after writing, or the `prev_hash` link does not match the
    /// previous row). Reported by the verify use case, never silently
    /// repaired.
    #[error("audit chain broken at row {row_id}: {detail}")]
    AuditChain {
        /// The id of the first row whose link does not verify.
        row_id: String,
        /// What does not match.
        detail: String,
    },
}

// A `SQLite` failure is a database error: map it to the `Db` variant
// directly, so `?` on a `rusqlite` call works in one step inside the
// core use cases (the `#[from]` on `Db` only covers `DbError`).
impl From<rusqlite::Error> for CoreError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Db(shopcore_db::DbError::Sql(err))
    }
}
