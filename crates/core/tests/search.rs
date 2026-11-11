//! Search/lookup tests (spec 001b-1 req. 5–6): barcode lookup, prefix +
//! `FTS5` search, catalog-version invalidation of the in-memory index.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use shopcore_core::search::{PrefixIndex, SearchError, lookup_barcode, search_skus};
use shopcore_core::settings::get_catalog_version;

use common::{NOW, TENANT, fresh_db, insert_sku, setup_pricing};

fn seed_catalog(db: &shopcore_db::Db) -> (String, String) {
    let (tax_group_id, price_list_id) = setup_pricing(db);
    insert_sku(
        db,
        "MILK1L",
        "Milk 1L",
        "1234567890123",
        2_500_000,
        &tax_group_id,
        &price_list_id,
    );
    insert_sku(
        db,
        "BRD500",
        "Bread 500g",
        "1234567890124",
        1_200_000,
        &tax_group_id,
        &price_list_id,
    );
    insert_sku(
        db,
        "SGR1KG",
        "Sugar 1kg",
        "1234567890125",
        1_800_000,
        &tax_group_id,
        &price_list_id,
    );
    (tax_group_id, price_list_id)
}

#[test]
fn barcode_lookup_returns_sku_with_price() {
    let db = fresh_db();
    seed_catalog(&db);
    let hit = lookup_barcode(&db, TENANT, "1234567890123", NOW).unwrap();
    assert_eq!(hit.code, "MILK1L");
    assert_eq!(hit.name, "Milk 1L");
    assert_eq!(hit.price_e4, Some(2_500_000));
}

#[test]
fn unknown_barcode_is_barcode_not_found_with_the_code() {
    let db = fresh_db();
    seed_catalog(&db);
    let err = lookup_barcode(&db, TENANT, "0000000000000", NOW).unwrap_err();
    assert_eq!(err.code(), "BARCODE_NOT_FOUND");
    assert!(matches!(
        err,
        SearchError::BarcodeNotFound(ref code) if code == "0000000000000"
    ));
}

#[test]
fn search_finds_by_name_prefix_and_fts5() {
    let db = fresh_db();
    seed_catalog(&db);
    let mut index = PrefixIndex::new();
    // Prefix index path (>= 3 chars, word-prefix).
    let hits = search_skus(&db, TENANT, "mil", &mut index, NOW).unwrap();
    assert_eq!(hits.len(), 1);
    assert_eq!(hits.first().unwrap().code, "MILK1L");
    // FTS5 path: short query, and a mid-word term the prefix index
    // would not match on its own ("500g" is a word in "Bread 500g").
    let hits = search_skus(&db, TENANT, "500", &mut index, NOW).unwrap();
    assert!(hits.iter().any(|h| h.code == "BRD500"));
    // No match at all: empty, not an error.
    assert_eq!(
        search_skus(&db, TENANT, "zzzzzz", &mut index, NOW)
            .unwrap()
            .len(),
        0
    );
}

#[test]
fn catalog_write_bumps_version_and_stale_index_is_never_served() {
    let db = fresh_db();
    let (tax_group_id, price_list_id) = seed_catalog(&db);
    let v0 = get_catalog_version(&db, TENANT).unwrap();
    assert!(v0 > 0, "catalog writes must bump the version counter");
    // Build the index, then add a SKU. The next query must see it —
    // the version mismatch forces a rebuild before the query runs.
    let mut index = PrefixIndex::new();
    let hits = search_skus(&db, TENANT, "mil", &mut index, NOW).unwrap();
    assert_eq!(hits.len(), 1);
    insert_sku(
        &db,
        "MILK2L",
        "Milk 2L",
        "1234567890999",
        4_800_000,
        &tax_group_id,
        &price_list_id,
    );
    let v1 = get_catalog_version(&db, TENANT).unwrap();
    assert!(
        v1 > v0,
        "version {v0} did not bump after a catalog write (now {v1})"
    );
    let hits = search_skus(&db, TENANT, "mil", &mut index, NOW).unwrap();
    assert_eq!(index.version(), v1, "the query must have rebuilt the index");
    assert_eq!(hits.len(), 2);
    assert!(hits.iter().any(|h| h.code == "MILK2L"));
}
