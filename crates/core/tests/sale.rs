//! 001b-2 integration tests: the sale transaction, draft cart, audit
//! hash chain, outbox, aggregates, negative stock, permissions, and
//! hardware — against a real `SQLite` database with the 001b-1
//! migrations applied, a real scratch DB, and fake clock/ids/hardware.
//!
//! Fixture strategy (001b-2 decision): the dev seed is behind the
//! `dev-seed` feature and `just check` runs nextest WITHOUT it, so these
//! tests build their own fixture mirroring the seed's values (branch
//! `MAIN` / terminal `T1`, open shift, cashier `1234` / manager `4321`,
//! `STD` 18% + `ZERO` tax groups, the same 6 `SKU`s). One template DB
//! file is built once per test binary (a `OnceLock`) and COPIED per
//! test, so the `Argon2id` PIN hashes (~1.7 s each in debug) are paid
//! once, not per test. The scratch DB is fresh per test.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

mod common;

use common::{test_root_001b2, unique_test_dir};
use shopcore_core::audit::verify_audit_chain;
use shopcore_core::draft::{
    draft_add_line, draft_apply_discount, draft_cancel, draft_remove_line, draft_restore_line,
    draft_set_qty, load_draft, open_draft, restore_draft,
};
use shopcore_core::sale::{
    CompleteSaleCashInput, FaultInjector, PrintStatus, STEP_AGGREGATES, STEP_AUDIT, STEP_COMMIT,
    STEP_IDEMPOTENCY, STEP_LINES, STEP_OUTBOX, STEP_PAYMENT, STEP_PRICING, STEP_PRINT_JOB,
    STEP_RECEIPT_COUNTER, STEP_SALE, STEP_SHIFT, STEP_STOCK, STEP_TAX_ROWS, SaleError,
    attempt_print, complete_sale_cash, kick_drawer, negative_stock_skus, reprint_receipt,
};
use shopcore_core::scratch::ScratchDb;
use shopcore_core::search::lookup_barcode;
use shopcore_core::settings::{
    KEY_ACTIVE_PRICE_LIST, KEY_CASH_ROUNDING_GRANULARITY_MINOR, KEY_TAX_DEFAULT_RATE_BP,
    KEY_TENANT_CURRENCY, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, set_setting,
};
use shopcore_core::users::{User, create_role, create_user, get_user_by_username};
use shopcore_db::Db;
use shopcore_domain::LineDiscount;
use shopcore_hardware::{BarcodeScanner, MockDrawer, MockPrinter, MockScanner, ScanEvent};
use std::path::PathBuf;
use std::sync::OnceLock;

/// The fake clock: every timestamp in the fixture is this epoch-ms.
/// (The `date_yyyymmdd` the sale takes is a separate input.)
const NOW: i64 = 1_800_000_000_000;
/// The sale date (receipt numbers read `MAIN-T1-20261111-1`, ...).
const DATE: &str = "20261111";
const TENANT: &str = "tenant-001b2";
const BRANCH_ID: &str = "branch-main";
const TERMINAL_ID: &str = "terminal-t1";
const BRANCH_CODE: &str = "MAIN";
const TERMINAL_CODE: &str = "T1";
const SHIFT_ID: &str = "shift-001";
const TAX_STD_ID: &str = "tax-std";
const TAX_ZERO_ID: &str = "tax-zero";
const TAX_INC_ID: &str = "tax-inc";
const TAX_RATE_STD_ID: &str = "taxrate-std-18";
const TAX_RATE_INC_ID: &str = "taxrate-inc-18";
const PRICE_LIST_ID: &str = "pricelist-std";
/// The `SQLCipher` key for test DBs (a hardcoded test key, same as the
/// `db`-crate tests — never a production key).
const TEST_KEY: &[u8] = b"001b2-test-key-32-bytes-long-abc";

/// (`sku_id`, code, barcode, `price_e4`, tax group, stock `qty_milli`).
/// The first six mirror the dev seed; `TST44` is a test-only `SKU`
/// priced so one unit + 18% tax totals exactly 5232 minor (the 001a
/// cash acceptance: total 52.32, tender 60.00, change 7.68).
const CATALOG: &[(&str, &str, &str, i64, &str, i64)] = &[
    (
        "sku-milk1l",
        "MILK1L",
        "1234567890123",
        2_500_000,
        "STD",
        100_000,
    ),
    (
        "sku-brd500",
        "BRD500",
        "1234567890124",
        1_200_000,
        "ZERO",
        80_000,
    ),
    (
        "sku-sgr1kg",
        "SGR1KG",
        "1234567890125",
        1_800_000,
        "STD",
        50_000,
    ),
    (
        "sku-rce5kg",
        "RCE5KG",
        "1234567890126",
        9_500_000,
        "ZERO",
        40_000,
    ),
    (
        "sku-tea200",
        "TEA200",
        "1234567890127",
        950_000,
        "STD",
        120_000,
    ),
    (
        "sku-egg12",
        "EGG12",
        "1234567890128",
        3_600_000,
        "ZERO",
        60_000,
    ),
    (
        "sku-tst44",
        "TST44",
        "1234567890999",
        443_400,
        "STD",
        10_000,
    ),
    // Golden-value SKUs for the tax-mode tests (001a goldens):
    // TST100 is 100.00 exclusive @18% (tax 18.00, total 118.00);
    // TST118 is 118.00 inclusive @18% (net 100.00, tax 18.00);
    // TST010 is 10.00 zero-rated (net-cash test).
    (
        "sku-tst100",
        "TST100",
        "1234567890998",
        1_000_000,
        "STD",
        10_000,
    ),
    (
        "sku-tst118",
        "TST118",
        "1234567890997",
        1_180_000,
        "INC",
        10_000,
    ),
    (
        "sku-tst010",
        "TST010",
        "1234567890996",
        100_000,
        "ZERO",
        10_000,
    ),
];

fn sku_id(code: &str) -> &'static str {
    CATALOG
        .iter()
        .find(|(_, c, _, _, _, _)| *c == code)
        .unwrap()
        .0
}

fn sku_price_e4(code: &str) -> i64 {
    CATALOG
        .iter()
        .find(|(_, c, _, _, _, _)| *c == code)
        .unwrap()
        .3
}

fn sku_barcode(code: &str) -> &'static str {
    CATALOG
        .iter()
        .find(|(_, c, _, _, _, _)| *c == code)
        .unwrap()
        .2
}

/// The template DB (built once) + the user ids it holds.
struct TemplateInfo {
    /// Directory holding the template `till.db`.
    dir: PathBuf,
    cashier: User,
    viewer: User,
}

static TEMPLATE: OnceLock<TemplateInfo> = OnceLock::new();

fn template() -> &'static TemplateInfo {
    TEMPLATE.get_or_init(|| {
        // Built once per test binary (OnceLock) and NEVER written to after
        // building: the DB is dropped after checkpointing, and every test
        // copies the file into its own fresh dir. The dir is removed by
        // the 1-hour stale-cleanup on a later run (a process-exit hook
        // would need `unsafe`, which the workspace forbids — ADR-035);
        // `just clean-test-tmp` removes it on demand.
        let dir = unique_test_dir(&test_root_001b2(), "template");
        let db_path = dir.join("till.db");
        let db = Db::open(&db_path, Some(TEST_KEY)).unwrap();
        let (cashier, viewer) = seed_template_users(&db);
        seed_template_org(&db);
        seed_template_catalog(&db);
        seed_template_settings(&db);
        // Checkpoint the WAL synchronously on the actor thread BEFORE
        // dropping the `Db`: dropping it only signals the actor thread
        // to close (it does not wait), so without this the WAL may
        // still hold rows when the file is copied below — and deleting
        // the `-wal` sidecar would then LOSE them. The checkpoint
        // copies every WAL frame into the main file, so the copy is
        // complete and the sidecars are safe to drop.
        db.write(|conn| {
            conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
                .map_err(shopcore_db::DbError::from)?;
            Ok(())
        })
        .unwrap();
        // Close the template DB (checkpoints the WAL) before anyone
        // copies it; drop the `-wal`/`-shm` sidecars so a copy is a
        // single self-contained file.
        drop(db);
        let _ = std::fs::remove_file(db_path.with_extension("db-wal"));
        let _ = std::fs::remove_file(db_path.with_extension("db-shm"));
        TemplateInfo {
            dir,
            cashier,
            viewer,
        }
    })
}

