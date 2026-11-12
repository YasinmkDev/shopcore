//! Cart pricing tests (spec 001c-1, req. 5): the sell screen's totals
//! come from [`price_draft_cart`], which prices a draft through the
//! 001a domain functions — the same path the sale transaction uses,
//! so the totals the cashier sees are the totals the sale commits.
//! Expected values are the 001a goldens, verified by hand there.
#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use common::{
    NOW, TENANT, fresh_db, insert_sku, setup_pricing, test_root_scratch, unique_test_dir,
};
use shopcore_core::cart::price_draft_cart;
use shopcore_core::draft::{draft_add_line, open_draft};
use shopcore_core::scratch::ScratchDb;
use shopcore_core::shift::open_shift_id;
use shopcore_db::Db;
use std::path::PathBuf;

/// A unique scratch directory under the workspace `target/` (real disk).
/// Removes the dir when dropped (best effort).
struct DirGuard(PathBuf);

impl Drop for DirGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// A priced cart fixture: exclusive (STD 18%) + inclusive (INC 18%) +
/// zero-rated groups, one SKU each, on one draft.
struct CartFixture {
    db: Db,
    scratch: ScratchDb,
    _guard: DirGuard,
    draft_id: String,
    exc_sku: String,
    inc_sku: String,
    zero_sku: String,
}

impl CartFixture {
    fn new(label: &str) -> Self {
        let db = fresh_db();
        let (exc_group, price_list_id) = setup_pricing(&db);
        // Inclusive 18% group + zero-rated group (no rates = exempt).
        let inc_group = uuid::Uuid::now_v7().to_string();
        let zero_group = uuid::Uuid::now_v7().to_string();
        let inc_group_c = inc_group.clone();
        let zero_group_c = zero_group.clone();
        db.write(move |conn| {
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO tax_group (id, tenant_id, code, name, price_includes_tax, created_at, updated_at, version) VALUES (?1, ?2, 'INC', 'Inclusive 18%', 1, ?3, ?3, 1)",
                rusqlite::params![inc_group_c, TENANT, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), TENANT, inc_group_c, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'ZERO', 'Zero rated', ?3, ?3, 1)",
                rusqlite::params![zero_group_c, TENANT, NOW],
            )
            .unwrap();
            tx.commit().unwrap();
            Ok(())
        })
        .unwrap();
        // 001a goldens: TST100 = 100.00 exclusive, TST118 = 118.00
        // inclusive, TST010 = 10.00 zero-rated.
        let exc_sku = insert_sku(
            &db,
            "TST100",
            "Test 100",
            "1234567890100",
            1_000_000,
            &exc_group,
            &price_list_id,
        );
        let inc_sku = insert_sku(
            &db,
            "TST118",
            "Test 118",
            "1234567890118",
            1_180_000,
            &inc_group,
            &price_list_id,
        );
        let zero_sku = insert_sku(
            &db,
            "TST010",
            "Test 010",
            "1234567890010",
            100_000,
            &zero_group,
            &price_list_id,
        );
        let dir = unique_test_dir(&test_root_scratch(), label);
        let guard = DirGuard(dir.clone());
        let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
        let draft =
            open_draft(&scratch, TENANT, "branch-1", "terminal-1", None, None, NOW).unwrap();
        Self {
            db,
            scratch,
            _guard: guard,
            draft_id: draft.id,
            exc_sku,
            inc_sku,
            zero_sku,
        }
    }

    fn add(&self, sku_id: &str, qty_milli: i64, unit_price_e4: i64) {
        draft_add_line(
            &self.scratch,
            &self.draft_id,
            sku_id,
            qty_milli,
            unit_price_e4,
            NOW,
        )
        .unwrap();
    }
}

#[test]
fn exclusive_cart_prices_like_the_sale() {
    // 001a golden: net 100.00 @18% exclusive → tax 18.00, total 118.00.
    let fx = CartFixture::new("cart-exclusive");
    fx.add(&fx.exc_sku, 1_000, 1_000_000);
    let draft = shopcore_core::draft::load_draft(&fx.scratch, &fx.draft_id)
        .unwrap()
        .unwrap();
    let priced = price_draft_cart(&fx.db, &draft).unwrap();
    assert_eq!(priced.subtotal_minor, 10_000);
    assert_eq!(priced.discount_minor, 0);
    assert_eq!(priced.tax_minor, 1_800);
    assert_eq!(priced.total_minor, 11_800);
    assert_eq!(priced.currency_exponent, 2);
    assert_eq!(priced.lines.len(), 1);
    assert_eq!(priced.lines[0].name, "Test 100");
    assert_eq!(priced.lines[0].line_total_minor, 11_800);
}

