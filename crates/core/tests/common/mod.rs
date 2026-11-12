//! Shared setup for `shopcore-core` integration tests: a fresh
//! in-memory `Db` with the 001b-1 migrations applied, plus helpers to
//! create roles/users and a minimal catalog.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::missing_panics_doc,
    clippy::must_use_candidate,
    dead_code
)]

use shopcore_core::settings::{
    KEY_ACTIVE_PRICE_LIST, KEY_TAX_DEFAULT_RATE_BP, KEY_TENANT_CURRENCY,
    KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, set_setting,
};
use shopcore_core::users::{create_role, create_user};
use shopcore_db::{Db, migrations::run_migrations};
use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

pub const TENANT: &str = "tenant-001";
pub const NOW: i64 = 1_700_000_000_000;

/// A fresh in-memory `Db` with migrations applied (idempotent re-run
/// included — the second `run_migrations` must be a no-op).
pub fn fresh_db() -> Db {
    let db = Db::open_in_memory().unwrap();
    db.write(run_migrations).unwrap();
    db.write(run_migrations).unwrap();
    db
}

/// Create a role + a user with the given PIN. Returns the user id.
pub fn make_user(db: &Db, username: &str, pin: &str, permissions: &[&str]) -> String {
    let role = create_role(
        db,
        TENANT,
        &format!("role-{username}"),
        permissions,
        1000,
        NOW,
    )
    .unwrap();
    create_user(db, TENANT, username, pin, Some(&role.id), NOW)
        .unwrap()
        .id
}

/// Insert a minimal catalog row set directly (one item + sku + barcode +
/// price list entry). Returns the sku id.
#[allow(clippy::too_many_arguments)]
pub fn insert_sku(
    db: &Db,
    code: &str,
    name: &str,
    barcode: &str,
    price_e4: i64,
    tax_group_id: &str,
    price_list_id: &str,
) -> String {
    let sku_id = uuid::Uuid::now_v7().to_string();
    let item_id = uuid::Uuid::now_v7().to_string();
    let tenant = TENANT.to_string();
    let code = code.to_string();
    let name = name.to_string();
    let barcode = barcode.to_string();
    let tax_group_id = tax_group_id.to_string();
    let price_list_id = price_list_id.to_string();
    let sku_id_out = sku_id.clone();
    db.write(move |conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO item (id, tenant_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, '', ?5, ?6, ?6, 1)",
            rusqlite::params![item_id, tenant, code, name, tax_group_id, NOW],
        )?;
        tx.execute(
            "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, '', ?6, ?7, ?7, 1)",
            rusqlite::params![sku_id, tenant, item_id, code, name, tax_group_id, NOW],
        )?;
        tx.execute(
            "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5, 1)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant, sku_id, barcode, NOW],
        )?;
        tx.execute(
            "INSERT INTO price_list_entry (id, tenant_id, price_list_id, sku_id, price_e4, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant, price_list_id, sku_id, price_e4, NOW],
        )?;
        tx.commit()?;
        Ok(())
    })
    .unwrap();
    sku_id_out
}

/// Create a tax group + rate and a price list; set the tenant settings
/// the search path needs (currency, active price list).
pub fn setup_pricing(db: &Db) -> (String, String) {
    let tax_group_id = uuid::Uuid::now_v7().to_string();
    let price_list_id = uuid::Uuid::now_v7().to_string();
    let tenant = TENANT.to_string();
    let tax_group_id_c = tax_group_id.clone();
    let price_list_id_c = price_list_id.clone();
    db.write(move |conn| {
        let tx = conn.transaction()?;
        tx.execute(
            "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard', ?3, ?3, 1)",
            rusqlite::params![tax_group_id_c, tenant, NOW],
        )?;
        tx.execute(
            "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
            rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant, tax_group_id_c, NOW],
        )?;
        tx.execute(
            "INSERT INTO price_list (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard', ?3, ?3, 1)",
            rusqlite::params![price_list_id_c, tenant, NOW],
        )?;
        tx.commit()?;
        Ok(())
    })
    .unwrap();
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
        &price_list_id,
        NOW,
    )
    .unwrap();
    (tax_group_id, price_list_id)
}