/// Roles + users for the template (PINs hashed with Argon2id — the
/// slow step, paid once per test binary, not per test). Returns the
/// cashier and the viewer; the manager is created but not returned
/// (discount tests look it up by username).
fn seed_template_users(db: &Db) -> (User, User) {
    let cashier_role = create_role(
        db,
        TENANT,
        "Cashier",
        &["sale.create", "sale.discount.line"],
        500,
        NOW,
    )
    .unwrap();
    let manager_role = create_role(
        db,
        TENANT,
        "Store Manager",
        &[
            "sale.create",
            "sale.discount.line",
            "sale.discount.bill",
            "sale.price_override",
            "sale.void_line",
            "shift.close",
            "cash.drawer_open",
            "stock.adjust",
            "user.manage",
            "settings.edit",
        ],
        2000,
        NOW,
    )
    .unwrap();
    let viewer_role = create_role(db, TENANT, "Viewer", &[], 0, NOW).unwrap();
    let cashier = create_user(db, TENANT, "cashier", "1234", Some(&cashier_role.id), NOW).unwrap();
    let _manager = create_user(db, TENANT, "manager", "4321", Some(&manager_role.id), NOW).unwrap();
    let viewer = create_user(db, TENANT, "viewer", "0000", Some(&viewer_role.id), NOW).unwrap();
    (cashier, viewer)
}

