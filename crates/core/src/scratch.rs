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
//! see ADR-034). A committed sale is never affected: it lives in the
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

/// The checkpoint timer's shared state: the last draft change (or
/// `None` when nothing is dirty) and the shutdown flag.
struct CheckpointState {
    inner: Mutex<CheckpointInner>,
    changed: Condvar,
}

struct CheckpointInner {
    last_change: Option<Instant>,
    shutdown: bool,
}

impl CheckpointState {
    fn new() -> Self {
        Self {
            inner: Mutex::new(CheckpointInner {
                last_change: None,
                shutdown: false,
            }),
            changed: Condvar::new(),
        }
    }

    /// Mark the draft changed (a draft write happened): the timer
    /// fires one interval after this instant, unless another change
    /// moves it later.
    fn mark_changed(&self) {
        if let Ok(mut inner) = self.inner.lock() {
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
}

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
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the file cannot be opened or the
    /// pragmas / DDL fail.
    pub fn open_with_checkpoint_interval(
        path: impl AsRef<Path>,
        interval: Duration,
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
        // checkpoint one interval after the draft last changed, off
        // the scan path. Detached — `Drop` signals shutdown and the
        // thread exits on its own (it holds only `Arc` clones, so
        // nothing is left behind). Same spawn pattern as the `DB`
        // actor thread.
        {
            let timer = CheckpointTimer {
                conn: Arc::clone(&conn),
                state: Arc::clone(&checkpoint_state),
                interval,
                checkpoints: Arc::clone(&checkpoints),
            };
            std::thread::spawn(move || checkpoint_timer(timer));
        }
        Ok(Self {
            conn,
            path,
            checkpoint_state,
            checkpoints,
        })
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
        // Signal the timer thread to exit; it holds only `Arc`
        // clones and exits on its own (no join needed).
        self.checkpoint_state.shutdown();
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
    checkpoints: Arc<AtomicU64>,
}

/// The checkpoint timer thread: waits until the draft is dirty, then
/// until one interval has passed since the LAST change, then forces a
/// WAL checkpoint on the shared connection. The scan path never waits
/// on it — a draft write only notifies the thread
/// ([`CheckpointState::mark_changed`]); the checkpoint itself runs
/// here, taking the connection mutex only for its own duration.
fn checkpoint_timer(timer: CheckpointTimer) {
    let CheckpointTimer {
        conn,
        state,
        interval,
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
        // Wait until one interval after the LAST change, waking early
        // when a new change moves the deadline (or on shutdown).
        loop {
            if inner.shutdown {
                return;
            }
            let Some(last) = inner.last_change else {
                break; // Cleared elsewhere; back to waiting for dirt.
            };
            let deadline = last + interval;
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
        // Clear the dirty flag BEFORE checkpointing: a change that
        // lands during the checkpoint re-arms the timer.
        inner.last_change = None;
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
