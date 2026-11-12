//! Scratch-DB checkpoint timer tests (ADR-034, owner fix 2026-11-11).
//!
//! In WAL mode with `synchronous=NORMAL`, commits may be lost back to
//! the LAST CHECKPOINT on power loss — so the scratch DB runs a
//! checkpoint timer on its own thread that forces a WAL checkpoint
//! about 1 s after the draft last changed, off the scan path. These
//! tests prove the timer fires after a draft change and measure the
//! real bound (time from the last change to the checkpoint completing,
//! under load).
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{test_root_scratch, unique_test_dir};
use shopcore_core::draft::{draft_add_line, load_draft, open_draft};
use shopcore_core::scratch::ScratchDb;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

/// A unique scratch directory under the workspace `target/` (real disk,
/// not `/tmp` tmpfs — the checkpoint timing must see real `fsync`
/// behaviour). See `common::unique_test_dir` (PID + `UUIDv7`, `create_dir`).
///
/// Removes the dir when dropped (best effort), so a failed test does
/// not leave a stale dir behind.
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Poll until the scratch DB has completed at least `want`
/// checkpoints (or fail after the timeout).
fn wait_for_checkpoints(scratch: &ScratchDb, want: u64, timeout: Duration) {
    let started = Instant::now();
    while scratch.checkpoints_completed() < want {
        assert!(
            started.elapsed() < timeout,
            "the checkpoint timer never fired ({} completed, wanted {want})",
            scratch.checkpoints_completed()
        );
        std::thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn checkpoint_timer_fires_after_a_draft_change() {
    let dir = unique_test_dir(&test_root_scratch(), "timer-fires");
    let _guard = DirGuard(dir.clone());
    let scratch_path = dir.join("scratch.db");
    // Short interval for the test (the production default is 1 s).
    let scratch =
        ScratchDb::open_with_checkpoint_interval(&scratch_path, Duration::from_millis(50)).unwrap();
    assert_eq!(scratch.checkpoints_completed(), 0);
    // A draft change (via the draft use cases, which write through
    // `with_conn_write` and re-arm the timer).
    let cart = open_draft(&scratch, "tenant", "branch", "terminal", None, None, 0).unwrap();
    draft_add_line(&scratch, &cart.id, "sku-1", 1_000, 100_000, 0).unwrap();
    wait_for_checkpoints(&scratch, 1, Duration::from_secs(5));
    // The WAL was checkpointed (TRUNCATE): the sidecar is empty.
    let wal_len = std::fs::metadata(scratch_path.with_extension("db-wal")).map_or(0, |m| m.len());
    assert_eq!(wal_len, 0, "the WAL file is truncated after the checkpoint");
    // The draft survived the checkpoint (it lives in the main file now).
    assert!(load_draft(&scratch, &cart.id).unwrap().is_some());
}

#[test]
fn checkpoint_timer_bound_under_load() {
    let dir = unique_test_dir(&test_root_scratch(), "timer-bound");
    let _guard = DirGuard(dir.clone());
    // Short idle interval AND short max wait (production defaults are
    // 1 s / 5 s): the max-wait cap, not the idle timer, is what fires
    // under continuous change.
    let scratch = ScratchDb::open_with_checkpoint_intervals(
        dir.join("scratch.db"),
        Duration::from_millis(50),
        Duration::from_millis(150),
    )
    .unwrap();
    let cart = open_draft(&scratch, "tenant", "branch", "terminal", None, None, 0).unwrap();
    assert_eq!(scratch.checkpoints_completed(), 0);
    // Load: 4 threads hammering draft writes for ~600 ms (scan-path
    // load). Each write re-arms the idle timer, so WITHOUT the
    // max-wait cap no checkpoint would fire during the load. WITH
    // the cap, a checkpoint fires at least every `max_wait` (150 ms)
    // since the DB first became dirty.
    let stop = AtomicBool::new(false);
    let loaded_at = Instant::now();
    std::thread::scope(|scope| {
        for _ in 0..4 {
            scope.spawn(|| {
                while !stop.load(Ordering::SeqCst) {
                    let _ = scratch.with_conn_write(|conn| {
                        conn.execute(
                            "UPDATE draft_sale SET updated_at = updated_at WHERE id = ?1",
                            rusqlite::params![cart.id],
                        )?;
                        Ok(())
                    });
                }
            });
        }
        // The FIRST checkpoint under continuous change must fire
        // within the max wait (plus scheduling slack on this VM).
        wait_for_checkpoints(&scratch, 1, Duration::from_secs(5));
        let first_fire = loaded_at.elapsed();
        eprintln!("first checkpoint under continuous change: {first_fire:?} (max wait 150 ms)");
        assert!(
            first_fire < Duration::from_secs(1),
            "the max-wait cap did not fire under load: first checkpoint after {first_fire:?}"
        );
        std::thread::sleep(Duration::from_millis(450));
        stop.store(true, Ordering::SeqCst);
    });
    // ~600 ms of load with a 150 ms cap: at least 2 checkpoints must
    // have fired DURING the load (the old assertion — zero — is the
    // unbounded-window behaviour this cap removes).
    let during_load = scratch.checkpoints_completed();
    eprintln!("checkpoints during ~600 ms of continuous change: {during_load} (max wait 150 ms)");
    assert!(
        during_load >= 2,
        "expected >= 2 checkpoints during the load, got {during_load}"
    );
    // The final change: measure from here to the checkpoint completing.
    let changed_at = Instant::now();
    draft_add_line(&scratch, &cart.id, "sku-1", 1_000, 100_000, 0).unwrap();
    wait_for_checkpoints(&scratch, during_load + 1, Duration::from_secs(5));
    let bound = changed_at.elapsed();
    eprintln!("measured checkpoint bound under load: {bound:?} (interval 50 ms)");
    assert!(
        bound < Duration::from_secs(3),
        "the checkpoint bound under load is {bound:?}, expected < 3 s"
    );
}