/// Branch, terminal, tax groups + rate, `UoM`s, price list, shift.
fn seed_template_org(db: &Db) {
    db.write(|conn| {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO branch (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'MAIN', 'Main Branch', ?3, ?3, 1)",
            rusqlite::params![BRANCH_ID, TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO terminal (id, tenant_id, branch_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, ?3, 'T1', 'Till 1', ?4, ?4, 1)",
            rusqlite::params![TERMINAL_ID, TENANT, BRANCH_ID, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard 18%', ?3, ?3, 1)",
            rusqlite::params![TAX_STD_ID, TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'ZERO', 'Zero rated', ?3, ?3, 1)",
            rusqlite::params![TAX_ZERO_ID, TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_group (id, tenant_id, code, name, price_includes_tax, created_at, updated_at, version) VALUES (?1, ?2, 'INC', 'Inclusive 18%', 1, ?3, ?3, 1)",
            rusqlite::params![TAX_INC_ID, TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
            rusqlite::params![TAX_RATE_INC_ID, TENANT, TAX_INC_ID, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
            rusqlite::params![TAX_RATE_STD_ID, TENANT, TAX_STD_ID, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO uom (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('uom-ea', ?1, 'EA', 'Each', ?2, ?2, 1)",
            rusqlite::params![TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO uom (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('uom-kg', ?1, 'KG', 'Kilogram', ?2, ?2, 1)",
            rusqlite::params![TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO price_list (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard', ?3, ?3, 1)",
            rusqlite::params![PRICE_LIST_ID, TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO shift (id, tenant_id, branch_id, terminal_id, status, opened_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5, ?5, 1)",
            rusqlite::params![SHIFT_ID, TENANT, BRANCH_ID, TERMINAL_ID, NOW],
        )
        .unwrap();
        tx.commit().unwrap();
        Ok(())
    })
    .unwrap();
}

/// Catalog + opening stock (balance == sum(movements) from the start).
fn seed_template_catalog(db: &Db) {
    for (sku, code, barcode, price_e4, tax, qty_milli) in CATALOG {
        let tax_group = match *tax {
            "STD" => TAX_STD_ID,
            "INC" => TAX_INC_ID,
            _ => TAX_ZERO_ID,
        };
        let uom = if *code == "SGR1KG" || *code == "RCE5KG" {
            "uom-kg"
        } else {
            "uom-ea"
        };
        let item_id = format!("item-{code}");
        let sku = sku.to_string();
        let code = code.to_string();
        let barcode = barcode.to_string();
        let price_e4 = *price_e4;
        let qty_milli = *qty_milli;
        db.write(move |conn| {
            let tx = conn.transaction().unwrap();
            tx.execute(
                "INSERT INTO item (id, tenant_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
                rusqlite::params![item_id, TENANT, code, code, uom, tax_group, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)",
                rusqlite::params![sku, TENANT, item_id, code, code, uom, tax_group, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5, 1)",
                rusqlite::params![format!("barcode-{code}"), TENANT, sku, barcode, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO price_list_entry (id, tenant_id, price_list_id, sku_id, price_e4, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                rusqlite::params![format!("ple-{code}"), TENANT, PRICE_LIST_ID, sku, price_e4, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO stock_balance (id, tenant_id, branch_id, sku_id, qty_milli, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                rusqlite::params![format!("bal-{code}"), TENANT, BRANCH_ID, sku, qty_milli, NOW],
            )
            .unwrap();
            tx.execute(
                "INSERT INTO stock_movement (id, tenant_id, branch_id, sku_id, type, qty_milli, unit_cost_e4, occurred_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 'opening', ?5, 0, ?6, ?6, ?6, 1)",
                rusqlite::params![format!("mov-open-{code}"), TENANT, BRANCH_ID, sku, qty_milli, NOW],
            )
            .unwrap();
            tx.commit().unwrap();
            Ok(())
        })
        .unwrap();
    }
}

/// Tenant settings: PKR, exponent 2, cash rounding OFF, tax 18%,
/// active price list = the seeded one.
fn seed_template_settings(db: &Db) {
    set_setting(
        db,
        TENANT,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY,
        "PKR",
        NOW,
    )
    .unwrap();
    set_setting(
        db,
        TENANT,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
        "2",
        NOW,
    )
    .unwrap();
    set_setting(
        db,
        TENANT,
        SCOPE_TENANT,
        "",
        KEY_CASH_ROUNDING_GRANULARITY_MINOR,
        "0",
        NOW,
    )
    .unwrap();
    set_setting(
        db,
        TENANT,
        SCOPE_TENANT,
        "",
        KEY_TAX_DEFAULT_RATE_BP,
        "1800",
        NOW,
    )
    .unwrap();
    set_setting(
        db,
        TENANT,
        SCOPE_TENANT,
        "",
        KEY_ACTIVE_PRICE_LIST,
        PRICE_LIST_ID,
        NOW,
    )
    .unwrap();
}
/// One test's databases: a COPY of the template main DB + a fresh
/// scratch DB. Dropped (and its directory removed) at the end of the
/// test.
struct Fixture {
    dir: PathBuf,
    db: Db,
    scratch: ScratchDb,
}

impl Fixture {
    fn new(label: &str) -> Self {
        let info = template();
        let dir = unique_test_dir(&test_root_001b2(), label);
        std::fs::copy(info.dir.join("till.db"), dir.join("till.db")).unwrap();
        let db = Db::open(dir.join("till.db"), Some(TEST_KEY)).unwrap();
        let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
        Self { dir, db, scratch }
    }

    /// Open a draft as the cashier and add `(code, qty_milli)` lines.
    fn draft_with(&self, lines: &[(&str, i64)]) -> String {
        let cart = open_draft(
            &self.scratch,
            TENANT,
            BRANCH_ID,
            TERMINAL_ID,
            Some(SHIFT_ID),
            Some(&template().cashier.id),
            NOW,
        )
        .unwrap();
        for (code, qty) in lines {
            draft_add_line(
                &self.scratch,
                &cart.id,
                sku_id(code),
                *qty,
                sku_price_e4(code),
                NOW,
            )
            .unwrap();
        }
        cart.id
    }

    /// Complete the draft as the cashier with the given tender.
    fn complete(
        &self,
        draft_id: &str,
        tender_minor: i64,
    ) -> Result<shopcore_core::sale::SaleCompleted, SaleError> {
        complete_sale_cash(
            &self.db,
            &self.scratch,
            CompleteSaleCashInput {
                tenant_id: TENANT.to_string(),
                branch_id: BRANCH_ID.to_string(),
                terminal_id: TERMINAL_ID.to_string(),
                branch_code: BRANCH_CODE.to_string(),
                terminal_code: TERMINAL_CODE.to_string(),
                shift_id: SHIFT_ID.to_string(),
                user_id: template().cashier.id.clone(),
                draft_id: draft_id.to_string(),
                tender_minor,
                now_ms: NOW,
                date_yyyymmdd: DATE.to_string(),
            },
            None,
        )
    }

    /// `SELECT COUNT(*)` helper.
    fn count(&self, sql: &str) -> i64 {
        self.db
            .read(|conn| {
                conn.query_row(sql, [], |row| row.get::<_, i64>(0))
                    .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }

    /// The stock balance (milli) for a `SKU` code.
    fn balance(&self, code: &str) -> i64 {
        let sku = sku_id(code).to_string();
        self.db
            .read(move |conn| {
                conn.query_row(
                    "SELECT qty_milli FROM stock_balance WHERE tenant_id = ?1 AND branch_id = ?2 AND sku_id = ?3",
                    rusqlite::params![TENANT, BRANCH_ID, sku],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }

    /// Sum of `stock_movements` (milli) for a `SKU` code.
    fn ledger_sum(&self, code: &str) -> i64 {
        let sku = sku_id(code).to_string();
        self.db
            .read(move |conn| {
                conn.query_row(
                    "SELECT COALESCE(SUM(qty_milli), 0) FROM stock_movement WHERE tenant_id = ?1 AND branch_id = ?2 AND sku_id = ?3",
                    rusqlite::params![TENANT, BRANCH_ID, sku],
                    |row| row.get::<_, i64>(0),
                )
                .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }

    /// The sale row for a receipt number: (subtotal, discount, tax, total).
    fn sale_totals(&self, receipt_no: &str) -> (i64, i64, i64, i64) {
        let receipt_no = receipt_no.to_string();
        self.db
            .read(move |conn| {
                conn.query_row(
                    "SELECT subtotal_minor, discount_minor, tax_minor, total_minor FROM sale WHERE receipt_no = ?1",
                    rusqlite::params![receipt_no],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0)?,
                            row.get::<_, i64>(1)?,
                            row.get::<_, i64>(2)?,
                            row.get::<_, i64>(3)?,
                        ))
                    },
                )
                .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }

    /// Per-component tax rows: (line tax, tax-row tax) pairs, in line
    /// order — they must be equal per line.
    fn tax_rows(&self) -> Vec<(i64, i64)> {
        self.db
            .read(|conn| {
                let mut stmt = conn
                    .prepare(
                        "SELECT sl.tax_minor, slt.tax_minor FROM sale_line sl JOIN sale_line_tax slt ON slt.sale_line_id = sl.id ORDER BY sl.rowid",
                    )
                    .unwrap();
                let rows = stmt
                    .query_map([], |row| {
                        Ok((row.get::<_, i64>(0).unwrap(), row.get::<_, i64>(1).unwrap()))
                    })
                    .unwrap();
                let mut out = Vec::new();
                for row in rows {
                    out.push(row.unwrap());
                }
                Ok(out)
            })
            .unwrap()
    }

    /// The `daily_cash_summary` row for the sale date:
    /// (`cash_sales_minor`, `expected_minor`).
    fn daily_cash(&self) -> (i64, i64) {
        self.db
            .read(|conn| {
                conn.query_row(
                    "SELECT cash_sales_minor, expected_minor FROM daily_cash_summary WHERE date = '20261111'",
                    [],
                    |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?)),
                )
                .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }

    /// The `daily_sales_summary` row for the sale date:
    /// (`sale_count`, subtotal, discount, tax, total).
    fn daily_summary(&self) -> (i64, i64, i64, i64, i64) {
        self.db
            .read(|conn| {
                conn.query_row(
                    "SELECT sale_count, subtotal_minor, discount_minor, tax_minor, total_minor FROM daily_sales_summary WHERE date = '20261111'",
                    [],
                    |row| {
                        Ok((
                            row.get::<_, i64>(0).unwrap(),
                            row.get::<_, i64>(1).unwrap(),
                            row.get::<_, i64>(2).unwrap(),
                            row.get::<_, i64>(3).unwrap(),
                            row.get::<_, i64>(4).unwrap(),
                        ))
                    },
                )
                .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }

    /// The `daily_cash_summary` row for the sale date:
    /// (`cash_sales`, expected).
    fn cash_summary(&self) -> (i64, i64) {
        self.db
            .read(|conn| {
                conn.query_row(
                    "SELECT cash_sales_minor, expected_minor FROM daily_cash_summary WHERE date = '20261111'",
                    [],
                    |row| Ok((row.get::<_, i64>(0).unwrap(), row.get::<_, i64>(1).unwrap())),
                )
                .map_err(shopcore_db::DbError::from)
            })
            .unwrap()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        // Best-effort cleanup (works on Linux even with the DBs open;
        // on Windows the files may linger under `target/`, harmless).
        let _ = std::fs::remove_dir_all(&self.dir);
    }
}

// ---------------------------------------------------------------------------
// Sale transaction
// ---------------------------------------------------------------------------

#[test]
fn sale_completes_and_writes_everything() {
    let fx = Fixture::new("sale-complete");
    // MILK1L x2 (250.00 ea, STD 18%) + TEA200 x1 (95.00, STD 18%):
    // subtotal 59,500; tax 9,000 + 1,710 = 10,710; total 70,210.
    let draft_id = fx.draft_with(&[("MILK1L", 2_000), ("TEA200", 1_000)]);
    let done = fx.complete(&draft_id, 100_000).unwrap();
    assert_eq!(done.receipt_no, "MAIN-T1-20261111-1");
    assert_eq!(done.total_minor, 70_210);
    assert_eq!(done.change_minor, 29_790);
    assert_eq!(done.rounding_line_minor, 0);
    assert!(done.is_fresh);
    assert!(done.receipt_queued);
    assert_eq!(done.print_doc.total_minor, 70_210);
    assert_eq!(done.print_doc.subtotal_minor, 59_500);
    assert_eq!(done.print_doc.tax_minor, 10_710);
    assert_eq!(done.print_doc.payments.len(), 1);
    assert_eq!(done.print_doc.payments[0].amount_minor, 100_000);

    // The sale row.
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 1);
    assert_eq!(
        fx.sale_totals("MAIN-T1-20261111-1"),
        (59_500, 0, 10_710, 70_210)
    );
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale_line"), 2);
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale_payment"), 1);
    // Payment stores the TENDER (100,000), not the total.
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM sale_payment WHERE amount_minor = 100000"),
        1
    );

    // Per-component tax rows sum to the line tax.
    assert_eq!(fx.tax_rows(), vec![(9_000, 9_000), (1_710, 1_710)]);

    // Stock: balances moved by exactly the sold quantities.
    assert_eq!(fx.balance("MILK1L"), 98_000);
    assert_eq!(fx.balance("TEA200"), 119_000);
    assert_eq!(fx.ledger_sum("MILK1L"), 98_000);
    assert_eq!(fx.ledger_sum("TEA200"), 119_000);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM stock_movement WHERE type = 'sale'"),
        2
    );

    // Aggregates, all in the same transaction.
    assert_eq!(fx.daily_summary(), (1, 59_500, 0, 10_710, 70_210));
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM daily_item_sales WHERE date = '20261111'"),
        2
    );
    assert_eq!(fx.cash_summary(), (70_210, 70_210));

    // Audit: one `sale.completed` row, chain valid.
    assert_eq!(fx.count("SELECT COUNT(*) FROM audit_log"), 1);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'sale.completed'"),
        1
    );
    assert_eq!(verify_audit_chain(&fx.db, TENANT, TERMINAL_ID).unwrap(), 1);

    // Outbox: one sale row + one stock row per SKU (spec-005 columns:
    // the stock rows ride stream `stock_movement`).
    assert_eq!(fx.count("SELECT COUNT(*) FROM outbox"), 3);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM outbox WHERE entity = 'sale'"),
        1
    );
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM outbox WHERE entity = 'stock_movement'"),
        2
    );
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM outbox WHERE stream = 'sale'"),
        1
    );
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM outbox WHERE stream = 'stock_movement'"),
        2
    );
    // Every row's idempotency key is its own id.
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM outbox WHERE idempotency_key != id"),
        0
    );

    // Print job queued; the draft is tombstoned (in the scratch DB)
    // and gone.
    assert_eq!(fx.count("SELECT COUNT(*) FROM print_job"), 1);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM print_job WHERE status = 'queued'"),
        1
    );
    let tombstones: i64 = fx
        .scratch
        .with_conn(|conn| {
            Ok(
                conn.query_row("SELECT COUNT(*) FROM draft_tombstone", [], |row| {
                    row.get::<_, i64>(0)
                })?,
            )
        })
        .unwrap();
    assert_eq!(tombstones, 1);
    assert!(load_draft(&fx.scratch, &draft_id).unwrap().is_none());
}

#[test]
fn cash_acceptance_5232() {
    // The 001a cash acceptance, end to end: TST44 is 44.34 + 18% tax
    // (798) = 52.32; tender 60.00; change 7.68; cash rounding OFF, so
    // there is no rounding line on the sale or the receipt.
    let fx = Fixture::new("cash-5232");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    let done = fx.complete(&draft_id, 6_000).unwrap();
    assert_eq!(done.total_minor, 5_232);
    assert_eq!(done.change_minor, 768);
    assert_eq!(done.rounding_line_minor, 0);
    assert_eq!(fx.sale_totals("MAIN-T1-20261111-1"), (4_434, 0, 798, 5_232));
}

#[test]
fn insufficient_tender_rejected() {
    let fx = Fixture::new("insufficient-tender");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    let err = fx.complete(&draft_id, 5_231).unwrap_err();
    assert_eq!(err.code(), "INSUFFICIENT_TENDER");
    // Nothing was written: no sale, no stock, no audit, no outbox.
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 0);
    assert_eq!(fx.balance("TST44"), 10_000);
    assert_eq!(fx.count("SELECT COUNT(*) FROM audit_log"), 0);
    assert_eq!(fx.count("SELECT COUNT(*) FROM outbox"), 0);
    // The draft is still open (the cashier can take more tender).
    assert!(load_draft(&fx.scratch, &draft_id).unwrap().is_some());
}

#[test]
fn shift_not_open() {
    let fx = Fixture::new("shift-not-open");
    // Close the shift (the shift use cases land in a later spec; the
    // test flips the row directly).
    fx.db
        .write(|conn| {
            conn.execute(
                "UPDATE shift SET status = 'closed' WHERE id = ?1",
                rusqlite::params![SHIFT_ID],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    let err = fx.complete(&draft_id, 6_000).unwrap_err();
    assert_eq!(err.code(), "SHIFT_NOT_OPEN");
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 0);
}

#[test]
fn sale_requires_sale_create() {
    let fx = Fixture::new("sale-permission");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    let viewer = template().viewer.clone();
    let err = complete_sale_cash(
        &fx.db,
        &fx.scratch,
        CompleteSaleCashInput {
            tenant_id: TENANT.to_string(),
            branch_id: BRANCH_ID.to_string(),
            terminal_id: TERMINAL_ID.to_string(),
            branch_code: BRANCH_CODE.to_string(),
            terminal_code: TERMINAL_CODE.to_string(),
            shift_id: SHIFT_ID.to_string(),
            user_id: viewer.id,
            draft_id,
            tender_minor: 6_000,
            now_ms: NOW,
            date_yyyymmdd: DATE.to_string(),
        },
        None,
    )
    .unwrap_err();
    assert_eq!(err.code(), "PERMISSION_DENIED");
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 0);
}

#[test]
fn double_submit_returns_same_sale() {
    let fx = Fixture::new("double-submit");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000), ("BRD500", 1_000)]);
    let first = fx.complete(&draft_id, 100_000).unwrap();
    // The second call replays: same sale, same receipt, not fresh.
    // (The new call's tender is ignored — the replay recomputes the
    // change from the STORED payment.)
    let second = fx.complete(&draft_id, 1).unwrap();
    assert_eq!(second.sale_id, first.sale_id);
    assert_eq!(second.receipt_no, first.receipt_no);
    assert_eq!(second.total_minor, first.total_minor);
    assert_eq!(second.change_minor, first.change_minor);
    assert!(!second.is_fresh);
    // One sale row, one stock posting per line, one audit row.
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 1);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM stock_movement WHERE type = 'sale'"),
        2
    );
    assert_eq!(fx.count("SELECT COUNT(*) FROM audit_log"), 1);
    assert_eq!(fx.balance("MILK1L"), 99_000);
}

#[test]
fn restore_discards_completed_draft() {
    let fx = Fixture::new("restore-discards");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    fx.complete(&draft_id, 6_000).unwrap();
    // After completion there is nothing to restore.
    assert!(
        restore_draft(&fx.db, &fx.scratch, TENANT, TERMINAL_ID)
            .unwrap()
            .is_none()
    );
    // And opening a draft now starts a fresh empty one.
    let fresh = open_draft(
        &fx.scratch,
        TENANT,
        BRANCH_ID,
        TERMINAL_ID,
        Some(SHIFT_ID),
        Some(&template().cashier.id),
        NOW,
    )
    .unwrap();
    assert_ne!(fresh.id, draft_id);
    assert_eq!(fresh.lines.len(), 0);
}

#[test]
fn late_flush_does_not_resurrect() {
    let fx = Fixture::new("late-flush");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    fx.complete(&draft_id, 6_000).unwrap();
    // Simulate a late scratch-DB flush arriving after the sale: the
    // completed draft's rows are written back into the scratch DB.
    let draft_id_clone = draft_id.clone();
    fx.scratch
        .with_conn(|conn| {
            conn.execute(
                "INSERT INTO draft_sale (id, tenant_id, branch_id, terminal_id, shift_id, user_id, status, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?7, ?7, 1)",
                rusqlite::params![draft_id_clone, TENANT, BRANCH_ID, TERMINAL_ID, SHIFT_ID, template().cashier.id, NOW],
            )
            .unwrap();
            conn.execute(
                "INSERT INTO draft_line (id, tenant_id, draft_sale_id, sku_id, qty_milli, unit_price_e4, discount_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 1000, 443400, 0, ?5, ?5, 1)",
                rusqlite::params![format!("late-{draft_id_clone}"), TENANT, draft_id_clone, sku_id("TST44"), NOW],
            )
            .unwrap();
            Ok(())
        })
        .unwrap();
    // Restore sees the tombstone and discards the resurrected draft.
    assert!(
        restore_draft(&fx.db, &fx.scratch, TENANT, TERMINAL_ID)
            .unwrap()
            .is_none()
    );
    assert!(load_draft(&fx.scratch, &draft_id).unwrap().is_none());
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 1);
}

#[test]
fn fault_injection_rolls_back_at_each_step() {
    for step in [
        STEP_IDEMPOTENCY,
        STEP_SHIFT,
        STEP_PRICING,
        STEP_RECEIPT_COUNTER,
        STEP_SALE,
        STEP_LINES,
        STEP_TAX_ROWS,
        STEP_PAYMENT,
        STEP_STOCK,
        STEP_AGGREGATES,
        STEP_AUDIT,
        STEP_OUTBOX,
        STEP_PRINT_JOB,
        STEP_COMMIT,
    ] {
        let fx = Fixture::new(&format!("fault-{step}"));
        let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
        let injector = FaultInjector::fail_at(step);
        let err = complete_sale_cash(
            &fx.db,
            &fx.scratch,
            CompleteSaleCashInput {
                tenant_id: TENANT.to_string(),
                branch_id: BRANCH_ID.to_string(),
                terminal_id: TERMINAL_ID.to_string(),
                branch_code: BRANCH_CODE.to_string(),
                terminal_code: TERMINAL_CODE.to_string(),
                shift_id: SHIFT_ID.to_string(),
                user_id: template().cashier.id.clone(),
                draft_id: draft_id.clone(),
                tender_minor: 100_000,
                now_ms: NOW,
                date_yyyymmdd: DATE.to_string(),
            },
            Some(&injector),
        )
        .unwrap_err();
        assert_eq!(err.code(), "FAULT_INJECTED", "step {step}");
        // The transaction rolled back: nothing was written anywhere.
        assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 0, "step {step}");
        assert_eq!(fx.count("SELECT COUNT(*) FROM sale_line"), 0, "step {step}");
        assert_eq!(
            fx.count("SELECT COUNT(*) FROM sale_line_tax"),
            0,
            "step {step}"
        );
        assert_eq!(
            fx.count("SELECT COUNT(*) FROM sale_payment"),
            0,
            "step {step}"
        );
        assert_eq!(
            fx.count("SELECT COUNT(*) FROM stock_movement WHERE type = 'sale'"),
            0,
            "step {step}"
        );
        assert_eq!(fx.balance("MILK1L"), 100_000, "step {step}");
        assert_eq!(fx.count("SELECT COUNT(*) FROM audit_log"), 0, "step {step}");
        assert_eq!(fx.count("SELECT COUNT(*) FROM outbox"), 0, "step {step}");
        assert_eq!(fx.count("SELECT COUNT(*) FROM print_job"), 0, "step {step}");
        assert_eq!(
            fx.count("SELECT COUNT(*) FROM receipt_counter"),
            0,
            "step {step}"
        );
        // The draft survives the failed commit (not tombstoned).
        assert!(
            load_draft(&fx.scratch, &draft_id).unwrap().is_some(),
            "step {step}"
        );
    }
}

#[test]
fn stock_balance_equals_ledger_after_sequence() {
    let fx = Fixture::new("stock-ledger");
    // Three sales, then a refund-free sequence: balances must equal the
    // ledger sum for every SKU after each one.
    for (lines, tender) in [
        (vec![("MILK1L", 1_000), ("TEA200", 2_000)], 100_000),
        (vec![("MILK1L", 3_000)], 100_000),
        (vec![("BRD500", 5_000), ("EGG12", 1_000)], 100_000),
    ] {
        let draft_id = fx.draft_with(&lines);
        fx.complete(&draft_id, tender).unwrap();
        for code in [
            "MILK1L", "BRD500", "SGR1KG", "RCE5KG", "TEA200", "EGG12", "TST44",
        ] {
            assert_eq!(fx.balance(code), fx.ledger_sum(code), "after sale, {code}");
        }
    }
    assert_eq!(fx.balance("MILK1L"), 96_000);
    assert_eq!(fx.balance("TEA200"), 118_000);
    assert_eq!(fx.balance("BRD500"), 75_000);
    assert_eq!(fx.balance("EGG12"), 59_000);
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 3);
}

#[test]
fn negative_stock_posts_and_is_flagged() {
    let fx = Fixture::new("negative-stock");
    // Sell 150 units of MILK1L against 100 in stock: the sale posts
    // (negative stock is allowed) and the balance goes negative.
    let draft_id = fx.draft_with(&[("MILK1L", 150_000)]);
    let done = fx.complete(&draft_id, 5_000_000).unwrap();
    // 150 units x 25,000 minor = 3,750,000 gross; 18% tax = 675,000.
    assert_eq!(done.total_minor, 4_425_000);
    assert_eq!(fx.balance("MILK1L"), -50_000);
    assert_eq!(fx.ledger_sum("MILK1L"), -50_000);
    // "Flagged" = a `balance < 0` query (no new table, no new column).
    let flagged = negative_stock_skus(&fx.db, TENANT, BRANCH_ID).unwrap();
    assert_eq!(flagged.len(), 1);
    assert_eq!(flagged[0].0, sku_id("MILK1L"));
    assert_eq!(flagged[0].1, -50_000);
}

#[test]
fn tax_rows_sum_to_line_tax() {
    let fx = Fixture::new("tax-rows");
    // STD line (18%) + ZERO line (no tax rows at all).
    let draft_id = fx.draft_with(&[("MILK1L", 1_000), ("BRD500", 2_000)]);
    fx.complete(&draft_id, 100_000).unwrap();
    // MILK1L: 25,000 gross, 4,500 tax, one row at 1800 bp.
    let milk_tax: (i64, i64, i64) = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT slt.tax_minor, slt.rate_bp, sl.tax_minor FROM sale_line sl JOIN sale_line_tax slt ON slt.sale_line_id = sl.id WHERE sl.sku_id = ?1",
                rusqlite::params![sku_id("MILK1L")],
                |row| {
                    Ok((
                        row.get::<_, i64>(0).unwrap(),
                        row.get::<_, i64>(1).unwrap(),
                        row.get::<_, i64>(2).unwrap(),
                    ))
                },
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert_eq!(milk_tax, (4_500, 1800, 4_500));
    // BRD500 (ZERO): no `sale_line_tax` rows, line tax 0.
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM sale_line_tax slt JOIN sale_line sl ON slt.sale_line_id = sl.id WHERE sl.sku_id = 'sku-brd500'"),
        0
    );
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM sale_line WHERE sku_id = 'sku-brd500' AND tax_minor = 0"),
        1
    );
}

#[test]
fn aggregates_update_in_same_transaction() {
    let fx = Fixture::new("aggregates");
    let d1 = fx.draft_with(&[("TST44", 1_000)]);
    fx.complete(&d1, 6_000).unwrap();
    let d2 = fx.draft_with(&[("TST44", 2_000)]);
    fx.complete(&d2, 20_000).unwrap();
    // Two sales of TST44 (5,232 + 10,464 = 15,696).
    let agg: (i64, i64, i64, i64, i64) = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT sale_count, subtotal_minor, discount_minor, tax_minor, total_minor FROM daily_sales_summary WHERE date = '20261111'",
                [],
                |row| {
                    Ok((
                        row.get::<_, i64>(0).unwrap(),
                        row.get::<_, i64>(1).unwrap(),
                        row.get::<_, i64>(2).unwrap(),
                        row.get::<_, i64>(3).unwrap(),
                        row.get::<_, i64>(4).unwrap(),
                    ))
                },
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert_eq!(agg, (2, 13_302, 0, 2_394, 15_696));
    let item: (i64, i64) = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT qty_milli, total_minor FROM daily_item_sales WHERE sku_id = 'sku-tst44' AND date = '20261111'",
                [],
                |row| Ok((row.get::<_, i64>(0).unwrap(), row.get::<_, i64>(1).unwrap())),
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert_eq!(item, (3_000, 15_696));
    let cash: (i64, i64) = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT cash_sales_minor, expected_minor FROM daily_cash_summary WHERE date = '20261111'",
                [],
                |row| Ok((row.get::<_, i64>(0).unwrap(), row.get::<_, i64>(1).unwrap())),
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert_eq!(cash, (15_696, 15_696));
}

#[test]
fn outbox_rows_in_same_transaction() {
    let fx = Fixture::new("outbox");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000), ("TEA200", 1_000)]);
    let done = fx.complete(&draft_id, 100_000).unwrap();
    // One sale row (payload names the sale) + one stock row per SKU.
    let sale_payload: String = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT payload FROM outbox WHERE entity = 'sale'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert!(sale_payload.contains(&done.sale_id));
    assert!(sale_payload.contains("MAIN-T1-20261111-1"));
    let stock_payloads: Vec<String> = fx
        .db
        .read(|conn| {
            let mut stmt = conn
                .prepare(
                    "SELECT payload FROM outbox WHERE entity = 'stock_movement' ORDER BY payload",
                )
                .unwrap();
            let rows = stmt.query_map([], |row| row.get::<_, String>(0)).unwrap();
            let mut out = Vec::new();
            for row in rows {
                out.push(row.unwrap());
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(stock_payloads.len(), 2);
    assert!(
        stock_payloads[0].contains(sku_id("MILK1L"))
            || stock_payloads[1].contains(sku_id("MILK1L")),
        "a stock row names MILK1L"
    );
    assert!(
        stock_payloads[0].contains(sku_id("TEA200"))
            || stock_payloads[1].contains(sku_id("TEA200")),
        "a stock row names TEA200"
    );
}

// ---------------------------------------------------------------------------
// Tax mode from the data (owner decision 2026-11-11)
// ---------------------------------------------------------------------------

#[test]
fn exclusive_sale_uses_the_groups_flag() {
    // 001a golden: net 100.00 @18% exclusive → tax 18.00, total 118.00.
    let fx = Fixture::new("tax-exclusive");
    let draft_id = fx.draft_with(&[("TST100", 1_000)]);
    let done = fx.complete(&draft_id, 11_800).unwrap();
    assert_eq!(done.total_minor, 11_800);
    assert_eq!(done.change_minor, 0);
    assert_eq!(
        fx.sale_totals(&done.receipt_no),
        (10_000, 0, 1_800, 11_800),
        "subtotal is the net for an exclusive line"
    );
    assert_eq!(fx.tax_rows(), vec![(1_800, 1_800)]);
}

#[test]
fn inclusive_sale_uses_the_groups_flag() {
    // 001a golden: gross 118.00 @18% inclusive → net 100.00, tax 18.00.
    let fx = Fixture::new("tax-inclusive");
    let draft_id = fx.draft_with(&[("TST118", 1_000)]);
    let done = fx.complete(&draft_id, 11_800).unwrap();
    assert_eq!(done.total_minor, 11_800);
    assert_eq!(done.change_minor, 0);
    // The domain's subtotal is the sum of line GROSSES, so for an
    // inclusive line it is the shelf price (11800), not the net.
    assert_eq!(
        fx.sale_totals(&done.receipt_no),
        (11_800, 0, 1_800, 11_800),
        "subtotal is the gross for an inclusive line"
    );
    assert_eq!(fx.tax_rows(), vec![(1_800, 1_800)]);
}

#[test]
fn mixed_mode_sale_uses_each_lines_own_flag() {
    // One exclusive line (TST100: 100.00 + 18.00 tax) and one inclusive
    // line (TST118: 118.00 incl. 18.00 tax) in the same sale — no
    // sale-level mode, each line uses its own group's flag.
    let fx = Fixture::new("tax-mixed");
    let draft_id = fx.draft_with(&[("TST100", 1_000), ("TST118", 1_000)]);
    let done = fx.complete(&draft_id, 23_600).unwrap();
    assert_eq!(done.total_minor, 23_600);
    assert_eq!(done.change_minor, 0);
    assert_eq!(
        fx.sale_totals(&done.receipt_no),
        (21_800, 0, 3_600, 23_600),
        "subtotal 10000 + 11800, tax 1800 + 1800"
    );
    assert_eq!(fx.tax_rows(), vec![(1_800, 1_800), (1_800, 1_800)]);
}

// ---------------------------------------------------------------------------
// Net cash (owner fix 2026-11-11)
// ---------------------------------------------------------------------------

#[test]
fn daily_cash_summary_uses_net_cash_not_tender() {
    let fx = Fixture::new("net-cash");
    // Sale 1: total 52.32, tender 60.00 → net cash 52.32.
    let draft1 = fx.draft_with(&[("TST44", 1_000)]);
    fx.complete(&draft1, 6_000).unwrap();
    // Sale 2: total 10.00, tender 10.00 → net cash 10.00.
    let draft2 = fx.draft_with(&[("TST010", 1_000)]);
    fx.complete(&draft2, 1_000).unwrap();
    // Net cash: 5232 + 1000 = 6232 — NOT the tender total (7000).
    // (The sale code already accumulates the total, not the tender;
    // this test pins that behaviour.)
    assert_eq!(fx.daily_cash(), (6_232, 6_232));
}

// ---------------------------------------------------------------------------
// Replay indicator (owner fix 2026-11-11)
// ---------------------------------------------------------------------------

#[test]
fn replay_with_a_different_tender_returns_the_original_sale() {
    let fx = Fixture::new("replay-tender");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    let first = fx.complete(&draft_id, 6_000).unwrap();
    assert!(
        !first.already_completed(),
        "the first call commits a new sale"
    );
    assert_eq!(first.total_minor, 5_232);
    assert_eq!(first.change_minor, 768);
    // Second call, same draft, a DIFFERENT tender: the original sale
    // comes back unchanged, flagged as already completed.
    let second = fx.complete(&draft_id, 10_000).unwrap();
    assert!(
        second.already_completed(),
        "the second call is a replay, not a new sale"
    );
    assert_eq!(second.sale_id, first.sale_id);
    assert_eq!(second.receipt_no, first.receipt_no);
    assert_eq!(second.total_minor, first.total_minor);
    assert_eq!(
        second.change_minor, 768,
        "change is recomputed from the ORIGINAL tender (6000), not the new one (10000)"
    );
    assert_eq!(second.change_minor, first.change_minor);
    // Still exactly one sale row and one sale stock posting.
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 1);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM stock_movement WHERE type = 'sale'"),
        1
    );
}

