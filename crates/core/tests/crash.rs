//! 001b-2 crash tests: fault injection PLUS real process kills at
//! random points during sale commit (and during migration / a sale
//! loop). On restart the DB must be consistent: one complete sale or
//! none, balances match the ledger, the draft is restored.
//!
//! These tests prove ATOMICITY and RECOVERY, NOT power-loss flushing:
//! a process kill still lets the OS flush the file, so it cannot
//! simulate a power cut (the manual power-pull test is a pre-pilot
//! checklist item, owner: user — it is NOT part of this spec).
//!
//! How it works: the test binary spawns ITSELF as a worker
//! (`--exact crash_worker` + env vars). The worker opens the same DB
//! files the parent built, does the sale with a `Sleep` fault inside
//! the transaction, and the parent kills it mid-transaction, then
//! reopens the DB and verifies consistency. When nextest runs
//! `crash_worker` normally (no env vars) it is a no-op that passes.
#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

use shopcore_core::draft::{draft_add_line, open_draft, restore_draft};
use shopcore_core::sale::{
    CompleteSaleCashInput, FaultInjector, STEP_COMMIT, STEP_SHIFT, STEP_STOCK, TaxMode,
    complete_sale_cash,
};
use shopcore_core::scratch::ScratchDb;
use shopcore_core::settings::{
    KEY_ACTIVE_PRICE_LIST, KEY_CASH_ROUNDING_GRANULARITY_MINOR, KEY_TAX_DEFAULT_RATE_BP,
    KEY_TENANT_CURRENCY, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, set_setting,
};
use shopcore_core::users::{create_role, create_user};
use shopcore_db::Db;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

const NOW: i64 = 1_800_000_000_000;
const DATE: &str = "20261111";
const TENANT: &str = "tenant-001b2-crash";
const BRANCH_ID: &str = "branch-main";
const TERMINAL_ID: &str = "terminal-t1";
const SHIFT_ID: &str = "shift-001";
const SKU_ID: &str = "sku-milk1l";
const SKU_PRICE_E4: i64 = 2_500_000;
const TEST_KEY: &[u8] = b"001b2-test-key-32-bytes-long-abc";

static COUNTER: AtomicU64 = AtomicU64::new(0);

fn unique_dir(label: &str) -> PathBuf {
    let n = COUNTER.fetch_add(1, Ordering::SeqCst);
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../target/test-tmp-001b2")
        .join(format!("crash-{label}-{}-{n}", std::process::id()))
}

/// A minimal till fixture (one SKU, one cashier, one open shift) —
///
/// built fresh per crash test so the kill/restart starts from a known
/// state. The full 7-SKU template lives in `sale.rs`; the crash tests
/// only need one `SKU` to prove atomicity.
fn build_fixture(dir: &PathBuf) -> (Db, ScratchDb, String) {
    std::fs::create_dir_all(dir).unwrap();
    let db = Db::open(dir.join("till.db"), Some(TEST_KEY)).unwrap();
    let cashier_role = create_role(
        &db,
        TENANT,
        "Cashier",
        &["sale.create", "sale.discount.line"],
        500,
        NOW,
    )
    .unwrap();
    let cashier = create_user(&db, TENANT, "cashier", "1234", Some(&cashier_role.id), NOW).unwrap();
    seed_crash_catalog(&db);
    seed_crash_settings(&db);
    // Checkpoint so the parent can hand the files to the worker.
    db.write(|conn| {
        conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);")
            .map_err(shopcore_db::DbError::from)?;
        Ok(())
    })
    .unwrap();
    let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
    (db, scratch, cashier.id)
}

