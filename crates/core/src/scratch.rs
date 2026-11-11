//! The scratch database for draft carts (spec 001b-2 req. 2, ADR-034).
//!
//! Drafts live in a SEPARATE SQLite file with its OWN connection and
//! `synchronous=NORMAL` — NOT the main database (which stays
//! `synchronous=FULL`). The scan path is the hot path: a `FULL` flush
//! per keystroke would break the < 50 ms budget, so the draft write
//! goes to the scratch DB only. The main DB never sees a draft flush.
//!
//! Accepted loss window (ADR-034): at most the scratch DB's unflushed
//! tail can be lost in a power cut — the committed sale is never
//! affected, and the worst case is the cashier re-scans the last line
//! or two. A process kill loses nothing (the OS still flushes the file);
//! only a real power cut can lose the unflushed tail, and the manual
//! power-pull test (pre-pilot checklist, owner) is what proves that.
//!
//! The scratch DB holds the draft tables (`draft_sale` / `draft_line`,
//! same shape as the main DB's 001b-1 tables) plus two tables of its
//! own: `draft_removed_line` (the single-level undo slot — one removed
//! line per draft) and `draft_tombstone` (a completed draft's id, so a
//! late flush can never resurrect it).

use std::path::{Path, PathBuf};
use std::sync::Mutex;

use rusqlite::Connection;

use crate::CoreError;

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
/// is `Send` but not `Sync`), `WAL` + `synchronous=NORMAL`.
pub struct ScratchDb {
    /// The connection. Serialized by the mutex; draft operations are
    /// short, single-statement writes, so contention is negligible.
    conn: Mutex<Connection>,
    /// The file path (for the tests that assert the main DB file is
    /// untouched by the scan path).
    path: PathBuf,
}

impl ScratchDb {
    /// Open (or create) the scratch database at `path`. Sets `WAL` +
    /// `synchronous=NORMAL` + `foreign_keys=ON` + `busy_timeout=5000`
    /// and creates the draft tables when missing. Idempotent: opening
    /// an existing scratch DB is a no-op for the schema.
    ///
    /// # Errors
    ///
    /// Returns [`CoreError::Db`] if the file cannot be opened or the
    /// pragmas / DDL fail.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, CoreError> {
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
        Ok(Self {
            conn: Mutex::new(conn),
            path,
        })
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
        self.with_conn(|conn| {
            conn.execute(
                "INSERT OR IGNORE INTO draft_tombstone (draft_id, sale_id, completed_at) VALUES (?1, ?2, ?3)",
                rusqlite::params![draft_id, sale_id, now_ms],
            )?;
            Ok(())
        })
    }
}