// ---------------------------------------------------------------------------
// Draft cart
// ---------------------------------------------------------------------------

#[test]
fn draft_persists_and_restores_after_restart() {
    let info = template();
    let dir = unique_test_dir(&test_root_001b2(), "draft-restart");
    std::fs::copy(info.dir.join("till.db"), dir.join("till.db")).unwrap();
    let main_path = dir.join("till.db");
    let scratch_path = dir.join("scratch.db");
    let draft_id;
    {
        // Only the scratch DB is opened here: the draft lives there,
        // the main DB file is only copied (not opened) in this scope.
        let scratch = ScratchDb::open(&scratch_path).unwrap();
        let cart = open_draft(
            &scratch,
            TENANT,
            BRANCH_ID,
            TERMINAL_ID,
            Some(SHIFT_ID),
            Some(&info.cashier.id),
            NOW,
        )
        .unwrap();
        draft_add_line(
            &scratch,
            &cart.id,
            sku_id("MILK1L"),
            2_000,
            sku_price_e4("MILK1L"),
            NOW,
        )
        .unwrap();
        draft_add_line(
            &scratch,
            &cart.id,
            sku_id("TEA200"),
            1_000,
            sku_price_e4("TEA200"),
            NOW,
        )
        .unwrap();
        draft_id = cart.id;
        // Both DBs drop here (files closed, WAL checkpointed).
    }
    // Reopen from the same files: the draft is restored, lines intact.
    let db = Db::open(&main_path, Some(TEST_KEY)).unwrap();
    let scratch = ScratchDb::open(&scratch_path).unwrap();
    let restored = restore_draft(&db, &scratch, TENANT, TERMINAL_ID)
        .unwrap()
        .unwrap();
    assert_eq!(restored.id, draft_id);
    assert_eq!(restored.lines.len(), 2);
    assert_eq!(restored.lines[0].sku_id, sku_id("MILK1L"));
    assert_eq!(restored.lines[0].qty_milli, 2_000);
    assert_eq!(restored.lines[1].sku_id, sku_id("TEA200"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn scan_path_never_touches_main_db() {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    let fx = Fixture::new("scan-path");
    // Settle: read from the main DB, then hash both files.
    assert_eq!(fx.count("SELECT COUNT(*) FROM sale"), 0);
    let hash_file = |path: &PathBuf| -> u64 {
        let mut h = DefaultHasher::new();
        if let Ok(bytes) = std::fs::read(path) {
            bytes.hash(&mut h);
        }
        h.finish()
    };
    let main_before = hash_file(&fx.dir.join("till.db"));
    let scratch_before =
        hash_file(&fx.dir.join("scratch.db")) ^ hash_file(&fx.dir.join("scratch.db-wal"));
    // Draft operations only (scan path): add, change qty, remove.
    let cart = open_draft(
        &fx.scratch,
        TENANT,
        BRANCH_ID,
        TERMINAL_ID,
        Some(SHIFT_ID),
        Some(&template().cashier.id),
        NOW,
    )
    .unwrap();
    let line = draft_add_line(
        &fx.scratch,
        &cart.id,
        sku_id("MILK1L"),
        1_000,
        sku_price_e4("MILK1L"),
        NOW,
    )
    .unwrap();
    draft_set_qty(&fx.scratch, &cart.id, &line.id, 3_000, NOW).unwrap();
    draft_remove_line(&fx.scratch, &cart.id, &line.id, NOW).unwrap();
    // The main DB file is byte-identical (no draft flush ever touched
    // it, and no FULL flush ran on the scan path) ...
    assert_eq!(hash_file(&fx.dir.join("till.db")), main_before);
    // ... while the scratch DB file changed (the draft lives there).
    let scratch_after =
        hash_file(&fx.dir.join("scratch.db")) ^ hash_file(&fx.dir.join("scratch.db-wal"));
    assert_ne!(scratch_after, scratch_before);
    // And the main DB's own draft tables are still empty (the draft
    // rows exist only in the scratch DB).
    assert_eq!(fx.count("SELECT COUNT(*) FROM draft_sale"), 0);
    assert_eq!(fx.count("SELECT COUNT(*) FROM draft_line"), 0);
    assert!(load_draft(&fx.scratch, &cart.id).unwrap().is_some());
}

#[test]
fn draft_cancel_writes_audit_when_cart_had_lines() {
    let fx = Fixture::new("draft-cancel");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
    let cancelled =
        draft_cancel(&fx.db, &fx.scratch, &draft_id, &template().cashier.id, NOW).unwrap();
    assert!(cancelled);
    assert!(load_draft(&fx.scratch, &draft_id).unwrap().is_none());
    // The cancel was audited (cart contents summarised in the row).
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'cart.cancelled'"),
        1
    );
    assert_eq!(verify_audit_chain(&fx.db, TENANT, TERMINAL_ID).unwrap(), 1);

    // Cancelling an EMPTY draft writes no audit row.
    let empty_id = fx.draft_with(&[]);
    let cancelled =
        draft_cancel(&fx.db, &fx.scratch, &empty_id, &template().cashier.id, NOW).unwrap();
    assert!(cancelled);
    assert_eq!(fx.count("SELECT COUNT(*) FROM audit_log"), 1);
}

#[test]
fn draft_restore_line_once() {
    let fx = Fixture::new("draft-restore-line");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000), ("TEA200", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let tea = cart
        .lines
        .iter()
        .find(|l| l.sku_id == sku_id("TEA200"))
        .unwrap()
        .clone();
    assert!(draft_remove_line(&fx.scratch, &draft_id, &tea.id, NOW).unwrap());
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 1);
    // First restore brings the line back.
    assert!(draft_restore_line(&fx.scratch, &draft_id, NOW).unwrap());
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 2);
    // Second restore is a no-op (single-level undo only).
    assert!(!draft_restore_line(&fx.scratch, &draft_id, NOW).unwrap());
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 2);
}