// ---------------------------------------------------------------------------
// Test scratch directories (fix for the 2026-11-11 template-dir collision).
//
// The old helper named dirs `{label}-{pid}-{n}` under a never-cleaned
// `target/test-tmp-*/` root. PIDs are reused across runs, so a new test
// process could reopen a leftover template DB and the role seeding
// failed with `UNIQUE constraint failed: role.tenant_id, role.name`.
// The fix: PID + a random UUIDv7 (unique per process AND per dir),
// `create_dir` (never `create_dir_all`) so a collision fails loudly,
// stale dirs older than 1 hour removed at startup, and the template dir
// removed at process exit (best effort, ADR-035).
// ---------------------------------------------------------------------------

/// Root for the sale/crash test scratch dirs (real disk, not `/tmp`
/// tmpfs — the crash tests need real `fsync` behaviour).
pub fn test_root_001b2() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp-001b2")
}

/// Root for the scratch-DB test dirs (same reason as `test_root_001b2`).
pub fn test_root_scratch() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target/test-tmp-scratch")
}

/// A unique test scratch directory under `root`, created with
/// `create_dir` (fail loudly if it exists). The name is
/// `{label}-{pid}-{uuidv7}`: the PID alone is NOT unique across runs,
/// and the UUID makes a collision across runs and within a run
/// effectively impossible. The caller owns the dir and must remove it
/// (per-test dirs via `Drop` or an explicit removal; the template dir
/// via `register_exit_cleanup_dir`).
pub fn unique_test_dir(root: &Path, label: &str) -> PathBuf {
    cleanup_stale_test_dirs(root);
    // Ensure the shared root exists. `create_dir` + ignore
    // `AlreadyExists` (the root is shared, not per-test — only the LEAF
    // must be fresh, and it is created with `create_dir` below).
    match std::fs::create_dir(root) {
        Ok(()) => {}
        Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => {}
        Err(e) => panic!("cannot create test root {}: {e}", root.display()),
    }
    let dir = root.join(format!(
        "{label}-{}-{}",
        std::process::id(),
        uuid::Uuid::now_v7()
    ));
    std::fs::create_dir(&dir).unwrap_or_else(|e| {
        panic!(
            "test dir {} already exists (UUID collision or stale dir): {e}",
            dir.display()
        )
    });
    dir
}

/// Remove test scratch subdirs older than 1 hour under `root` (best
/// effort). Runs on every `unique_test_dir` call: a concurrently-running
/// test process only has fresh dirs (< 1 hour), so it is never touched;
/// dirs from a killed or crashed run are reclaimed by the next run.
/// The 1-hour threshold also means a test run that itself lasts longer
/// than an hour is not affected (a full `just check` takes ~4 min).
///
/// This is also what removes the template dir: the template lives in a
/// `OnceLock`, so no `Drop` ever runs for it, and a process-exit hook
/// (`atexit`) is NOT used — it needs `unsafe`, and the workspace
/// forbids `unsafe` code (`unsafe_code = "forbid"`, root `Cargo.toml`).
/// The stale-cleanup bounds the accumulation to ~1 hour of runs, and
/// `just clean-test-tmp` removes everything on demand (ADR-035).
fn cleanup_stale_test_dirs(root: &Path) {
    let Ok(entries) = std::fs::read_dir(root) else {
        return;
    };
    let cutoff = SystemTime::now() - Duration::from_secs(3600);
    for entry in entries.flatten() {
        if !entry.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        let Ok(mtime) = entry.metadata().and_then(|m| m.modified()) else {
            continue;
        };
        if mtime < cutoff {
            let _ = std::fs::remove_dir_all(entry.path());
        }
    }
}
