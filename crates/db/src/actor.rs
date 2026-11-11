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
    /// Sender side of the actor channel. Dropping the last `Db` closes
    /// the channel and the actor thread exits.
    write_tx: mpsc::Sender<WriteJob>,
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
        self.inner
            .write_tx
            .send(job)
            .map_err(|_| DbError::ActorStopped)?;
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
    /// write connection.
    fn from_write_connection(
        write: Connection,
        connect_target: String,
        key_hex: Option<String>,
    ) -> Self {
        let (write_tx, write_rx) = mpsc::channel::<WriteJob>();
        std::thread::spawn(move || {
            let mut conn = write;
            while let Ok(job) = write_rx.recv() {
                let result = (job.run)(&mut conn);
                // The caller may have gone away; the result is dropped.
                let _ = job.reply.send(result);
            }
        });
        Self {
            inner: std::sync::Arc::new(DbInner {
                write_tx,
                read_pool: Mutex::new(Vec::new()),
                connect_target,
                key_hex,
            }),
        }
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
