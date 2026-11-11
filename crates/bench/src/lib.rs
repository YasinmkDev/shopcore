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

pub mod real_schema;
pub mod report;
pub mod scenarios;
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
        crate::real_schema::create_schema(&mut first).expect("schema");
        let tiny = crate::seed::DatasetSize::tiny();
        crate::seed::seed(&mut first, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        let first_sum = crate::seed::checksum(&first).expect("checksum");

        let mut second = rusqlite::Connection::open_in_memory().expect("open");
        crate::real_schema::create_schema(&mut second).expect("schema");
        crate::seed::seed(&mut second, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        let second_sum = crate::seed::checksum(&second).expect("checksum");

        assert_eq!(
            first_sum, second_sum,
            "same seed must build the same dataset"
        );
    }

    // Regression test for the "database is locked" flake (spec 002,
    // found 2026-11-10: scenario 5 failed ~3 in 8 smoke runs).
    // Root cause, in two parts:
    // (a) `open_db` ran `PRAGMA journal_mode=WAL` on every open. That
    // statement does NOT honor `busy_timeout` — it fails instantly with
    // SQLITE_BUSY if another connection holds a lock — and the timeout
    // was set after it, so nothing waited. `open_db` now verifies the
    // mode instead of changing it (the seeder creates every bench DB
    // in WAL mode) and sets `busy_timeout` first.
    // (b) Scenario 5's background writer did 200 autocommit INSERTs per
    // batch. Each autocommit INSERT fsyncs (synchronous=FULL), so the
    // writer held/re-acquired the write lock for seconds on a slow
    // disk and starved the main thread's commit past its 5 s
    // busy_timeout. The writer now uses one transaction per batch.
    // This test proves (a): with a write lock held on another
    // connection, `open_db` on a WAL database must succeed (not fail
    // with SQLITE_BUSY), and on a non-WAL database it must fail with
    // the clear WAL-mode error, never with "database is locked".
    #[test]
    fn open_db_waits_for_brief_write_lock() {
        use std::sync::mpsc;
        let dir = std::env::temp_dir().join(format!(
            "bench-lock-test-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        // Case 1: WAL database, write lock held -> open_db must succeed.
        let path = dir.join("lock-wal.db");
        {
            let conn = rusqlite::Connection::open(&path).expect("open");
            conn.execute_batch("PRAGMA journal_mode=WAL; CREATE TABLE t(x);")
                .expect("create");
        }
        let (tx, rx) = mpsc::channel();
        let path2 = path.clone();
        let holder = std::thread::spawn(move || {
            let mut conn = rusqlite::Connection::open(&path2).expect("open");
            conn.execute_batch("PRAGMA busy_timeout=5000;")
                .expect("timeout");
            let sql_tx = conn.transaction().expect("begin");
            sql_tx
                .execute("INSERT INTO t VALUES (1)", [])
                .expect("insert");
            tx.send(()).expect("signal");
            std::thread::sleep(std::time::Duration::from_millis(300));
            sql_tx.commit().expect("commit");
        });
        rx.recv().expect("holder signal");
        let result = crate::scenarios::open_db(&path);
        holder.join().expect("holder thread");
        assert!(
            result.is_ok(),
            "open_db on a WAL database must not fail while a write lock \
             is briefly held: {:?}",
            result.err()
        );
        // Case 2: non-WAL database, write lock held -> the WAL-mode
        // error, never SQLITE_BUSY.
        let path3 = dir.join("lock-rollback.db");
        {
            let conn = rusqlite::Connection::open(&path3).expect("open");
            conn.execute_batch("CREATE TABLE t(x);").expect("create");
        }
        let (tx2, rx2) = mpsc::channel();
        let path4 = path3.clone();
        let holder2 = std::thread::spawn(move || {
            let mut conn = rusqlite::Connection::open(&path4).expect("open");
            conn.execute_batch("PRAGMA busy_timeout=5000;")
                .expect("timeout");
            let sql_tx = conn.transaction().expect("begin");
            sql_tx
                .execute("INSERT INTO t VALUES (1)", [])
                .expect("insert");
            tx2.send(()).expect("signal");
            std::thread::sleep(std::time::Duration::from_millis(300));
            sql_tx.commit().expect("commit");
        });
        rx2.recv().expect("holder signal");
        let result2 = crate::scenarios::open_db(&path3);
        holder2.join().expect("holder thread");
        let is_wal_mode_error = matches!(&result2, Err(crate::BenchError::Usage(msg)) if msg.contains("not in WAL mode"));
        assert!(
            is_wal_mode_error,
            "open_db on a non-WAL database must return the WAL-mode \
             Usage error, never SQLITE_BUSY: {result2:?}"
        );
        std::fs::remove_dir_all(&dir).ok();
    }

    // Regression test for part (b): scenario 5 must complete on a
    // tiny dataset even with the background writer/reader running.
    // The old autocommit writer starved the main commit ~3 in 8 runs
    // on this VM; three consecutive passes make a recurrence unlikely.
    // The test database MUST live on a real disk (the crate's
    // target/, not /tmp): /tmp is tmpfs here, where an fsync is free
    // (200 autocommit INSERTs = 0.002 s), so the starvation never
    // happens there. On the real disk the same 200 INSERTs take
    // ~2.5 s (one fsync each), which is what starved the main commit.
    #[test]
    fn scenario_sale_commit_under_load_succeeds() {
        let dir = std::path::PathBuf::from(
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR"),
        )
        .join("target")
        .join("bench-test-dbs")
        .join(format!(
            "load-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_or(0, |d| d.as_nanos())
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let path = dir.join("load.db");
        {
            let mut conn = rusqlite::Connection::open(&path).expect("open");
            crate::real_schema::create_schema(&mut conn).expect("schema");
            let tiny = crate::seed::DatasetSize::tiny();
            crate::seed::seed(&mut conn, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        }
        for run in 0..3 {
            let result = crate::scenarios::scenario_sale_commit_under_load(&path, 9_000_000 + run);
            assert!(
                result.is_ok(),
                "scenario 5 run {run} must not fail with 'database is locked': {:?}",
                result.err()
            );
        }
        std::fs::remove_dir_all(&dir).ok();
    }

    // Proof that a fresh seed is the recorded dataset: seed the smoke
    // size (10k SKUs) with the default seed and compare the checksum
    // to the value recorded when the harness was built. If the seeder
    // changes, this test fails loudly and the recorded value must be
    // re-issued deliberately (never edited to match a broken seed).
    // Re-issued 2026-11-11 (spec 001b-2 real-schema switch): the
    // `customer` RNG draw was removed (no `customer` table in the real
    // schema), so the stream — and the sale/line sums — changed.
    // Recorded 2026-11-10 from a fresh smoke seed (superseded).
    #[test]
    fn smoke_seed_checksum_matches_recorded() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        crate::real_schema::create_schema(&mut conn).expect("schema");
        let smoke = crate::seed::DatasetSize::smoke();
        crate::seed::seed(&mut conn, &smoke, crate::seed::DEFAULT_SEED).expect("seed");
        let sum = crate::seed::checksum(&conn).expect("checksum");
        assert_eq!(
            sum,
            "sku=10000,barcode=12000,sale=24000,\
             sale_line=108019,stock_movement=108019,\
             sale_sum=806957603,line_sum=806957603",
            "fresh smoke seed checksum must match the recorded dataset"
        );
    }

    // The verify step must pass on a freshly seeded dataset: every
    // per-SKU balance equals the sum of that SKU's movements.
    #[test]
    fn verify_passes_on_fresh_seed() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        crate::real_schema::create_schema(&mut conn).expect("schema");
        let tiny = crate::seed::DatasetSize::tiny();
        crate::seed::seed(&mut conn, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        let report = crate::seed::verify(&conn).expect("verify");
        assert!(
            report.sale_line_sums_match,
            "sale_sum must equal line_sum on a fresh seed"
        );
        assert_eq!(
            report.balance_mismatches, 0,
            "no per-SKU balance may differ from its movements"
        );
        assert_eq!(
            report.movements_without_balance, 0,
            "every moved SKU must have a balance row"
        );
    }

    // Resume: re-seeding a complete dataset is a no-op (same checksum).
    #[test]
    fn seed_resume_is_idempotent() {
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        crate::real_schema::create_schema(&mut conn).expect("schema");
        let tiny = crate::seed::DatasetSize::tiny();
        crate::seed::seed(&mut conn, &tiny, crate::seed::DEFAULT_SEED).expect("seed");
        let first = crate::seed::checksum(&conn).expect("checksum");
        crate::seed::seed(&mut conn, &tiny, crate::seed::DEFAULT_SEED).expect("re-seed");
        let second = crate::seed::checksum(&conn).expect("checksum");
        assert_eq!(
            first, second,
            "re-seeding a complete dataset must not change it"
        );
    }

    // Resume: seed 10k sales, then resume to 10.5k — the resumed
    // dataset must equal a fresh seed of the same size (same checksum).
    // `movements: 0` in both sizes: the receipts phase is skipped, so
    // the only movements are the sale lines' (one per line). The old
    // DELETE-based interruption simulation cannot work on the real
    // schema: the append-only triggers forbid deleting completed
    // sales, so a partial dataset is built by seeding fewer sales.
    #[test]
    fn seed_resume_completes_partial_dataset() {
        let partial = crate::seed::DatasetSize {
            skus: 100,
            active_skus: 20,
            barcodes: 120,
            sales: 10_000,
            sale_lines: 47_000,
            movements: 0,
        };
        let size = crate::seed::DatasetSize {
            sales: 10_500,
            ..partial
        };
        // Reference: a fresh, uninterrupted seed of the full size.
        let mut reference = rusqlite::Connection::open_in_memory().expect("open");
        crate::real_schema::create_schema(&mut reference).expect("schema");
        crate::seed::seed(&mut reference, &size, crate::seed::DEFAULT_SEED).expect("seed");
        let expected = crate::seed::checksum(&reference).expect("checksum");
        // Interrupted copy: seed only the partial size, then resume.
        let mut conn = rusqlite::Connection::open_in_memory().expect("open");
        crate::real_schema::create_schema(&mut conn).expect("schema");
        crate::seed::seed(&mut conn, &partial, crate::seed::DEFAULT_SEED).expect("seed");
        crate::seed::seed(&mut conn, &size, crate::seed::DEFAULT_SEED).expect("resume");
        let got = crate::seed::checksum(&conn).expect("checksum");
        assert_eq!(
            got, expected,
            "a resumed seed must produce the identical dataset (same checksum)"
        );
        // And the resumed dataset must verify (per-SKU balances match).
        let report = crate::seed::verify(&conn).expect("verify");
        assert_eq!(
            report.balance_mismatches, 0,
            "balances must match movements"
        );
        assert_eq!(
            report.movements_without_balance, 0,
            "every moved SKU must have a balance row"
        );
    }
}
