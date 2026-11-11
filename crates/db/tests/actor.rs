#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! `DB` actor + append-only trigger tests (spec 001b-1 req. 3, 13, 4):
//! a single write connection serializes writes, the read pool serves
//! concurrent reads, writes from outside the actor are impossible at the
//! API level (there is no public `Connection` handle), and the append-only
//! triggers refuse `UPDATE`/`DELETE` on the ledger tables and on completed
//! sale rows.

use shopcore_db::{Db, DbError};

const NOW: i64 = 1_700_000_000_000;

fn insert_catalog(db: &Db) {
    db.write(|conn| {
        conn.execute(
            "INSERT INTO item (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('i1', 't1', 'ITM1', 'Milk 1L', ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO sku (id, tenant_id, item_id, code, name, created_at, updated_at, version) VALUES ('s1', 't1', 'i1', 'SKU1', 'Milk 1L', ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO barcode (id, tenant_id, sku_id, code, created_at, updated_at, version) VALUES ('b1', 't1', 's1', '1234567890123', ?1, ?1, 1)",
            [NOW],
        )?;
        Ok(())
    })
    .unwrap();
}

#[test]
fn concurrent_writes_serialize_without_busy_errors() {
    let db = Db::open_in_memory().unwrap();
    std::thread::scope(|scope| {
        for thread in 0..10 {
            let db = &db;
            scope.spawn(move || {
                for row in 0..10 {
                    let key = format!("t{thread}-r{row}");
                    db.write(move |conn| {
                        conn.execute(
                            "INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version) VALUES (?1, 't1', 'tenant', '', ?2, 'v', ?3, ?3, 1)",
                            rusqlite::params![format!("id-{key}"), key, NOW],
                        )?;
                        Ok(())
                    })
                    .unwrap();
                }
            });
        }
    });
    let count: i64 = db
        .read(|conn| {
            conn.query_row(
                "SELECT COUNT(*) FROM settings WHERE scope = 'tenant' AND key LIKE 't%-r%'",
                [],
                |row| row.get(0),
            )
            .map_err(DbError::from)
        })
        .unwrap();
    assert_eq!(count, 100);
}

#[test]
fn read_pool_serves_concurrent_reads() {
    let db = Db::open_in_memory().unwrap();
    insert_catalog(&db);
    std::thread::scope(|scope| {
        for _ in 0..8 {
            let db = &db;
            scope.spawn(move || {
                for _ in 0..25 {
                    let name: String = db
                        .read(|conn| {
                            conn.query_row("SELECT name FROM sku WHERE id = 's1'", [], |row| {
                                row.get(0)
                            })
                            .map_err(DbError::from)
                        })
                        .unwrap();
                    assert_eq!(name, "Milk 1L");
                }
            });
        }
    });
}

#[test]
fn write_then_read_sees_the_same_data() {
    let db = Db::open_in_memory().unwrap();
    insert_catalog(&db);
    let code: String = db
        .read(|conn| {
            conn.query_row("SELECT code FROM barcode WHERE sku_id = 's1'", [], |row| {
                row.get(0)
            })
            .map_err(DbError::from)
        })
        .unwrap();
    assert_eq!(code, "1234567890123");
}

#[test]
fn append_only_triggers_refuse_updates_and_deletes() {
    let db = Db::open_in_memory().unwrap();
    insert_catalog(&db);
    db.write(|conn| {
        conn.execute(
            "INSERT INTO stock_movement (id, tenant_id, branch_id, sku_id, type, qty_milli, occurred_at, created_at, updated_at, version) VALUES ('m1', 't1', 'br1', 's1', 'sale', -1000, ?1, ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO audit_log (id, tenant_id, action, at, created_at, updated_at, version) VALUES ('a1', 't1', 'sale.completed', ?1, ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO sale (id, tenant_id, branch_id, terminal_id, status, created_at, updated_at, version) VALUES ('sa1', 't1', 'br1', 'te1', 'completed', ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO sale_line (id, tenant_id, sale_id, sku_id, qty_milli, unit_price_e4, total_minor, created_at, updated_at, version) VALUES ('sl1', 't1', 'sa1', 's1', 1000, 10000, 10000, ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO sale_payment (id, tenant_id, sale_id, method, amount_minor, created_at, updated_at, version) VALUES ('sp1', 't1', 'sa1', 'cash', 10000, ?1, ?1, 1)",
            [NOW],
        )?;
        Ok(())
    })
    .unwrap();

    for (table, id) in [
        ("stock_movement", "m1"),
        ("audit_log", "a1"),
        ("sale", "sa1"),
        ("sale_line", "sl1"),
        ("sale_payment", "sp1"),
    ] {
        let update_sql = format!("UPDATE {table} SET updated_at = ?1 WHERE id = ?2");
        let update_id = id.to_string();
        let update = db.write(move |conn| {
            conn.execute(&update_sql, rusqlite::params![NOW + 1, update_id])?;
            Ok(())
        });
        assert!(update.is_err(), "{table} UPDATE must be refused");
        let delete_sql = format!("DELETE FROM {table} WHERE id = ?1");
        let delete_id = id.to_string();
        let delete = db.write(move |conn| {
            conn.execute(&delete_sql, rusqlite::params![delete_id])?;
            Ok(())
        });
        assert!(delete.is_err(), "{table} DELETE must be refused");
    }
}

