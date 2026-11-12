//! The `DB` actor (spec 001b-1 req. 3, `context/04-data-and-sync.md`).
//!
//! One dedicated thread owns the single write connection (`WAL`,
//! `synchronous=FULL`, `SQLCipher` when a key is given). A pool of
//! read-only connections serves queries. All access goes through this
//! struct — there is no public way to obtain a `Connection`, so a write
//! from outside the actor is impossible at the API level.
//!
//! The actor is synchronous by design: `write` blocks the calling thread
//! until the actor thread has run the closure. Async callers MUST call it
//! from `tokio::task::spawn_blocking` (or any blocking thread) — never
//! from an async task, and never from the `Tauri` main thread.

use std::path::Path;
use std::sync::{Mutex, mpsc};

use rusqlite::{Connection, OpenFlags};

use crate::DbError;
use crate::migrations::run_migrations;

/// A job sent to the actor thread: a closure to run on the write
/// connection, plus the channel its result is sent back on.
type WriteFn = Box<dyn FnOnce(&mut Connection) -> Result<(), DbError> + Send>;

struct WriteJob {
    run: WriteFn,
    reply: mpsc::Sender<Result<(), DbError>>,
}

/// The database handle. Cheap to share by reference (`&Db`) or by value
/// (`Db` is `Clone` — clones share the same actor thread and read pool);
/// the app holds one `Db` and passes it to the use cases.
#[derive(Clone)]
pub struct Db {
    inner: std::sync::Arc<DbInner>,
}

/// The shared state behind a `Db` handle.
struct DbInner {
    /// Sender side of the actor channel. `None` after [`Db::shutdown`]
    /// (or after the last `Db` is dropped): closing the channel is what
    /// makes the actor thread exit its loop. Behind a `Mutex` so
    /// `shutdown` can close it from `&self` while other clones still
    /// exist (their later `write` calls then fail with `ActorStopped`,
    /// which is the correct shutdown semantic).
    write_tx: Mutex<Option<mpsc::Sender<WriteJob>>>,
    /// The actor thread's join handle. `None` after it has been joined
    /// (or before the thread was spawned, which never happens — the
    /// handle is set in `from_write_connection`). Behind a `Mutex` so
    /// `shutdown` can take it from `&self`. Joined on shutdown so no
    /// detached thread outlives the `Db` (a detached actor thread that
    /// is still closing its `SQLCipher` connection while the database
    /// files are deleted segfaults — the WAL `-shm` file is
    /// memory-mapped; see `context/08-decisions.md` ADR-036).
    actor_handle: Mutex<Option<std::thread::JoinHandle<()>>>,
    /// Pool of read-only connections. `Connection` is `Send` but not
    /// `Sync`, so the pool itself is behind a `Mutex`.
    read_pool: Mutex<Vec<Connection>>,
    /// The connect target: a filesystem path, or a shared-cache `URI` for
    /// in-memory test databases.
    connect_target: String,
    /// `SQLCipher` key as hex, if the database is encrypted. Applied to
    /// every connection the pool opens, and to the write connection.
    key_hex: Option<String>,
}

/// How long [`Db::shutdown`] waits for the actor thread to exit before
/// giving up (the thread only has to finish its current job and close
/// the connection, so this should never be hit; a hit means the actor
/// is stuck and the caller must know).
const SHUTDOWN_JOIN_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(5);

