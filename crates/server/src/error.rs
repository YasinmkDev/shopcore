//! Server errors (spec 005). No panics, no unwraps in lib code.

use thiserror::Error;

/// Errors the sync server can return.
#[derive(Debug, Error)]
pub enum ServerError {
    /// Database error (Postgres via sqlx, or SQLite for snapshots).
    #[error("database error: {0}")]
    Database(#[from] sqlx::Error),

    /// SQLite error while building a snapshot.
    #[error("snapshot sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),

    /// I/O error (snapshot file write, etc.).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// The batch failed validation (wrong version, too many rows, bad JSON).
    #[error("invalid batch: {0}")]
    InvalidBatch(String),

    /// The revalidation stub found a sale whose line totals do not sum to
    /// the sale total. Flagged, never altered.
    #[error("revalidation mismatch: lines sum to {actual} minor, sale total is {expected} minor")]
    RevalidationMismatch {
        /// The sale total the till recorded.
        expected: i64,
        /// The sum of the line totals.
        actual: i64,
    },

    /// The revalidation stub overflowed while summing line totals.
    #[error("revalidation overflow while summing line totals")]
    RevalidationOverflow,

    /// The requested stream does not exist.
    #[error("unknown stream: {0}")]
    UnknownStream(String),

    /// The snapshot for this tenant/branch does not exist (not built yet).
    #[error("snapshot not found for tenant {0} branch {1}")]
    SnapshotNotFound(uuid::Uuid, uuid::Uuid),
}