#[test]
fn draft_set_qty_zero_removes_and_stashes() {
    let fx = Fixture::new("draft-set-qty-zero");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000), ("TEA200", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let milk = cart
        .lines
        .iter()
        .find(|l| l.sku_id == sku_id("MILK1L"))
        .unwrap()
        .clone();
    // Qty 0 removes the line (and stashes it for undo).
    assert!(
        draft_set_qty(&fx.scratch, &draft_id, &milk.id, 0, NOW)
            .unwrap()
            .is_none()
    );
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 1);
    assert!(draft_restore_line(&fx.scratch, &draft_id, NOW).unwrap());
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 2);
}

#[test]
fn draft_add_same_sku_same_price_increments() {
    let fx = Fixture::new("draft-add-same");
    let cart = open_draft(
        &fx.scratch,
        TENANT,
        BRANCH_ID,
        TERMINAL_ID,
        Some(SHIFT_ID),
        Some(&template().cashier.id),
        NOW,
    )
    .unwrap();
    draft_add_line(
        &fx.scratch,
        &cart.id,
        sku_id("MILK1L"),
        1_000,
        sku_price_e4("MILK1L"),
        NOW,
    )
    .unwrap();
    let line = draft_add_line(
        &fx.scratch,
        &cart.id,
        sku_id("MILK1L"),
        2_000,
        sku_price_e4("MILK1L"),
        NOW,
    )
    .unwrap();
    // Same SKU + same price: one line, quantity incremented.
    assert_eq!(line.qty_milli, 3_000);
    let cart = load_draft(&fx.scratch, &cart.id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 1);
}