#[test]
fn inclusive_cart_prices_like_the_sale() {
    // 001a golden: gross 118.00 @18% inclusive → net 100.00, tax 18.00.
    // The subtotal is the sum of line GROSSES, so it is the shelf
    // price (11800), not the net.
    let fx = CartFixture::new("cart-inclusive");
    fx.add(&fx.inc_sku, 1_000, 1_180_000);
    let draft = shopcore_core::draft::load_draft(&fx.scratch, &fx.draft_id)
        .unwrap()
        .unwrap();
    let priced = price_draft_cart(&fx.db, &draft).unwrap();
    assert_eq!(priced.subtotal_minor, 11_800);
    assert_eq!(priced.tax_minor, 1_800);
    assert_eq!(priced.total_minor, 11_800);
    assert_eq!(priced.lines[0].line_total_minor, 11_800);
}

#[test]
fn mixed_mode_cart_uses_each_lines_own_flag() {
    let fx = CartFixture::new("cart-mixed");
    fx.add(&fx.exc_sku, 1_000, 1_000_000);
    fx.add(&fx.inc_sku, 1_000, 1_180_000);
    let draft = shopcore_core::draft::load_draft(&fx.scratch, &fx.draft_id)
        .unwrap()
        .unwrap();
    let priced = price_draft_cart(&fx.db, &draft).unwrap();
    assert_eq!(priced.subtotal_minor, 21_800);
    assert_eq!(priced.tax_minor, 3_600);
    assert_eq!(priced.total_minor, 23_600);
}

#[test]
fn zero_rated_cart_has_no_tax() {
    let fx = CartFixture::new("cart-zero");
    fx.add(&fx.zero_sku, 1_000, 100_000);
    let draft = shopcore_core::draft::load_draft(&fx.scratch, &fx.draft_id)
        .unwrap()
        .unwrap();
    let priced = price_draft_cart(&fx.db, &draft).unwrap();
    assert_eq!(priced.subtotal_minor, 1_000);
    assert_eq!(priced.tax_minor, 0);
    assert_eq!(priced.total_minor, 1_000);
}

#[test]
fn empty_cart_prices_to_zero() {
    let fx = CartFixture::new("cart-empty");
    let draft = shopcore_core::draft::load_draft(&fx.scratch, &fx.draft_id)
        .unwrap()
        .unwrap();
    let priced = price_draft_cart(&fx.db, &draft).unwrap();
    assert_eq!(priced.subtotal_minor, 0);
    assert_eq!(priced.discount_minor, 0);
    assert_eq!(priced.tax_minor, 0);
    assert_eq!(priced.total_minor, 0);
    assert!(priced.lines.is_empty());
}

#[test]
fn missing_sku_is_an_error() {
    let fx = CartFixture::new("cart-missing-sku");
    fx.add("sku-does-not-exist", 1_000, 1_000_000);
    let draft = shopcore_core::draft::load_draft(&fx.scratch, &fx.draft_id)
        .unwrap()
        .unwrap();
    let err = price_draft_cart(&fx.db, &draft).unwrap_err();
    assert!(
        err.to_string().contains("sku-does-not-exist"),
        "unexpected error: {err}"
    );
}

#[test]
fn open_shift_id_reads_the_open_shift() {
    let fx = CartFixture::new("cart-shift");
    // No shift row yet: none open.
    assert_eq!(open_shift_id(&fx.db, TENANT, "terminal-1").unwrap(), None);
    let shift_id = uuid::Uuid::now_v7().to_string();
    let shift_id_c = shift_id.clone();
    fx.db
        .write(move |conn| {
            conn.execute(
                "INSERT INTO shift (id, tenant_id, branch_id, terminal_id, status, opened_at, created_at, updated_at, version) VALUES (?1, ?2, 'branch-1', 'terminal-1', 'open', ?3, ?3, ?3, 1)",
                rusqlite::params![shift_id_c, TENANT, NOW],
            )?;
            Ok(())
        })
        .unwrap();
    assert_eq!(
        open_shift_id(&fx.db, TENANT, "terminal-1").unwrap(),
        Some(shift_id)
    );
    // Another terminal has no open shift.
    assert_eq!(open_shift_id(&fx.db, TENANT, "terminal-2").unwrap(), None);
}
