//! The REAL app schema (001b-1 migrations) for the bench.
//!
//! The throwaway mirrored schema (`schema.rs`) was deleted in spec
//! 001b-2: the bench now measures the real tables, triggers, and
//! constraints the app actually writes — the `sku`→`item` FK, the
//! `sale_line_tax` rows, the `receipt_counter`, the `print_job`, the
//! `audit_log` hash-chain columns, the FTS5 + catalog-version triggers.
//! `run_migrations` (`shopcore-db`) applies migration 0001; this module
//! adds only the bench-only `bench_scratch` table (scenario 5's load
//! generator — not an app table).

use crate::BenchError;

/// Create the real schema on `conn`: the app pragmas (WAL,
/// `synchronous=FULL`, foreign keys on), the 001b-1 migrations, and the
/// bench-only scratch table.
///
/// # Errors
/// Returns [`BenchError::Sql`] if a pragma or the scratch DDL fails,
/// [`BenchError::Usage`] if a migration fails.
pub fn create_schema(conn: &mut rusqlite::Connection) -> Result<(), BenchError> {
    conn.execute_batch(
        "PRAGMA journal_mode=WAL; \
         PRAGMA synchronous=FULL; \
         PRAGMA foreign_keys=ON; \
         PRAGMA busy_timeout=5000; \
         PRAGMA temp_store=MEMORY;",
    )
    .map_err(BenchError::Sql)?;
    shopcore_db::migrations::run_migrations(conn)
        .map_err(|err| BenchError::Usage(format!("real-schema migrations failed: {err}")))?;
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS bench_scratch (id INTEGER PRIMARY KEY, payload TEXT NOT NULL);",
    )
    .map_err(BenchError::Sql)?;
    Ok(())
}