#[test]
fn draft_sale_rows_stay_editable_until_completion() {
    let db = Db::open_in_memory().unwrap();
    db.write(|conn| {
        conn.execute(
            "INSERT INTO sale (id, tenant_id, branch_id, terminal_id, status, created_at, updated_at, version) VALUES ('sa9', 't1', 'br1', 'te1', 'draft', ?1, ?1, 1)",
            [NOW],
        )?;
        conn.execute(
            "INSERT INTO sale_line (id, tenant_id, sale_id, sku_id, qty_milli, unit_price_e4, total_minor, created_at, updated_at, version) VALUES ('sl9', 't1', 'sa9', 's1', 1000, 10000, 10000, ?1, ?1, 1)",
            [NOW],
        )?;
        Ok(())
    })
    .unwrap();
    // A draft sale and its lines can still be edited (001b-2 edits drafts).
    db.write(|conn| {
        conn.execute("UPDATE sale SET status = 'held' WHERE id = 'sa9'", [])?;
        conn.execute("UPDATE sale_line SET qty_milli = 2000 WHERE id = 'sl9'", [])?;
        Ok(())
    })
    .unwrap();
    let qty: i64 = db
        .read(|conn| {
            conn.query_row(
                "SELECT qty_milli FROM sale_line WHERE id = 'sl9'",
                [],
                |row| row.get(0),
            )
            .map_err(DbError::from)
        })
        .unwrap();
    assert_eq!(qty, 2000);
}

#[test]
fn catalog_version_bumps_on_every_catalog_write() {
    let db = Db::open_in_memory().unwrap();
    let version = |db: &Db| -> Option<i64> {
        db.read(|conn| {
            Ok(conn
                .query_row(
                    "SELECT CAST(value AS INTEGER) FROM settings WHERE tenant_id = 't1' AND scope = 'tenant' AND scope_id = '' AND key = 'catalog.version'",
                    [],
                    |row| row.get(0),
                )
                .map_err(DbError::from)
                .ok())
        })
        .unwrap()
    };
    assert_eq!(version(&db), None);
    insert_catalog(&db);
    // item + sku + barcode inserts each bumped the counter once.
    assert_eq!(version(&db), Some(3));
    db.write(|conn| {
        conn.execute(
            "UPDATE sku SET name = 'Milk 1L Full Cream' WHERE id = 's1'",
            [],
        )?;
        Ok(())
    })
    .unwrap();
    assert_eq!(version(&db), Some(4));
}

#[test]
fn fts5_finds_by_name_code_and_barcode() {
    let db = Db::open_in_memory().unwrap();
    insert_catalog(&db);
    let hits: Vec<String> = db
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT sku_id FROM sku_fts WHERE sku_fts MATCH 'milk*'")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(hits, vec!["s1".to_string()]);
    let by_barcode: Vec<String> = db
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT sku_id FROM sku_fts WHERE sku_fts MATCH '1234567890123*'")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(by_barcode, vec!["s1".to_string()]);
}

#[test]
fn sqlcipher_key_roundtrip() {
    let dir = std::env::temp_dir().join(format!("shopcore-db-key-test-{}", uuid::Uuid::now_v7()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("till.db");
    let key = b"test-key-32-bytes-long-0123456789";
    {
        let db = Db::open(&path, Some(key)).unwrap();
        db.write(|conn| {
            conn.execute(
                "INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version) VALUES ('k1', 't1', 'tenant', '', 'probe', 'v', ?1, ?1, 1)",
                [NOW],
            )?;
            Ok(())
        })
        .unwrap();
    }
    // Same key re-opens and sees the row.
    let db = Db::open(&path, Some(key)).unwrap();
    let value: String = db
        .read(|conn| {
            conn.query_row(
                "SELECT value FROM settings WHERE key = 'probe'",
                [],
                |row| row.get(0),
            )
            .map_err(DbError::from)
        })
        .unwrap();
    assert_eq!(value, "v");
    drop(db);
    // A wrong key cannot read the database (SQLCipher rejects the first
    // read with a decryption error).
    let wrong = Db::open(&path, Some(b"wrong-key-32-bytes-long-abcdefg"));
    // Open itself failing is also acceptable; what matters is that a
    // wrong key can never read the database.
    if let Ok(db) = wrong {
        let read = db.read(|conn| {
            conn.query_row("SELECT COUNT(*) FROM settings", [], |row| {
                row.get::<_, i64>(0)
            })
            .map_err(DbError::from)
        });
        assert!(read.is_err(), "wrong key must not read the database");
    }
    std::fs::remove_dir_all(&dir).unwrap();
}
