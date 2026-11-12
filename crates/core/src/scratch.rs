//! The scratch database for draft carts (spec 001b-2 req. 2, ADR-034).
//!
//! Drafts live in a SEPARATE SQLite file with its OWN connection and
//! `synchronous=NORMAL` — NOT the main database (which stays
//! `synchronous=FULL`). The scan path is the hot path: a `FULL` flush
//! per keystroke would break the < 50 ms budget, so the draft write
//! goes to the scratch DB only. The main DB never sees a draft flush.
//!
//! Accepted loss window (ADR-034, corrected 2026-11-11): in WAL mode
//! with `synchronous=NORMAL`, commits may be lost back to the LAST
//! CHECKPOINT on power loss — not just an unflushed tail, and NOT a
//! ~250 ms window. So the scratch DB runs a checkpoint timer on its
//! own thread ([`ScratchDb::open_with_checkpoint_interval`]): about
//! 1 s after the draft last changed, the timer forces a WAL checkpoint
//! (`PRAGMA wal_checkpoint(TRUNCATE)`) OFF the scan path, bounding
//! the loss window to ~1 s plus the checkpoint duration (measured —
//! see ADR-034). Under continuous draft changes the 1 s idle deadline
//! keeps moving, so a checkpoint ALSO fires at least every ~5 s since
//! the DB first became dirty (`DEFAULT_CHECKPOINT_MAX_WAIT`) — the
//! loss window under load is bounded by ~5 s, not unbounded.
//! A committed sale is never affected: it lives in the
//! main DB (`synchronous=FULL`). A process kill loses nothing (the OS
//! still flushes the file); only a real power cut can lose the
//! un-checkpointed tail, and the manual power-pull test (pre-pilot
//! checklist, owner) is what proves that.
//!
//! The scratch DB holds the draft tables (`draft_sale` / `draft_line`,
//! same shape as the main DB's 001b-1 tables) plus two tables of its
//! own: `draft_removed_line` (the single-level undo slot — one removed
//! line per draft) and `draft_tombstone` (a completed draft's id, so a
//! late flush can never resurrect it).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

use rusqlite::Connection;

use crate::CoreError;

/// The default checkpoint interval: the timer forces a WAL checkpoint
/// this long after the draft last changed (ADR-034).
pub const DEFAULT_CHECKPOINT_INTERVAL: Duration = Duration::from_secs(1);

/// The default max wait between checkpoints: even under continuous
/// draft changes (which keep re-arming the idle timer), the timer
/// forces a WAL checkpoint at least this often since the last flush
/// (ADR-034, 2026-11-12 follow-up). Bounds the loss window under
/// load to ~`MAX_WAIT` instead of unbounded.
pub const DEFAULT_CHECKPOINT_MAX_WAIT: Duration = Duration::from_secs(5);

/// The checkpoint timer's shared state: the last draft change (or
/// `None` when nothing is dirty), the first change since the last
/// checkpoint, and the shutdown flag.
struct CheckpointState {
    inner: Mutex<CheckpointInner>,
    changed: Condvar,
}

struct CheckpointInner {
    last_change: Option<Instant>,
    /// First draft change since the last checkpoint (`None` when
    /// clean). The max-wait deadline is measured from here: a
    /// checkpoint fires at most `max_wait` after the DB first became
    /// dirty, even if changes keep arriving.
    dirty_since: Option<Instant>,
    shutdown: bool,
}