impl Db {
    /// Open (or create) the database at `path` and run migrations on the
    /// write connection. `key` is the `SQLCipher` key, if the database is
    /// encrypted; `None` opens a plaintext database. The write connection
    /// is `WAL` + `synchronous=FULL` + `foreign_keys=ON` +
    /// `busy_timeout=5000` + `temp_store=MEMORY` (04).
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Sql`] if the database cannot be opened or a
    /// pragma fails, and [`DbError::Migration`] if a migration fails.
    pub fn open(path: impl AsRef<Path>, key: Option<&[u8]>) -> Result<Self, DbError> {
        let target = path.as_ref().to_string_lossy().into_owned();
        let key_hex = key.map(hex_encode);
        let mut write = open_write_connection(&target, key_hex.as_deref())?;
        run_migrations(&mut write)?;
        Ok(Self::from_write_connection(write, target, key_hex))
    }

    /// Open an in-memory database for tests. Each call gets its own
    /// shared-cache `URI`, so the read pool and the actor thread share one
    /// database while test databases never leak into each other.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Sql`] if the database cannot be opened, and
    /// [`DbError::Migration`] if a migration fails.
    pub fn open_in_memory() -> Result<Self, DbError> {
        let target = format!(
            "file:shopcore-test-{}?mode=memory&cache=shared",
            uuid::Uuid::now_v7()
        );
        let mut write = open_write_connection(&target, None)?;
        run_migrations(&mut write)?;
        Ok(Self::from_write_connection(write, target, None))
    }

    /// Run `f` on the write connection, on the actor thread. Serialized
    /// with every other write. Blocks the calling thread until the actor
    /// has run the closure — call from a blocking thread, never from an
    /// async task.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::ActorStopped`] if the actor thread is gone, or
    /// whatever `f` returns.
    pub fn write<F>(&self, f: F) -> Result<(), DbError>
    where
        F: FnOnce(&mut Connection) -> Result<(), DbError> + Send + 'static,
    {
        let (tx, rx) = mpsc::channel();
        let job = WriteJob {
            run: Box::new(f),
            reply: tx,
        };
        // Clone the sender out of the `Mutex` so the lock is not held
        // while sending (the channel is unbounded, so the send itself
        // never blocks, but cloning keeps the critical section short).
        // `None` means `shutdown` ran: no more writes are accepted.
        let sender = self
            .inner
            .write_tx
            .lock()
            .map_err(|_| DbError::InvalidInput("write channel lock poisoned".to_string()))?
            .clone()
            .ok_or(DbError::ActorStopped)?;
        sender.send(job).map_err(|_| DbError::ActorStopped)?;
        rx.recv().map_err(|_| DbError::ActorStopped)?
    }

    /// Run `f` on a read-only connection from the pool. Concurrent reads
    /// run on different connections; a read never blocks a write (WAL)
    /// and never sees a half-written transaction.
    ///
    /// # Errors
    ///
    /// Returns [`DbError::Sql`] if a read connection cannot be opened, or
    /// whatever `f` returns.
    pub fn read<T, F>(&self, f: F) -> Result<T, DbError>
    where
        T: Send,
        F: FnOnce(&Connection) -> Result<T, DbError>,
    {
        let conn = self.take_read_conn()?;
        let result = f(&conn);
        self.return_read_conn(conn);
        result
    }

    /// Spawn the actor thread around an already-opened, already-migrated
    /// write connection. The thread's join handle is kept in the inner
    /// so [`Db::shutdown`] (and the inner's `Drop`) can join it — the
    /// thread must be joined, never detached, before the database files
    /// are deleted (ADR-036).
    fn from_write_connection(
        write: Connection,
        connect_target: String,
        key_hex: Option<String>,
    ) -> Self {
        let (write_tx, write_rx) = mpsc::channel::<WriteJob>();
        let actor_handle = std::thread::spawn(move || {
            let mut conn = write;
            while let Ok(job) = write_rx.recv() {
                let result = (job.run)(&mut conn);
                // The caller may have gone away; the result is dropped.
                let _ = job.reply.send(result);
            }
            // `conn` drops here, on the actor thread, after the loop
            // exits (channel closed) — `sqlite3_close` runs while no
            // other thread can still be using the database.
        });
        Self {
            inner: std::sync::Arc::new(DbInner {
                write_tx: Mutex::new(Some(write_tx)),
                actor_handle: Mutex::new(Some(actor_handle)),
                read_pool: Mutex::new(Vec::new()),
                connect_target,
                key_hex,
            }),
        }
    }

    /// Gracefully shut the actor thread down and close every
    /// connection: close the command channel (so the actor thread exits
    /// its loop), join the actor thread (with a timeout), then close
    /// the pooled read connections. Idempotent — a second call is a
    /// no-op. After this returns `Ok`, no thread is still using the
    /// database files, so the caller may delete them.
    ///
    /// Call this before deleting the database files, and call it from
    /// the app's exit path (see `Core::shutdown`). Never call it from
    /// the actor thread itself (it would deadlock joining itself).
    ///
    /// # Errors
    ///
    /// Returns [`DbError::ShutdownTimeout`] if the actor thread did not
    /// exit within the timeout (it is stuck — do NOT delete the files).
    pub fn shutdown(&self) -> Result<(), DbError> {
        self.inner.shutdown_inner()
    }

    /// Take a read-only connection from the pool, opening a new one if the
    /// pool is empty.
    fn take_read_conn(&self) -> Result<Connection, DbError> {
        let mut pool = self
            .inner
            .read_pool
            .lock()
            .map_err(|_| DbError::InvalidInput("read pool lock poisoned".to_string()))?;
        if let Some(conn) = pool.pop() {
            return Ok(conn);
        }
        drop(pool);
        open_read_connection(&self.inner.connect_target, self.inner.key_hex.as_deref())
    }

    /// Return a connection to the pool for reuse. If the pool lock is
    /// poisoned, the connection simply drops and closes — safe, just one
    /// fewer pooled connection.
    fn return_read_conn(&self, conn: Connection) {
        if let Ok(mut pool) = self.inner.read_pool.lock() {
            pool.push(conn);
        }
    }
}

