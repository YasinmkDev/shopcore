//! `ShopCore` benchmarks: seeds data (`just seed-large`: 500k `SKUs`, `5M` sale
//! lines) and runs the budgets in `context/05-performance-budgets.md`
//! with a scenario runner + hdrhistogram percentiles (median of 5 runs).
//! `just bench` on the full dataset is the manual Phase 0 gate (target PC);
//! CI runs a 10k-row smoke dataset as a relative regression tripwire only.
//!
//! Spec 002: the seeder + scenario runner live here. The scenario runner
//! measures the budgets with hdrhistogram percentiles — NOT criterion
//! (spec 002 review, 2026-11-10: criterion is the wrong tool for
//! end-to-end scenarios).

pub mod report;
pub mod scenarios;
pub mod schema;
pub mod seed;

/// Errors from the bench harness.
#[derive(Debug, thiserror::Error)]
pub enum BenchError {
    /// SQLite failure.
    #[error("sqlite error: {0}")]
    Sql(#[from] rusqlite::Error),
    /// Filesystem failure.
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),
    /// JSON failure.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
    /// Histogram creation failure.
    #[error("histogram creation failed: {0}")]
    HistogramCreation(#[from] hdrhistogram::CreationError),
    /// Histogram record failure.
    #[error("histogram record failed: {0}")]
    HistogramRecord(#[from] hdrhistogram::RecordError),
    /// Usage error (bad flags, missing database).
    #[error("{0}")]
    Usage(String),
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    // Gate test (spec 002, amendment 6): FTS5 MUST be available in the
    // SQLCipher-bundled SQLite build. Scenario 2 (name search) depends on
    // it. If this test fails, STOP and report — do not work around it.
    #[test]
    fn fts5_available_in_bundled_sqlcipher() {
        let conn = rusqlite::Connection::open_in_memory().expect("in-memory DB must open");
        let created = conn.execute_batch(
            "CREATE VIRTUAL TABLE fts5_gate USING fts5(name, code); \
             INSERT INTO fts5_gate (name, code) VALUES ('Rice 5kg', 'RICE5'); \
             INSERT INTO fts5_gate (name, code) VALUES ('Milk 1L', 'MILK1');",
        );
        assert!(
            created.is_ok(),
            "FTS5 NOT SUPPORTED in the SQLCipher-bundled SQLite build: {:?}. \
             STOP — spec 002 scenario 2 (FTS5 name search) cannot run. Do not \
             work around this; report it.",
            created.err()
        );
        let count: i64 = conn
            .query_row(
                "SELECT COUNT(*) FROM fts5_gate WHERE fts5_gate MATCH 'Rice*'",
                [],
                |row| row.get(0),
            )
            .expect("FTS5 prefix query must run");
        assert_eq!(count, 1, "FTS5 prefix search must return the seeded row");
    }

    #[test]
    fn seeder_is_deterministic() {
        let mut first = rusqlite::Connection::open_in_memory().expect("open");
        crate::schema::create_schema(&first).expect("schema");
        let tiny = crate::seed::DatasetSize::tiny();
        crate::seed::seed(&mut first, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        let first_sum = crate::seed::checksum(&first).expect("checksum");

        let mut second = rusqlite::Connection::open_in_memory().expect("open");
        crate::schema::create_schema(&second).expect("schema");
        crate::seed::seed(&mut second, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        let second_sum = crate::seed::checksum(&second).expect("checksum");

        assert_eq!(
            first_sum, second_sum,
            "same seed must build the same dataset"
        );
    }
}
