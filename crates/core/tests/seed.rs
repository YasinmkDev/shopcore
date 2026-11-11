//! Dev-seed test (spec 001b-1 req. 11). Compiled ONLY with the `dev-seed`
//! cargo feature — `cargo nextest run -p shopcore-core --features dev-seed`.

#![cfg(feature = "dev-seed")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use shopcore_core::search::lookup_barcode;
use shopcore_core::seed::{DEV_SEED_MARKER, seed_dev_till};
use shopcore_core::users::verify_pin;

use common::{NOW, TENANT, fresh_db};

#[test]
fn seed_produces_a_usable_till() {
    assert_eq!(DEV_SEED_MARKER, "SHOPCORE_DEV_SEED_MARKER_V1");
    let db = fresh_db();
    let report = seed_dev_till(&db, TENANT, NOW).unwrap();
    assert_eq!(report.sku_count, 6);
    // The seeded users can sign in with the documented dev PINs.
    assert_eq!(
        verify_pin(&db, TENANT, "cashier", "1234", NOW)
            .unwrap()
            .username,
        "cashier"
    );
    assert_eq!(
        verify_pin(&db, TENANT, "manager", "4321", NOW)
            .unwrap()
            .username,
        "manager"
    );
    // The seeded catalog is searchable by barcode, with a price.
    let hit = lookup_barcode(&db, TENANT, "1234567890123", NOW).unwrap();
    assert_eq!(hit.name, "Milk 1L");
    assert!(hit.price_e4.is_some());
    // Branch, terminal and an open shift exist.
    let (branch, terminal, shift) = db
        .read(move |conn| {
            let branch: i64 = conn.query_row(
                "SELECT COUNT(*) FROM branch WHERE id = ?1",
                rusqlite::params![report.branch_id],
                |row| row.get(0),
            )?;
            let terminal: i64 = conn.query_row(
                "SELECT COUNT(*) FROM terminal WHERE id = ?1",
                rusqlite::params![report.terminal_id],
                |row| row.get(0),
            )?;
            let shift: i64 = conn.query_row(
                "SELECT COUNT(*) FROM shift WHERE id = ?1 AND status = 'open'",
                rusqlite::params![report.shift_id],
                |row| row.get(0),
            )?;
            Ok((branch, terminal, shift))
        })
        .unwrap();
    assert_eq!((branch, terminal, shift), (1, 1, 1));
}