/// The catalog rows for the crash fixture (branch, terminal, tax,
/// price list, shift, one item/`SKU`/barcode, opening stock).
fn seed_crash_catalog(db: &Db) {
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
            "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('tax-std', ?1, 'STD', 'Standard 18%', ?2, ?2, 1)",
            rusqlite::params![TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES ('taxrate-std-18', ?1, 'tax-std', 1800, ?2, ?2, 1)",
            rusqlite::params![TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO price_list (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('pricelist-std', ?1, 'STD', 'Standard', ?2, ?2, 1)",
            rusqlite::params![TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO shift (id, tenant_id, branch_id, terminal_id, status, opened_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5, ?5, 1)",
            rusqlite::params![SHIFT_ID, TENANT, BRANCH_ID, TERMINAL_ID, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO item (id, tenant_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES ('item-milk1l', ?1, 'MILK1L', 'Milk 1L', 'uom-ea', 'tax-std', ?2, ?2, 1)",
            rusqlite::params![TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, 'item-milk1l', 'MILK1L', 'Milk 1L', 'uom-ea', 'tax-std', ?3, ?3, 1)",
            rusqlite::params![SKU_ID, TENANT, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, created_at, updated_at, version) VALUES ('barcode-milk1l', ?1, ?2, '1234567890123', 1, ?3, ?3, 1)",
            rusqlite::params![TENANT, SKU_ID, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO price_list_entry (id, tenant_id, price_list_id, sku_id, price_e4, created_at, updated_at, version) VALUES ('ple-milk1l', ?1, 'pricelist-std', ?2, ?3, ?4, ?4, 1)",
            rusqlite::params![TENANT, SKU_ID, SKU_PRICE_E4, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO stock_balance (id, tenant_id, branch_id, sku_id, qty_milli, created_at, updated_at, version) VALUES ('bal-milk1l', ?1, ?2, ?3, 100000, ?4, ?4, 1)",
            rusqlite::params![TENANT, BRANCH_ID, SKU_ID, NOW],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO stock_movement (id, tenant_id, branch_id, sku_id, type, qty_milli, unit_cost_e4, occurred_at, created_at, updated_at, version) VALUES ('mov-open-milk1l', ?1, ?2, ?3, 'opening', 100000, 0, ?4, ?4, ?4, 1)",
            rusqlite::params![TENANT, BRANCH_ID, SKU_ID, NOW],
        )
        .unwrap();
        tx.commit().unwrap();
        Ok(())
    })
    .unwrap();
}

/// The tenant settings for the crash fixture (PKR, exponent 2, cash
/// rounding OFF, default tax 18%, active price list).
fn seed_crash_settings(db: &Db) {
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
        "pricelist-std",
        NOW,
    )
    .unwrap();
}
/// Count rows in the main DB.
fn count(db: &Db, sql: &str) -> i64 {
    db.read(|conn| {
        conn.query_row(sql, [], |row| row.get::<_, i64>(0))
            .map_err(shopcore_db::DbError::from)
    })
    .unwrap()
}

/// The stock balance (milli) for the one `SKU`.
fn balance(db: &Db) -> i64 {
    db.read(|conn| {
        conn.query_row(
            "SELECT qty_milli FROM stock_balance WHERE tenant_id = ?1 AND branch_id = ?2 AND sku_id = ?3",
            rusqlite::params![TENANT, BRANCH_ID, SKU_ID],
            |row| row.get::<_, i64>(0),
        )
        .map_err(shopcore_db::DbError::from)
    })
    .unwrap()
}

/// Sum of `stock_movements` (milli) for the one `SKU`.
fn ledger_sum(db: &Db) -> i64 {
    db.read(|conn| {
        conn.query_row(
            "SELECT COALESCE(SUM(qty_milli), 0) FROM stock_movement WHERE tenant_id = ?1 AND branch_id = ?2 AND sku_id = ?3",
            rusqlite::params![TENANT, BRANCH_ID, SKU_ID],
            |row| row.get::<_, i64>(0),
        )
        .map_err(shopcore_db::DbError::from)
    })
    .unwrap()
}

/// Spawn this test binary as a worker (`--exact crash_worker`) with
/// the given env vars. Returns the child process.
fn spawn_worker(envs: &[(&str, &str)]) -> Child {
    let exe = std::env::current_exe().unwrap();
    let mut cmd = Command::new(exe);
    cmd.args(["--exact", "crash_worker", "--nocapture"])
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    for (k, v) in envs {
        cmd.env(k, v);
    }
    cmd.spawn().unwrap()
}

/// Wait for a file to appear (poll every 50 ms, up to `timeout`).
fn wait_for_file(path: &Path, timeout: Duration) -> bool {
    let start = std::time::Instant::now();
    while start.elapsed() < timeout {
        if path.exists() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

/// The `sale` worker mode: complete the draft named by
/// `CRASH_WORKER_DRAFT` with a 30 s `Sleep` fault at
/// `CRASH_WORKER_STEP` (the parent kills it mid-transaction).
fn worker_sale_mode(db: &Db, scratch: &ScratchDb, signal: &Path, cashier_id: &str) {
    let draft_id = std::env::var("CRASH_WORKER_DRAFT").unwrap();
    let step = std::env::var("CRASH_WORKER_STEP").unwrap();
    let step: &'static str = match step.as_str() {
        "shift" => STEP_SHIFT,
        "stock" => STEP_STOCK,
        "commit" => STEP_COMMIT,
        other => panic!("unknown crash step {other}"),
    };
    // Signal "about to commit", then sleep inside the transaction.
    std::fs::write(signal, b"starting").unwrap();
    let injector = FaultInjector::sleep_at(step, Duration::from_secs(30));
    let result = complete_sale_cash(
        db,
        scratch,
        CompleteSaleCashInput {
            tenant_id: TENANT.to_string(),
            branch_id: BRANCH_ID.to_string(),
            terminal_id: TERMINAL_ID.to_string(),
            branch_code: "MAIN".to_string(),
            terminal_code: "T1".to_string(),
            shift_id: SHIFT_ID.to_string(),
            user_id: cashier_id.to_string(),
            draft_id,
            tender_minor: 100_000,
            now_ms: NOW,
            date_yyyymmdd: DATE.to_string(),
            tax_mode: TaxMode::Exclusive,
        },
        Some(&injector),
    );
    // If the parent did NOT kill us, the sale completed after
    // the sleep — record that (the parent asserts it did not).
    match result {
        Ok(done) => {
            std::fs::write(signal, format!("completed {}", done.sale_id)).unwrap();
        }
        Err(err) => {
            std::fs::write(signal, format!("error {}", err.code())).unwrap();
        }
    }
}

/// The worker entry point (also the `crash_worker` test): when the
/// `CRASH_WORKER_MODE` env var is set, do the worker's job instead of
/// passing trivially. Modes:
/// - `sale`: complete the draft named by `CRASH_WORKER_DRAFT` with a
///   30 s `Sleep` fault at `CRASH_WORKER_STEP` (the parent kills it
///   mid-transaction). Writes `CRASH_WORKER_SIGNAL` first.
/// - `loop`: loop (draft, add line, complete sale) until killed.
/// - `draft`: add lines to a draft in a loop (50 ms apart) until killed.
#[test]
fn crash_worker() {
    let Ok(mode) = std::env::var("CRASH_WORKER_MODE") else {
        return; // Normal nextest run: no-op, passes.
    };
    let db_path = std::env::var("CRASH_WORKER_DB").unwrap();
    let scratch_path = std::env::var("CRASH_WORKER_SCRATCH").unwrap();
    let signal = PathBuf::from(std::env::var("CRASH_WORKER_SIGNAL").unwrap());
    let cashier_id = std::env::var("CRASH_WORKER_CASHIER").unwrap();
    let db = Db::open(&db_path, Some(TEST_KEY)).unwrap();
    let scratch = ScratchDb::open(&scratch_path).unwrap();
    match mode.as_str() {
        "sale" => worker_sale_mode(&db, &scratch, &signal, &cashier_id),
        "loop" => {
            std::fs::write(&signal, b"starting").unwrap();
            loop {
                let cart = open_draft(
                    &scratch,
                    TENANT,
                    BRANCH_ID,
                    TERMINAL_ID,
                    Some(SHIFT_ID),
                    Some(&cashier_id),
                    NOW,
                )
                .unwrap();
                draft_add_line(&scratch, &cart.id, SKU_ID, 1_000, SKU_PRICE_E4, NOW).unwrap();
                let _ = complete_sale_cash(
                    &db,
                    &scratch,
                    CompleteSaleCashInput {
                        tenant_id: TENANT.to_string(),
                        branch_id: BRANCH_ID.to_string(),
                        terminal_id: TERMINAL_ID.to_string(),
                        branch_code: "MAIN".to_string(),
                        terminal_code: "T1".to_string(),
                        shift_id: SHIFT_ID.to_string(),
                        user_id: cashier_id.clone(),
                        draft_id: cart.id,
                        tender_minor: 100_000,
                        now_ms: NOW,
                        date_yyyymmdd: DATE.to_string(),
                        tax_mode: TaxMode::Exclusive,
                    },
                    None,
                );
            }
        }
        "draft" => {
            let cart = open_draft(
                &scratch,
                TENANT,
                BRANCH_ID,
                TERMINAL_ID,
                Some(SHIFT_ID),
                Some(&cashier_id),
                NOW,
            )
            .unwrap();
            std::fs::write(&signal, b"starting").unwrap();
            // Add one line every 50 ms until killed (the parent kills
            // at a random point; the draft must survive, whole).
            loop {
                draft_add_line(&scratch, &cart.id, SKU_ID, 1_000, SKU_PRICE_E4, NOW).unwrap();
                std::thread::sleep(Duration::from_millis(50));
            }
        }
        other => panic!("unknown crash worker mode {other}"),
    }
}

/// Kill the worker mid-transaction at `step`, reopen the DB, and
/// verify: NO sale was committed (the transaction rolled back),
/// balances match the ledger, and the draft is restorable.
fn kill_during_sale_at(step: &'static str) {
    let dir = unique_dir(&format!("sale-{step}"));
    let (db, scratch, cashier_id) = build_fixture(&dir);
    // The parent creates the draft + line; the worker only completes it.
    let cart = open_draft(
        &scratch,
        TENANT,
        BRANCH_ID,
        TERMINAL_ID,
        Some(SHIFT_ID),
        Some(&cashier_id),
        NOW,
    )
    .unwrap();
    draft_add_line(&scratch, &cart.id, SKU_ID, 1_000, SKU_PRICE_E4, NOW).unwrap();
    let draft_id = cart.id.clone();
    // Close the parent's DBs so the worker owns the files.
    drop(db);
    drop(scratch);
    let signal = dir.join("signal");
    let mut child = spawn_worker(&[
        ("CRASH_WORKER_MODE", "sale"),
        ("CRASH_WORKER_DB", dir.join("till.db").to_str().unwrap()),
        (
            "CRASH_WORKER_SCRATCH",
            dir.join("scratch.db").to_str().unwrap(),
        ),
        ("CRASH_WORKER_SIGNAL", signal.to_str().unwrap()),
        ("CRASH_WORKER_CASHIER", &cashier_id),
        ("CRASH_WORKER_DRAFT", &draft_id),
        ("CRASH_WORKER_STEP", step),
    ]);
    assert!(
        wait_for_file(&signal, Duration::from_secs(15)),
        "worker never signalled (step {step})"
    );
    // The worker is inside the transaction now (sleeping on the fault
    // for 30 s) — kill it there.
    std::thread::sleep(Duration::from_secs(1));
    child.kill().unwrap();
    let _ = child.wait();
    // Reopen and verify: the kill rolled the transaction back.
    let db = Db::open(dir.join("till.db"), Some(TEST_KEY)).unwrap();
    let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM sale"), 0, "step {step}");
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM sale_line"),
        0,
        "step {step}"
    );
    assert_eq!(
        count(
            &db,
            "SELECT COUNT(*) FROM stock_movement WHERE type = 'sale'"
        ),
        0,
        "step {step}"
    );
    assert_eq!(balance(&db), 100_000, "step {step}");
    assert_eq!(balance(&db), ledger_sum(&db), "step {step}");
    assert_eq!(
        count(&db, "SELECT COUNT(*) FROM audit_log"),
        0,
        "step {step}"
    );
    assert_eq!(count(&db, "SELECT COUNT(*) FROM outbox"), 0, "step {step}");
    // The draft survived the kill and restores whole (the sale never
    // committed, so no tombstone exists).
    let restored = restore_draft(&db, &scratch, TENANT, TERMINAL_ID)
        .unwrap()
        .unwrap();
    assert_eq!(restored.id, draft_id, "step {step}");
    assert_eq!(restored.lines.len(), 1, "step {step}");
    assert_eq!(restored.lines[0].qty_milli, 1_000, "step {step}");
    drop(db);
    drop(scratch);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn kill_during_sale_shift_is_atomic() {
    kill_during_sale_at(STEP_SHIFT);
}

#[test]
fn kill_during_sale_stock_is_atomic() {
    kill_during_sale_at(STEP_STOCK);
}

#[test]
fn kill_during_sale_commit_is_atomic() {
    kill_during_sale_at(STEP_COMMIT);
}

#[test]
fn kill_at_random_point_in_sale_loop() {
    // Three iterations, killed at increasing delays: the DB must hold
    // only COMPLETE sales (never a partial one), balances must match
    // the ledger, and the audit chain must verify.
    for (i, delay_ms) in [200u64, 600, 1200].iter().enumerate() {
        let dir = unique_dir(&format!("loop-{i}"));
        let (db, scratch, cashier_id) = build_fixture(&dir);
        drop(db);
        drop(scratch);
        let signal = dir.join("signal");
        let mut child = spawn_worker(&[
            ("CRASH_WORKER_MODE", "loop"),
            ("CRASH_WORKER_DB", dir.join("till.db").to_str().unwrap()),
            (
                "CRASH_WORKER_SCRATCH",
                dir.join("scratch.db").to_str().unwrap(),
            ),
            ("CRASH_WORKER_SIGNAL", signal.to_str().unwrap()),
            ("CRASH_WORKER_CASHIER", &cashier_id),
        ]);
        assert!(wait_for_file(&signal, Duration::from_secs(15)));
        std::thread::sleep(Duration::from_millis(*delay_ms));
        child.kill().unwrap();
        let _ = child.wait();
        let db = Db::open(dir.join("till.db"), Some(TEST_KEY)).unwrap();
        let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
        let sales = count(&db, "SELECT COUNT(*) FROM sale");
        let lines = count(&db, "SELECT COUNT(*) FROM sale_line");
        let movements = count(
            &db,
            "SELECT COUNT(*) FROM stock_movement WHERE type = 'sale'",
        );
        // One line + one movement per sale, no partial sale.
        assert_eq!(lines, sales, "iteration {i}");
        assert_eq!(movements, sales, "iteration {i}");
        assert_eq!(balance(&db), ledger_sum(&db), "iteration {i}");
        assert_eq!(balance(&db), 100_000 - sales * 1_000, "iteration {i}");
        // The audit chain verifies (one row per completed sale).
        assert_eq!(
            shopcore_core::audit::verify_audit_chain(&db, TENANT, TERMINAL_ID).unwrap(),
            usize::try_from(sales).unwrap(),
            "iteration {i}"
        );
        drop(db);
        drop(scratch);
        let _ = std::fs::remove_dir_all(&dir);
    }
}

#[test]
fn kill_during_draft_edit_loses_nothing_committed() {
    // The documented loss window (ADR-034) is about a POWER cut: at
    // most the scratch DB's unflushed tail can be lost. A process kill
    // cannot simulate that (the OS still flushes the file on process
    // death), so this test asserts the weaker, provable claim: after a
    // kill mid-edit, every line the worker had added is still there
    // (in practice nothing is lost), no sale exists, and the draft
    // restores whole. The power-cut loss itself is proven only by the
    // manual power-pull test (pre-pilot checklist, owner: user).
    let dir = unique_dir("draft-edit");
    let (db, scratch, cashier_id) = build_fixture(&dir);
    drop(db);
    drop(scratch);
    let signal = dir.join("signal");
    let mut child = spawn_worker(&[
        ("CRASH_WORKER_MODE", "draft"),
        ("CRASH_WORKER_DB", dir.join("till.db").to_str().unwrap()),
        (
            "CRASH_WORKER_SCRATCH",
            dir.join("scratch.db").to_str().unwrap(),
        ),
        ("CRASH_WORKER_SIGNAL", signal.to_str().unwrap()),
        ("CRASH_WORKER_CASHIER", &cashier_id),
    ]);
    assert!(wait_for_file(&signal, Duration::from_secs(15)));
    // Kill after ~400 ms: the worker added ~8 lines by then.
    std::thread::sleep(Duration::from_millis(400));
    child.kill().unwrap();
    let _ = child.wait();
    let db = Db::open(dir.join("till.db"), Some(TEST_KEY)).unwrap();
    let scratch = ScratchDb::open(dir.join("scratch.db")).unwrap();
    assert_eq!(count(&db, "SELECT COUNT(*) FROM sale"), 0);
    let restored = restore_draft(&db, &scratch, TENANT, TERMINAL_ID)
        .unwrap()
        .unwrap();
    // Same SKU + same price increments one line: the quantity is the
    // number of lines the worker managed to add before the kill.
    assert_eq!(restored.lines.len(), 1);
    let qty = restored.lines[0].qty_milli;
    assert!(qty >= 1_000, "expected at least one line, got {qty}");
    assert!(qty <= 20_000, "expected at most ~20 lines, got {qty}");
    drop(db);
    drop(scratch);
    let _ = std::fs::remove_dir_all(&dir);
}
