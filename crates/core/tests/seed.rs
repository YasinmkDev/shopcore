//! Dev-seed test (spec 001b-1 req. 11). Compiled ONLY with the `dev-seed`
//! cargo feature — `cargo nextest run -p shopcore-core --features dev-seed`.

#![cfg(feature = "dev-seed")]
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use shopcore_core::search::lookup_barcode;
use shopcore_core::seed::{DEV_SEED_MARKER, sample_barcodes, seed_dev_till};
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

#[test]
fn sample_barcodes_lists_the_seeded_catalog() {
    // The DEV BUILD banner's list (001c-1 follow-up 4): every seeded
    // SKU with its barcode, shelf price in minor units, and tax mode.
    // Prices come from the live catalog through the domain rounding
    // rule, so the list always matches what the till scans.
    let db = fresh_db();
    seed_dev_till(&db, TENANT, NOW).unwrap();
    let rows = sample_barcodes(&db, TENANT).unwrap();
    assert_eq!(rows.len(), 6);
    let by_barcode = |barcode: &str| {
        rows.iter()
            .find(|row| row.barcode == barcode)
            .unwrap_or_else(|| panic!("missing barcode {barcode}"))
    };
    let milk = by_barcode("1234567890123");
    assert_eq!(milk.name, "Milk 1L");
    assert_eq!(milk.price_minor, 25_000);
    assert_eq!(milk.tax_mode, "exclusive");
    assert_eq!(milk.rate_bp, 1800);
    let bread = by_barcode("1234567890124");
    assert_eq!(bread.name, "Bread 500g");
    assert_eq!(bread.price_minor, 12_000);
    assert_eq!(bread.tax_mode, "zero");
    assert_eq!(bread.rate_bp, 0);
    let sugar = by_barcode("1234567890125");
    assert_eq!(sugar.price_minor, 18_000);
    assert_eq!(sugar.tax_mode, "exclusive");
    let rice = by_barcode("1234567890126");
    assert_eq!(rice.price_minor, 95_000);
    assert_eq!(rice.tax_mode, "zero");
    let tea = by_barcode("1234567890127");
    assert_eq!(tea.price_minor, 9_500);
    assert_eq!(tea.tax_mode, "exclusive");
    let eggs = by_barcode("1234567890128");
    assert_eq!(eggs.price_minor, 36_000);
    assert_eq!(eggs.tax_mode, "zero");
}

#[test]
fn sample_barcodes_is_empty_without_an_active_price_list() {
    // No seed, no price list: an empty list, not an error.
    let db = fresh_db();
    assert!(sample_barcodes(&db, TENANT).unwrap().is_empty());
}
