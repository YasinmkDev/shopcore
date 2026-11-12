#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! `SQLCipher` backward-compatibility: a database file written by the old
//! build (rusqlite 0.41.1 / sqlcipher-src 419.0.6, `SQLCipher` 4.19.0) must
//! still open with the same key under the new build (rusqlite 0.42.0 /
//! sqlcipher-src 420.0.0, `SQLCipher` 4.20.0). The fixture
//! `tests/fixtures/sqlcipher-419-compat.db` was created by the old build
//! (see `examples/make_419_fixture.rs`, since deleted); it contains one
//! marker row in `settings` (`sqlcipher-419-compat` = `sqlcipher-419-ok`).
//!
//! The test copies the fixture into a fresh temp dir first, so the
//! committed file is never modified (opening a WAL database can create
//! sidecars). The key is the test-only key already used by
//! `actor::sqlcipher_key_roundtrip`; it is not a real secret.

use shopcore_db::Db;

const FIXTURE_KEY: &[u8] = b"test-key-32-bytes-long-0123456789";

#[test]
fn old_db_opens_with_same_key() {
    let fixture = format!(
        "{}/tests/fixtures/sqlcipher-419-compat.db",
        env!("CARGO_MANIFEST_DIR")
    );
    assert!(
        std::path::Path::new(&fixture).exists(),
        "compat fixture missing: {fixture}"
    );
    let dir = std::env::temp_dir().join(format!("shopcore-db-compat-419-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("sqlcipher-419-compat.db");
    std::fs::copy(&fixture, &path).unwrap();
    let db = Db::open(&path, Some(FIXTURE_KEY)).unwrap();
    let value: String = db
        .read(|conn| {
            conn.query_row(
                "SELECT value FROM settings WHERE key = 'sqlcipher-419-compat'",
                [],
                |row| row.get(0),
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert_eq!(value, "sqlcipher-419-ok");
    drop(db);
    std::fs::remove_dir_all(&dir).unwrap();
}