impl DbInner {
    /// The shared shutdown path, called by [`Db::shutdown`] and by the
    /// inner's `Drop` (last `Db` clone gone). Closes the command
    /// channel, joins the actor thread with a timeout, then closes the
    /// pooled read connections. Idempotent.
    fn shutdown_inner(&self) -> Result<(), DbError> {
        // Close the channel first: taking the sender out is what makes
        // the actor thread's `recv` fail and its loop exit. (The
        // channel was the only sender, so no other clone can re-open
        // it; their later `write` calls fail with `ActorStopped`.)
        if let Ok(mut tx) = self.write_tx.lock() {
            *tx = None;
        }
        // Join the actor thread. `is_finished` is polled so the wait
        // is bounded: a stuck actor must be reported, not waited on
        // forever (and never joined from the actor thread itself —
        // `shutdown` is never called there).
        let handle = self
            .actor_handle
            .lock()
            .map_err(|_| DbError::InvalidInput("actor handle lock poisoned".to_string()))?
            .take();
        if let Some(handle) = handle {
            let deadline = std::time::Instant::now() + SHUTDOWN_JOIN_TIMEOUT;
            while !handle.is_finished() {
                if std::time::Instant::now() >= deadline {
                    return Err(DbError::ShutdownTimeout);
                }
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            // The thread finished; joining now cannot block. A panic
            // in the actor is a defect — surface it rather than
            // swallowing it.
            handle.join().map_err(|_| DbError::ActorStopped)?;
        }
        // Close the pooled read connections explicitly (dropping the
        // `Vec` closes each `Connection`).
        if let Ok(mut pool) = self.read_pool.lock() {
            pool.clear();
        }
        Ok(())
    }
}

impl Drop for DbInner {
    fn drop(&mut self) {
        // Best-effort graceful shutdown (same path as `Db::shutdown`):
        // close the channel, join the actor thread, close the read
        // pool. A failure here (stuck actor) cannot be reported from
        // `Drop`, but the explicit `shutdown` call — which the app and
        // the tests use before deleting the files — does report it.
        let _ = self.shutdown_inner();
    }
}

/// Open the write connection with the 04 pragmas. The `SQLCipher` key, if
/// given, is applied FIRST (it must precede any other statement).
fn open_write_connection(target: &str, key_hex: Option<&str>) -> Result<Connection, DbError> {
    let conn = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_WRITE
            | OpenFlags::SQLITE_OPEN_CREATE
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    if let Some(key) = key_hex {
        conn.execute_batch(&format!("PRAGMA key = \"x'{key}'\""))?;
    }
    conn.execute_batch(
        "PRAGMA journal_mode=WAL;
         PRAGMA synchronous=FULL;
         PRAGMA foreign_keys=ON;
         PRAGMA busy_timeout=5000;
         PRAGMA temp_store=MEMORY;",
    )?;
    Ok(conn)
}

/// Open a read-only connection to the same database. The `SQLCipher` key,
/// if any, is applied first, like on the write connection.
fn open_read_connection(target: &str, key_hex: Option<&str>) -> Result<Connection, DbError> {
    let conn = Connection::open_with_flags(
        target,
        OpenFlags::SQLITE_OPEN_READ_ONLY
            | OpenFlags::SQLITE_OPEN_URI
            | OpenFlags::SQLITE_OPEN_FULL_MUTEX,
    )?;
    if let Some(key) = key_hex {
        conn.execute_batch(&format!("PRAGMA key = \"x'{key}'\""))?;
    }
    conn.execute_batch("PRAGMA busy_timeout=5000;")?;
    Ok(conn)
}

/// Lowercase hex encoding (no `0x` prefix), for the `SQLCipher` key pragma.
fn hex_encode(bytes: &[u8]) -> String {
    use std::fmt::Write as _;
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        // `write!` on a `String` cannot fail; the result is discarded.
        let _ = write!(out, "{byte:02x}");
    }
    out
}
