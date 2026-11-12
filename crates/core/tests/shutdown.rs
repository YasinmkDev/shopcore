//! Teardown stress test (ADR-036, owner fix 2026-11-12).
//!
//! The sale and crash binaries segfaulted (SIGSEGV) during teardown:
//! the `Db` actor thread and the `ScratchDb` checkpoint timer thread
//! were detached (never joined), and the test directories were deleted
//! while those threads were still closing their connections. This test
//! creates and drops `Db` + `ScratchDb` thousands of times in
//! parallel and asserts every shutdown joins cleanly (no stuck
//! thread, no error) — if the teardown regresses to detached threads,
//! this test fails (or segfaults) instead of the suite.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{test_root_001b2, unique_test_dir};
use shopcore_core::scratch::ScratchDb;
use shopcore_db::Db;
use std::sync::OnceLock;

/// The `SQLCipher` test key (same as the sale tests — the segfault hit
/// the encrypted file-backed WAL path, so the stress test uses it).
const TEST_KEY: &[u8] = b"test-key-32-bytes-long-0123456789";

/// A pre-migrated template DB file, built once per test binary (the
/// migrations are the slow part; copying the file per iteration keeps
/// the stress test fast). The template `Db` is shut down after
/// building, so no thread outlives it.
fn template_db_path() -> std::path::PathBuf {
    static TEMPLATE: OnceLock<std::path::PathBuf> = OnceLock::new();
    TEMPLATE
        .get_or_init(|| {
            let dir = unique_test_dir(&test_root_001b2(), "shutdown-template");
            let path = dir.join("till.db");
            let db = Db::open(&path, Some(TEST_KEY)).unwrap();
            db.shutdown().unwrap();
            drop(db);
            path
        })
        .clone()
}

/// One create/drop cycle: copy the template, open a `Db` + a
/// `ScratchDb`, do a write through each (so the actor and timer
/// threads are active), shut both down cleanly, then delete the dir.
/// Asserts both shutdowns return `Ok` (clean join, no stuck thread).
fn one_cycle(label: &str) {
    let dir = unique_test_dir(&test_root_001b2(), label);
    std::fs::copy(template_db_path(), dir.join("till.db")).unwrap();
    let db = Db::open(dir.join("till.db"), Some(TEST_KEY)).unwrap();
    let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
    // A write through each, so the actor thread and the timer thread
    // have real work (and the timer is re-armed).
    db.write(|conn| {
        conn.execute(
            "INSERT INTO settings (id, tenant_id, created_at, updated_at, scope, scope_id, key, value) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![
                uuid::Uuid::now_v7().to_string(),
                "tenant",
                0_i64,
                0_i64,
                "tenant",
                "",
                "stress",
                "1"
            ],
        )?;
        Ok(())
    })
    .unwrap();
    scratch
        .with_conn_write(|conn| {
            conn.execute(
                "INSERT INTO draft_sale (id, tenant_id, created_at, updated_at, branch_id, terminal_id, status) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open')",
                rusqlite::params!["stress-draft", "tenant", 0_i64, 0_i64, "branch", "terminal"],
            )?;
            Ok(())
        })
        .unwrap();
    // Graceful shutdown FIRST (join the threads, close the
    // connections) — only then delete the directory. This is the
    // order the fix guarantees; asserting `Ok` here is what catches
    // a regression to detached threads.
    db.shutdown().unwrap();
    scratch.shutdown().unwrap();
    drop(db);
    drop(scratch);
    std::fs::remove_dir_all(&dir).unwrap();
}

#[test]
#[ignore = "slow stress test (~6 min for 2000 cycles): run with `just stress` or in the CI 20x job, not in the fast `just check` loop"]
fn shutdown_joins_cleanly_under_parallel_load() {
    // 8 threads x 250 cycles = 2000 create/drop cycles in parallel.
    // Each cycle asserts both shutdowns join cleanly. If the
    // teardown regresses (detached threads, files deleted while a
    // thread is still closing), this test fails or segfaults.
    const THREADS: usize = 8;
    const CYCLES_PER_THREAD: usize = 250;
    std::thread::scope(|scope| {
        for t in 0..THREADS {
            scope.spawn(move || {
                for i in 0..CYCLES_PER_THREAD {
                    one_cycle(&format!("shutdown-{t}-{i}"));
                }
            });
        }
    });
}
