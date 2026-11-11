//! Errors for the `db` crate.

/// Errors that can come out of the `DB` actor or the migration runner.
#[derive(Debug, thiserror::Error)]
pub enum DbError {
    /// An underlying `SQLite` error.
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),

    /// The actor thread is gone (it panicked, or the `Db` was dropped).
    #[error("db actor stopped")]
    ActorStopped,

    /// A migration could not be applied.
    #[error("migration failed: {0}")]
    Migration(String),

    /// A query expected exactly one row and found none.
    #[error("row not found: {0}")]
    NotFound(String),

    /// A caller passed an invalid argument (bad path, bad key, bad input).
    #[error("invalid input: {0}")]
    InvalidInput(String),
}