#[test]
fn draft_add_same_sku_different_price_separate_lines() {
    let fx = Fixture::new("draft-add-diff-price");
    let cart = open_draft(
        &fx.scratch,
        TENANT,
        BRANCH_ID,
        TERMINAL_ID,
        Some(SHIFT_ID),
        Some(&template().cashier.id),
        NOW,
    )
    .unwrap();
    draft_add_line(
        &fx.scratch,
        &cart.id,
        sku_id("MILK1L"),
        1_000,
        sku_price_e4("MILK1L"),
        NOW,
    )
    .unwrap();
    // A different price (e.g. a price override) is a separate line.
    draft_add_line(
        &fx.scratch,
        &cart.id,
        sku_id("MILK1L"),
        1_000,
        2_000_000,
        NOW,
    )
    .unwrap();
    let cart = load_draft(&fx.scratch, &cart.id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 2);
}

#[test]
fn discount_above_threshold_needs_pin() {
    let fx = Fixture::new("discount-needs-pin");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let line = cart.lines[0].clone();
    let cashier = get_user_by_username(&fx.db, TENANT, "cashier")
        .unwrap()
        .unwrap();
    // 10% of 25,000 = 2,500 — above the cashier's 5% ceiling.
    let ten_pct = LineDiscount::Percent(shopcore_domain::Rate::from_basis_points(1000).unwrap());
    let err = draft_apply_discount(
        &fx.db,
        &fx.scratch,
        &draft_id,
        &line.id,
        ten_pct,
        &cashier,
        None,
        NOW,
    )
    .unwrap_err();
    assert_eq!(err.code(), "DISCOUNT_NEEDS_APPROVAL");
    // Rejected + audited; the line discount is still 0.
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'discount.rejected'"),
        1
    );
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines[0].discount_minor, 0);
    // With the manager PIN it approves, and the approval is audited
    // with BOTH users.
    let applied = draft_apply_discount(
        &fx.db,
        &fx.scratch,
        &draft_id,
        &line.id,
        ten_pct,
        &cashier,
        Some(("manager", "4321")),
        NOW,
    )
    .unwrap();
    assert_eq!(applied, 2_500);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines[0].discount_minor, 2_500);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'discount.approved'"),
        1
    );
    let approved_row: (String, String) = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT user_id, after_json FROM audit_log WHERE action = 'discount.approved'",
                [],
                |row| {
                    Ok((
                        row.get::<_, String>(0).unwrap(),
                        row.get::<_, String>(1).unwrap(),
                    ))
                },
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    assert_eq!(approved_row.0, cashier.id);
    assert!(approved_row.1.contains("manager"));
    assert_eq!(verify_audit_chain(&fx.db, TENANT, TERMINAL_ID).unwrap(), 2);
}

