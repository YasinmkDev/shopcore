//! The `Core` handle: the app-level owner of the `Db` and the
//! `ScratchDb` (ADR-036).
//!
//! The app (Tauri desktop, hub server) holds one `Core` and passes
//! `&Db` / `&ScratchDb` to the use cases. On exit it MUST call
//! `Core::shutdown` — the Tauri exit event calls it (see the 001c-1
//! spec) — so the actor and timer threads are joined before the
//! process exits; otherwise the process can segfault during teardown
//! (a detached thread still closing its connection while the
//! database files are deleted, ADR-036).

use crate::error::CoreError;
use crate::scratch::ScratchDb;
use shopcore_db::Db;

/// The app-level core handle: owns the `Db` and the `ScratchDb`.
/// `Send` + `Sync`, so it can live in Tauri managed state.
pub struct Core {
    db: Db,
    scratch: ScratchDb,
}

impl Core {
    /// Build a `Core` from an already-opened `Db` and `ScratchDb`.
    #[must_use]
    pub fn new(db: Db, scratch: ScratchDb) -> Self {
        Self { db, scratch }
    }

    /// The `Db` (for the use cases).
    #[must_use]
    pub fn db(&self) -> &Db {
        &self.db
    }

    /// The `ScratchDb` (for the draft use cases).
    #[must_use]
    pub fn scratch(&self) -> &ScratchDb {
        &self.scratch
    }

    /// Gracefully shut the core down: shut the `ScratchDb` and the
    /// `Db` down (join their threads, flush the scratch DB, close the
    /// connections). Idempotent. Both are attempted even if the first
    /// fails; the first error is returned. After this returns `Ok`,
    /// no thread is still using the database files, so the process
    /// may exit (or the files may be deleted).
    ///
    /// # Errors
    ///
    /// Returns the first shutdown error (`CoreError::ShutdownTimeout`
    /// or the `Db`'s `ShutdownTimeout`).
    pub fn shutdown(&self) -> Result<(), CoreError> {
        let scratch_result = self.scratch.shutdown();
        let db_result = self.db.shutdown();
        scratch_result?;
        db_result?;
        Ok(())
    }
}

impl Drop for Core {
    fn drop(&mut self) {
        // Best-effort graceful shutdown (same path as `shutdown`).
        let _ = self.shutdown();
    }
}
