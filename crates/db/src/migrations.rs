//! Forward-only migrations (spec 001b-1 req. 1, ADR-031).
//!
//! Migrations are embedded `SQL` files, applied in version order. Each
//! applied version is recorded in `schema_migrations`, so a re-run is a
//! no-op for already-applied versions. Migrations may be squashed or
//! rewritten until the first customer install (ADR-031); after that they
//! are strictly forward-only — never edited, only new numbered files.

use rusqlite::Connection;

use crate::DbError;

/// The embedded migrations, in version order. Add a new entry (never edit
/// an existing one after shipping) when a spec needs a schema change.
pub const MIGRATIONS: &[(u32, &str)] = &[(
    1,
    include_str!("../migrations/0001_checkout_foundation.sql"),
)];

/// Apply every not-yet-applied migration to `conn`, in order. Each
/// migration runs inside its own transaction; the version row is written
/// in the same transaction, so a failed migration leaves no trace.
/// Idempotent: re-running skips applied versions.
///
/// # Errors
///
/// Returns [`DbError::Migration`] if a migration cannot start, apply, or
/// commit, and [`DbError::Sql`] for any other `SQLite` failure.
pub fn run_migrations(conn: &mut Connection) -> Result<(), DbError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS schema_migrations (
           version INTEGER PRIMARY KEY,
           applied_at INTEGER NOT NULL
         )",
    )?;
    for (version, sql) in MIGRATIONS {
        let applied: bool = conn.query_row(
            "SELECT EXISTS(SELECT 1 FROM schema_migrations WHERE version = ?1)",
            rusqlite::params![version],
            |row| row.get(0),
        )?;
        if applied {
            continue;
        }
        let tx = conn.transaction().map_err(|e| {
            DbError::Migration(format!(
                "migration {version}: could not start transaction: {e}"
            ))
        })?;
        tx.execute_batch(sql)
            .map_err(|e| DbError::Migration(format!("migration {version}: {e}")))?;
        tx.execute(
            "INSERT INTO schema_migrations (version, applied_at) VALUES (?1, ?2)",
            rusqlite::params![version, now_ms()],
        )
        .map_err(|e| DbError::Migration(format!("migration {version}: {e}")))?;
        tx.commit().map_err(|e| {
            DbError::Migration(format!("migration {version}: could not commit: {e}"))
        })?;
    }
    Ok(())
}

/// Current time as epoch milliseconds (UTC). Used for migration bookkeeping
/// only; business timestamps come from the injected clock at the edges.
fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0))
}