#[test]
fn discount_within_threshold_applies() {
    let fx = Fixture::new("discount-within");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let line = cart.lines[0].clone();
    let cashier = get_user_by_username(&fx.db, TENANT, "cashier")
        .unwrap()
        .unwrap();
    // 5% of 25,000 = 1,250 — exactly the cashier's ceiling, so no
    // manager PIN is needed.
    let five_pct = LineDiscount::Percent(shopcore_domain::Rate::from_basis_points(500).unwrap());
    let applied = draft_apply_discount(
        &fx.db,
        &fx.scratch,
        &draft_id,
        &line.id,
        five_pct,
        &cashier,
        None,
        NOW,
    )
    .unwrap();
    assert_eq!(applied, 1_250);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines[0].discount_minor, 1_250);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'discount.applied'"),
        1
    );
}

#[test]
fn discount_wrong_pin_rejected() {
    let fx = Fixture::new("discount-wrong-pin");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let line = cart.lines[0].clone();
    let cashier = get_user_by_username(&fx.db, TENANT, "cashier")
        .unwrap()
        .unwrap();
    let ten_pct = LineDiscount::Percent(shopcore_domain::Rate::from_basis_points(1000).unwrap());
    let err = draft_apply_discount(
        &fx.db,
        &fx.scratch,
        &draft_id,
        &line.id,
        ten_pct,
        &cashier,
        Some(("manager", "9999")),
        NOW,
    )
    .unwrap_err();
    assert_eq!(err.code(), "DISCOUNT_NEEDS_APPROVAL");
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'discount.rejected'"),
        1
    );
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    assert_eq!(cart.lines[0].discount_minor, 0);
}

