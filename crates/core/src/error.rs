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
}