impl CheckpointState {
    fn new() -> Self {
        Self {
            inner: Mutex::new(CheckpointInner {
                last_change: None,
                dirty_since: None,
                shutdown: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Mark the draft changed (a draft write happened): the timer
    /// fires one interval after this instant, unless another change
    /// moves it later. The first change after a checkpoint also
    /// starts the max-wait clock (`dirty_since`).
    fn mark_changed(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            if inner.last_change.is_none() {
                inner.dirty_since = Some(Instant::now());
            }
            inner.last_change = Some(Instant::now());
        }
        self.changed.notify_one();
    }

    /// Shut the timer thread down (called from [`ScratchDb`]'s `Drop`).
    fn shutdown(&self) {
        if let Ok(mut inner) = self.inner.lock() {
            inner.shutdown = true;
        }
        self.changed.notify_all();
    }
}

/// DDL for the scratch database. The draft tables mirror the main DB's
/// 001b-1 `draft_sale` / `draft_line` columns exactly, so a draft row
/// means the same thing in either store.
const SCRATCH_DDL: &str = "
CREATE TABLE IF NOT EXISTS draft_sale (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1,
  branch_id TEXT NOT NULL,
  terminal_id TEXT NOT NULL,
  shift_id TEXT,
  user_id TEXT,
  status TEXT NOT NULL DEFAULT 'open'
);
CREATE TABLE IF NOT EXISTS draft_line (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1,
  draft_sale_id TEXT NOT NULL REFERENCES draft_sale(id),
  sku_id TEXT NOT NULL,
  qty_milli INTEGER NOT NULL,
  unit_price_e4 INTEGER NOT NULL,
  discount_minor INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS idx_scratch_draft_line_draft ON draft_line(draft_sale_id);
-- Single-level undo slot: at most one removed line per draft (the most
-- recently removed one). A second removal overwrites it.
CREATE TABLE IF NOT EXISTS draft_removed_line (
  draft_sale_id TEXT PRIMARY KEY REFERENCES draft_sale(id),
  id TEXT NOT NULL,
  tenant_id TEXT NOT NULL,
  sku_id TEXT NOT NULL,
  qty_milli INTEGER NOT NULL,
  unit_price_e4 INTEGER NOT NULL,
  discount_minor INTEGER NOT NULL DEFAULT 0,
  removed_at INTEGER NOT NULL
);
-- Completed drafts: a late scratch-DB flush after completion must not
-- resurrect the draft. The tombstone is checked before any restore or
-- flush applies (the main DB's sale row with the same `draft_id` is the
-- backstop — the tombstone is best-effort, the sale row is authoritative).
CREATE TABLE IF NOT EXISTS draft_tombstone (
  draft_id TEXT PRIMARY KEY,
  sale_id TEXT NOT NULL,
  completed_at INTEGER NOT NULL
);
";

/// The scratch database: one connection (behind a `Mutex` — `Connection`
/// is `Send` but not `Sync`), `WAL` + `synchronous=NORMAL`, plus a
/// checkpoint timer thread (ADR-034) that forces a WAL checkpoint
/// about 1 s after the draft last changed, off the scan path.
pub struct ScratchDb {
    /// The connection. Serialized by the mutex; draft operations are
    /// short, single-statement writes, so contention is negligible.
    /// Shared with the checkpoint timer thread (same mutex — the timer
    /// only takes it to run a checkpoint, never the other way round).
    conn: Arc<Mutex<Connection>>,
    /// The file path (for the tests that assert the main DB file is
    /// untouched by the scan path).
    path: PathBuf,
    /// The checkpoint timer's shared state.
    checkpoint_state: Arc<CheckpointState>,
    /// Completed WAL checkpoints since this scratch DB was opened
    /// (test hook — see [`ScratchDb::checkpoints_completed`]).
    checkpoints: Arc<AtomicU64>,
    /// The checkpoint timer thread's join handle. `None` after it has
    /// been joined. Behind a `Mutex` so [`ScratchDb::shutdown`] can
    /// take it from `&self`. Joined on shutdown so no detached thread
    /// outlives the `ScratchDb` (a detached timer thread that is still
    /// checkpointing while the scratch files are deleted segfaults —
    /// the WAL `-shm` file is memory-mapped; see ADR-036).
    timer_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
}

/// How long [`ScratchDb::shutdown`] waits for the checkpoint timer
/// thread to exit before giving up (the thread only has to wake, see
/// the shutdown flag, and return, so this should never be hit).
const SCRATCH_SHUTDOWN_JOIN_TIMEOUT: Duration = Duration::from_secs(5);

impl ScratchDb {
    /// Open (or create) the scratch database at `path`. Sets `WAL` +
    /// `synchronous=NORMAL` + `foreign_keys=ON` + `busy_timeout=5000`
    /// and creates the draft tables when missing. Idempotent: opening
    /// an existing scratch DB is a no-op for the schema. Spawns the
    /// checkpoint timer thread with the default 1 s interval.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the file cannot be opened or the
    /// pragmas / DDL fail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CoreError> {
        Self::open_with_checkpoint_interval(path, DEFAULT_CHECKPOINT_INTERVAL)
    }

    /// Open with a custom checkpoint interval (tests use a short
    /// interval, e.g. 50 ms, so the timer test does not wait a second).
    /// The max wait between checkpoints is the default (5 s).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the file cannot be opened or the
    /// pragmas / DDL fail.
    pub fn open_with_checkpoint_interval(
        path: impl AsRef<Path>,
        interval: Duration,
    ) -> Result<Self, CoreError> {
        Self::open_with_checkpoint_intervals(path, interval, DEFAULT_CHECKPOINT_MAX_WAIT)
    }

    /// Open with a custom checkpoint interval AND max wait (tests use
    /// short values for both, so the max-wait test does not wait 5 s).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the file cannot be opened or the
    /// pragmas / DDL fail.
    pub fn open_with_checkpoint_intervals(
        path: impl AsRef<Path>,
        interval: Duration,
        max_wait: Duration,
    ) -> Result<Self, CoreError> {
        let path = path.as_ref().to_path_buf();
        let conn = Connection::open(&path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL;
             PRAGMA synchronous=NORMAL;
             PRAGMA foreign_keys=ON;
             PRAGMA busy_timeout=5000;
             PRAGMA temp_store=MEMORY;",
        )?;
        conn.execute_batch(SCRATCH_DDL)?;
        let conn = Arc::new(Mutex::new(conn));
        let checkpoint_state = Arc::new(CheckpointState::new());
        let checkpoints = Arc::new(AtomicU64::new(0));
        // The checkpoint timer thread (ADR-034): forces a WAL
        // checkpoint one interval after the draft last changed, and
        // at least every `max_wait` since the last flush even under
        // continuous changes, off the scan path. The thread's join
        // handle is kept so `shutdown` (and `Drop`) can join it —
        // the thread must be joined, never detached, before the
        // scratch files are deleted (ADR-036).
        let timer_handle = {
            let timer = CheckpointTimer {
                conn: Arc::clone(&conn),
                state: Arc::clone(&checkpoint_state),
                interval,
                max_wait,
                checkpoints: Arc::clone(&checkpoints),
            };
            std::thread::spawn(move || checkpoint_timer(timer))
        };
        Ok(Self {
            conn,
            path,
            checkpoint_state,
            checkpoints,
            timer_handle: Mutex::new(Some(timer_handle)),
        })
    }

    /// Gracefully shut the checkpoint timer thread down and flush the
    /// scratch DB: signal the timer thread to exit, join it (with a
    /// timeout), then force a final WAL checkpoint so every draft
    /// write is durable in the main file. Idempotent — a second call
    /// is a no-op. After this returns `Ok`, no thread is still using
    /// the scratch files, so the caller may delete them.
    ///
    /// Call this before deleting the scratch database files, and call
    /// it from the app's exit path (see `Core::shutdown`). Never call
    /// it from the timer thread itself (it would deadlock joining
    /// itself).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::ShutdownTimeout`] if the timer thread did
    /// not exit within the timeout (it is stuck — do NOT delete the
    /// files).
    pub fn shutdown(&self) -> Result<(), CoreError> {
        // Signal the timer thread to exit first (it wakes from the
        // condvar, sees the flag, and returns).
        self.checkpoint_state.shutdown();
        // Join the timer thread. `is_finished` is polled so the wait
        // is bounded.
        let handle = self
            .timer_handle
            .lock()
            .map_err(|_| CoreError::InvalidInput("timer handle lock poisoned".to_string()))?
            .take();
        if let Some(handle) = handle {
            let deadline = std::time::Instant::now() + SCRATCH_SHUTDOWN_JOIN_TIMEOUT;
            while !handle.is_finished() {
                if std::time::Instant::now() >= deadline {
                    return Err(CoreError::ShutdownTimeout);
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            handle.join().map_err(|_| {
                CoreError::InvalidInput("checkpoint timer thread panicked".to_string())
            })?;
        }
        // Flush: force a final WAL checkpoint now that no thread is
        // racing the connection, so the scratch DB is fully durable
        // before the caller deletes (or keeps) the files.
        let _ = run_checkpoint(&self.conn);
        Ok(())
    }

    /// Mark the draft changed (call after a successful draft write):
    /// the checkpoint timer fires one interval after this instant.
    /// Cheap (a mutex + notify) — the scan path never waits on the
    /// checkpoint itself.
    pub fn mark_changed(&self) {
        self.checkpoint_state.mark_changed();
    }

    /// Completed WAL checkpoints since this scratch DB was opened
    /// (test hook for the timer test).
    #[must_use]
    pub fn checkpoints_completed(&self) -> u64 {
        self.checkpoints.load(Ordering::SeqCst)
    }

    /// The scratch file path.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Run `f` on the scratch connection. The connection is locked for
    /// the duration — keep `f` short (one draft operation).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidInput`] if the lock is poisoned, and
    /// whatever `f` returns.
    pub fn with_conn<T, F>(&self, f: F) -> Result<T, CoreError>
    where
        F: FnOnce(&mut Connection) -> Result<T, CoreError>,
    {
        let mut guard = self
            .conn
            .lock()
            .map_err(|_| CoreError::InvalidInput("scratch db lock poisoned".to_string()))?;
        f(&mut guard)
    }

    /// Run `f` on the scratch connection AND mark the draft changed
    /// (for WRITES — see [`ScratchDb::mark_changed`]). Reads use
    /// [`ScratchDb::with_conn`] and do not re-arm the timer. Every
    /// draft-mutating use case goes through here, so a write can
    /// never forget to re-arm the checkpoint timer.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::InvalidInput`] if the lock is poisoned, and
    /// whatever `f` returns.
    pub fn with_conn_write<T, F>(&self, f: F) -> Result<T, CoreError>
    where
        F: FnOnce(&mut Connection) -> Result<T, CoreError>,
    {
        let out = self.with_conn(f)?;
        self.mark_changed();
        Ok(out)
    }

    /// Is this draft tombstoned (completed)? Checked before any restore
    /// or flush applies.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the read fails.
    pub fn is_tombstoned(&self, draft_id: &str) -> Result<bool, CoreError> {
        let draft_id = draft_id.to_string();
        self.with_conn(|conn| {
            let n: i64 = conn.query_row(
                "SELECT COUNT(*) FROM draft_tombstone WHERE draft_id = ?1",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )?;
            Ok(n > 0)
        })
    }

    /// Tombstone a completed draft (best-effort: the main DB's sale row
    /// is the authoritative backstop — see the module docs).
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the write fails.
    pub fn tombstone(&self, draft_id: &str, sale_id: &str, now_ms: i64) -> Result<(), CoreError> {
        let draft_id = draft_id.to_string();
        let sale_id = sale_id.to_string();
        self.with_conn_write(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO draft_tombstone (draft_id, sale_id, completed_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![draft_id, sale_id, now_ms],
            )?;
            Ok(())
        })
    }
}

impl Drop for ScratchDb {
    fn drop(&mut self) {
        // Best-effort graceful shutdown (same path as `shutdown`):
        // signal the timer thread, join it, flush the WAL. A failure
        // here (stuck timer) cannot be reported from `Drop`, but the
        // explicit `shutdown` call — which the app and the tests use
        // before deleting the files — does report it.
        let _ = self.shutdown();
    }
}

/// Force a WAL checkpoint (`TRUNCATE`: copy all WAL frames into the
/// main file and truncate the WAL, so everything checkpointed is
/// durable in the main file). Returns `true` when the checkpoint
/// completed (not busy). Best-effort: a failure only delays the
/// checkpoint to the next timer fire.
fn run_checkpoint(conn: &Mutex<Connection>) -> bool {
    let Ok(guard) = conn.lock() else {
        return false;
    };
    guard
        .query_row("PRAGMA wal_checkpoint(TRUNCATE)", [], |row| {
            row.get::<_, i64>(0) // `busy`: 1 when a reader blocked it.
        })
        .is_ok_and(|busy| busy == 0)
}

/// The checkpoint timer's owned inputs (one struct, moved into the
/// timer thread — the thread outlives `open`, so it owns its `Arc`s).
struct CheckpointTimer {
    conn: Arc<Mutex<Connection>>,
    state: Arc<CheckpointState>,
    interval: Duration,
    /// Max time between checkpoints even under continuous changes.
    max_wait: Duration,
    checkpoints: Arc<AtomicU64>,
}

/// The checkpoint timer thread: waits until the draft is dirty, then
/// until one interval has passed since the LAST change — or until the
/// max wait has passed since the LAST FLUSH, whichever comes first —
/// then forces a WAL checkpoint on the shared connection. Under
/// continuous draft changes the idle deadline keeps moving, but the
/// max-wait deadline does not, so a checkpoint fires at least every
/// `max_wait` even while the draft keeps changing (ADR-034: the loss
/// window under load is bounded by `max_wait`, not unbounded). The
/// scan path never waits on it — a draft write only notifies the
/// thread ([`CheckpointState::mark_changed`]); the checkpoint itself
/// runs here, taking the connection mutex only for its own duration.
fn checkpoint_timer(timer: CheckpointTimer) {
    let CheckpointTimer {
        conn,
        state,
        interval,
        max_wait,
        checkpoints,
    } = timer;
    loop {
        // Wait until dirty or shut down.
        let Ok(mut inner) = state.inner.lock() else {
            return;
        };
        while inner.last_change.is_none() && !inner.shutdown {
            inner = match state.changed.wait(inner) {
                Ok(guard) => guard,
                Err(_) => return,
            };
        }
        if inner.shutdown {
            return;
        }
        // Wait until the earlier of the two deadlines: one interval
        // after the LAST change, or `max_wait` after the DB FIRST
        // became dirty since the last checkpoint. Wakes early when a
        // new change moves the idle deadline (or on shutdown) and
        // recomputes.
        loop {
            if inner.shutdown {
                return;
            }
            let Some(last) = inner.last_change else {
                break; // Cleared elsewhere; back to waiting for dirt.
            };
            let Some(dirty_since) = inner.dirty_since else {
                break; // Unreachable while dirty; be safe.
            };
            let deadline = (last + interval).min(dirty_since + max_wait);
            let now = Instant::now();
            if now >= deadline {
                break;
            }
            let Ok((guard, _)) = state
                .changed
                .wait_timeout(inner, deadline.saturating_duration_since(now))
            else {
                return;
            };
            inner = guard;
        }
        if inner.shutdown {
            return;
        }
        if inner.last_change.is_none() {
            continue;
        }
        // Clear the dirty flags BEFORE checkpointing: a change that
        // lands during the checkpoint re-arms the timer (and restarts
        // the max-wait clock from that change).
        inner.last_change = None;
        inner.dirty_since = None;
        drop(inner);
        if run_checkpoint(&conn) {
            checkpoints.fetch_add(1, Ordering::SeqCst);
        } else {
            // The checkpoint could not run (a reader held the DB —
            // impossible with our single connection, but be safe):
            // re-arm so it retries after the interval.
            state.mark_changed();
        }
    }
}