#[test]
fn discount_self_approval_rejected() {
    let fx = Fixture::new("discount-self-approval");
    let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let line = cart.lines[0].clone();
    let cashier = get_user_by_username(&fx.db, TENANT, "cashier")
        .unwrap()
        .unwrap();
    // The cashier cannot approve their own over-threshold discount
    // (07: approver must differ from the cashier).
    let ten_pct = LineDiscount::Percent(shopcore_domain::Rate::from_basis_points(1000).unwrap());
    let err = draft_apply_discount(
        &fx.db,
        &fx.scratch,
        &draft_id,
        &line.id,
        ten_pct,
        &cashier,
        Some(("cashier", "1234")),
        NOW,
    )
    .unwrap_err();
    assert_eq!(err.code(), "DISCOUNT_NEEDS_APPROVAL");
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'discount.rejected'"),
        1
    );
}

// ---------------------------------------------------------------------------
// Audit hash chain
// ---------------------------------------------------------------------------

#[test]
fn tampered_audit_row_caught_by_verify() {
    let fx = Fixture::new("audit-tamper");
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    fx.complete(&draft_id, 6_000).unwrap();
    // The id of the sale's audit row.
    let row_id: String = fx
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT id FROM audit_log WHERE action = 'sale.completed'",
                [],
                |row| row.get::<_, String>(0),
            )
            .map_err(shopcore_db::DbError::from)
        })
        .unwrap();
    // Simulate an attacker with direct DB access: drop the append-only
    // trigger, change one field, re-create the trigger.
    fx.db
        .write({
            let row_id = row_id.clone();
            move |conn| {
            let tx = conn.transaction().unwrap();
            tx.execute("DROP TRIGGER IF EXISTS trg_audit_log_no_update", [])
                .unwrap();
            tx.execute(
                "UPDATE audit_log SET after_json = '{\"tampered\":true}' WHERE id = ?1",
                rusqlite::params![row_id],
            )
            .unwrap();
            tx.execute(
                "CREATE TRIGGER IF NOT EXISTS trg_audit_log_no_update BEFORE UPDATE ON audit_log BEGIN SELECT RAISE(ABORT, 'audit_log is append-only'); END",
                [],
            )
            .unwrap();
            tx.commit().unwrap();
            Ok(())
        }})
        .unwrap();
    // Verify reports the FIRST broken link, with the tampered row id —
    // never silently repaired.
    let err = verify_audit_chain(&fx.db, TENANT, TERMINAL_ID).unwrap_err();
    match err {
        shopcore_core::CoreError::AuditChain { row_id: broken, .. } => {
            assert_eq!(broken, row_id);
        }
        other => panic!("expected AuditChain, got {other:?}"),
    }
    // And the trigger is back: a normal UPDATE is refused again.
    let refused = fx.db.write(move |conn| {
        conn.execute(
            "UPDATE audit_log SET after_json = '{}' WHERE id = ?1",
            rusqlite::params![row_id],
        )
        .map_err(shopcore_db::DbError::from)?;
        Ok(())
    });
    assert!(refused.is_err());
}

#[test]
fn verify_reports_row_count() {
    let fx = Fixture::new("audit-count");
    // Three audited events, in sequence on one terminal (a terminal
    // holds ONE open draft at a time — `open_draft` returns the
    // existing one): a discount, a sale, then a cancel.
    let draft_id = fx.draft_with(&[("MILK1L", 1_000)]);
    let cart = load_draft(&fx.scratch, &draft_id).unwrap().unwrap();
    let line = cart.lines[0].clone();
    let cashier = get_user_by_username(&fx.db, TENANT, "cashier")
        .unwrap()
        .unwrap();
    let five_pct = LineDiscount::Percent(shopcore_domain::Rate::from_basis_points(500).unwrap());
    draft_apply_discount(
        &fx.db,
        &fx.scratch,
        &draft_id,
        &line.id,
        five_pct,
        &cashier,
        None,
        NOW,
    )
    .unwrap();
    fx.complete(&draft_id, 100_000).unwrap();
    let draft2 = fx.draft_with(&[("TEA200", 1_000)]);
    draft_cancel(&fx.db, &fx.scratch, &draft2, &cashier.id, NOW).unwrap();
    assert_eq!(verify_audit_chain(&fx.db, TENANT, TERMINAL_ID).unwrap(), 3);
}

// ---------------------------------------------------------------------------
// Hardware
// ---------------------------------------------------------------------------

#[tokio::test]
async fn printer_offline_sale_completes_and_reprint_queued() {
    let fx = Fixture::new("printer-offline");
    let mut printer = MockPrinter::new();
    printer.set_offline(true);
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    let done = fx.complete(&draft_id, 6_000).unwrap();
    // The sale completed even though the printer is offline.
    assert!(done.receipt_queued);
    assert_eq!(printer.printed_docs().len(), 0);
    // A print attempt while offline leaves the job queued.
    let status = attempt_print(&fx.db, &done.print_job_id, &mut printer)
        .await
        .unwrap();
    assert_eq!(status, PrintStatus::Queued);
    assert_eq!(status.code(), "PRINT_QUEUED");
    assert_eq!(printer.printed_docs().len(), 0);
    // Printer back online: the queued receipt prints.
    printer.set_offline(false);
    let status = attempt_print(&fx.db, &done.print_job_id, &mut printer)
        .await
        .unwrap();
    assert_eq!(status, PrintStatus::Printed);
    assert_eq!(printer.printed_docs().len(), 1);
    assert_eq!(printer.printed_docs()[0].total_minor, 5_232);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM print_job WHERE status = 'printed'"),
        1
    );
    // A reprint is an audited event.
    let status = reprint_receipt(
        &fx.db,
        &done.sale_id,
        &template().cashier.id,
        TERMINAL_ID,
        &mut printer,
        NOW,
    )
    .await
    .unwrap();
    assert_eq!(status, PrintStatus::Printed);
    assert_eq!(printer.printed_docs().len(), 2);
    assert_eq!(
        fx.count("SELECT COUNT(*) FROM audit_log WHERE action = 'receipt.reprint'"),
        1
    );
}

#[tokio::test]
async fn scanner_feeds_scans_to_draft() {
    let fx = Fixture::new("scanner");
    let mut scanner = MockScanner::new();
    let mut rx = scanner.take_receiver().unwrap();
    scanner.feed_scan(sku_barcode("MILK1L")).unwrap();
    scanner.feed_scan(sku_barcode("TEA200")).unwrap();
    scanner.feed_scan(sku_barcode("TEA200")).unwrap();
    let cart = open_draft(
        &fx.scratch,
        TENANT,
        BRANCH_ID,
        TERMINAL_ID,
        Some(SHIFT_ID),
        Some(&template().cashier.id),
        NOW,
    )
    .unwrap();
    // Drain the channel: each scan is looked up and added to the draft.
    for _ in 0..3 {
        let event = rx.recv().await.unwrap();
        let ScanEvent::Barcode { code } = event else {
            panic!("expected a barcode scan");
        };
        let hit = lookup_barcode(&fx.db, TENANT, &code, NOW).unwrap();
        draft_add_line(
            &fx.scratch,
            &cart.id,
            &hit.sku_id,
            1_000,
            hit.price_e4.unwrap(),
            NOW,
        )
        .unwrap();
    }
    let cart = load_draft(&fx.scratch, &cart.id).unwrap().unwrap();
    assert_eq!(cart.lines.len(), 2);
    let milk = cart
        .lines
        .iter()
        .find(|l| l.sku_id == sku_id("MILK1L"))
        .unwrap();
    assert_eq!(milk.qty_milli, 1_000);
    let tea = cart
        .lines
        .iter()
        .find(|l| l.sku_id == sku_id("TEA200"))
        .unwrap();
    assert_eq!(tea.qty_milli, 2_000);
}

#[tokio::test]
async fn drawer_opens_once_per_sale() {
    let fx = Fixture::new("drawer");
    let mut drawer = MockDrawer::new();
    let draft_id = fx.draft_with(&[("TST44", 1_000)]);
    fx.complete(&draft_id, 6_000).unwrap();
    kick_drawer(&mut drawer).await.unwrap();
    assert_eq!(drawer.open_count(), 1);
}
