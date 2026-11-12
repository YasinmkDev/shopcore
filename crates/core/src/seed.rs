//! Dev seed (spec 001b-1 req. 11).
//!
//! This module is compiled ONLY with the `dev-seed` cargo feature. No
//! release profile enables it. CI builds the release binary and greps it
//! for [`DEV_SEED_MARKER`]; the build fails if the marker is found (see
//! `.github/workflows/ci.yml`). The seed uses the REAL tables: one branch,
//! one terminal, one open shift, a cashier + a manager with PINs, a sample
//! catalog with barcodes/prices/tax groups, and stock balances.
//!
//! The seed PINs are `1234` (cashier) and `4321` (manager). They are dev
//! fixtures, not secrets — the seed never runs in a release build.

use shopcore_db::Db;

use crate::CoreError;
use crate::settings::{
    KEY_ACTIVE_PRICE_LIST, KEY_CASH_ROUNDING_GRANULARITY_MINOR, KEY_TAX_DEFAULT_RATE_BP,
    KEY_TENANT_CURRENCY, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, set_setting,
};
use crate::users::{create_role, create_user};

/// The unique string CI greps the release binary for. Referenced in the
/// log line below so the string is emitted into any binary that links
/// this module; when the feature is off, the module is not compiled and
/// the string is absent.
pub const DEV_SEED_MARKER: &str = "SHOPCORE_DEV_SEED_MARKER_V1";

/// What the seed created (ids the caller can use to drive the till).
#[derive(Debug, Clone)]
pub struct SeedReport {
    /// The branch id.
    pub branch_id: String,
    /// The terminal id.
    pub terminal_id: String,
    /// The open shift id.
    pub shift_id: String,
    /// The cashier username (`cashier`, PIN `1234`).
    pub cashier_username: String,
    /// The manager username (`manager`, PIN `4321`).
    pub manager_username: String,
    /// The active price list id.
    pub price_list_id: String,
    /// How many SKUs were seeded.
    pub sku_count: usize,
}

/// Seed a fresh database with a usable dev till. Idempotent only on a
/// fresh database — calling it twice fails on the unique constraints
/// (that is intentional: seeding is a fresh-DB operation).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if any write fails, and [`CoreError::Pin`]
/// if a seed PIN cannot be hashed.
pub fn seed_dev_till(db: &Db, tenant_id: &str, now_ms: i64) -> Result<SeedReport, CoreError> {
    tracing::info!(marker = DEV_SEED_MARKER, tenant_id, "applying dev seed");
    let branch_id = uuid::Uuid::now_v7().to_string();
    let terminal_id = uuid::Uuid::now_v7().to_string();
    let shift_id = uuid::Uuid::now_v7().to_string();
    let price_list_id = uuid::Uuid::now_v7().to_string();
    let tax_group_id = uuid::Uuid::now_v7().to_string();
    let zero_tax_group_id = uuid::Uuid::now_v7().to_string();
    let inclusive_tax_group_id = uuid::Uuid::now_v7().to_string();
    let uom_each_id = uuid::Uuid::now_v7().to_string();
    let uom_kg_id = uuid::Uuid::now_v7().to_string();

    // Roles + users (PINs hashed with Argon2id by create_user).
    let cashier_role = create_role(
        db,
        tenant_id,
        "Cashier",
        &["sale.create", "sale.discount.line"],
        500,
        now_ms,
    )?;
    let manager_role = create_role(
        db,
        tenant_id,
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
        now_ms,
    )?;
    create_user(
        db,
        tenant_id,
        "cashier",
        "1234",
        Some(&cashier_role.id),
        now_ms,
    )?;
    create_user(
        db,
        tenant_id,
        "manager",
        "4321",
        Some(&manager_role.id),
        now_ms,
    )?;

    // Branch, terminal, shift, tax, UoMs, price list.
    db.write({
        let branch_id = branch_id.clone();
        let terminal_id = terminal_id.clone();
        let shift_id = shift_id.clone();
        let price_list_id = price_list_id.clone();
        let tax_group_id = tax_group_id.clone();
        let zero_tax_group_id = zero_tax_group_id.clone();
        let inclusive_tax_group_id = inclusive_tax_group_id.clone();
        let uom_each_id = uom_each_id.clone();
        let uom_kg_id = uom_kg_id.clone();
        let tenant_id = tenant_id.to_string();
        move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO branch (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'MAIN', 'Main Branch', ?3, ?3, 1)",
                rusqlite::params![branch_id, tenant_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO terminal (id, tenant_id, branch_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, ?3, 'T1', 'Till 1', ?4, ?4, 1)",
                rusqlite::params![terminal_id, tenant_id, branch_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO shift (id, tenant_id, branch_id, terminal_id, status, opened_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5, ?5, 1)",
                rusqlite::params![shift_id, tenant_id, branch_id, terminal_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard 18%', ?3, ?3, 1)",
                rusqlite::params![tax_group_id, tenant_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, tax_group_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'ZERO', 'Zero-rated', ?3, ?3, 1)",
                rusqlite::params![zero_tax_group_id, tenant_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 0, ?4, ?4, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, zero_tax_group_id, now_ms],
            )?;
            // Inclusive-priced group (price_includes_tax = 1): shelf
            // prices already include the 18% tax (owner decision
            // 2026-11-11 — the mode comes from the data).
            tx.execute(
                "INSERT INTO tax_group (id, tenant_id, code, name, price_includes_tax, created_at, updated_at, version) VALUES (?1, ?2, 'INC', 'Inclusive 18%', 1, ?3, ?3, 1)",
                rusqlite::params![inclusive_tax_group_id, tenant_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, inclusive_tax_group_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO uom (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'EA', 'Each', ?3, ?3, 1)",
                rusqlite::params![uom_each_id, tenant_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO uom (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'KG', 'Kilogram', ?3, ?3, 1)",
                rusqlite::params![uom_kg_id, tenant_id, now_ms],
            )?;
            tx.execute(
                "INSERT INTO price_list (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard', ?3, ?3, 1)",
                rusqlite::params![price_list_id, tenant_id, now_ms],
            )?;
            tx.commit()?;
            Ok(())
        }
    })?;

    // Sample catalog: (code, name, barcode, price_e4, tax_group, uom, stock qty_milli).
    let catalog: [(&str, &str, &str, i64, &str, &str, i64); 6] = [
        (
            "MILK1L",
            "Milk 1L",
            "1234567890123",
            2_500_000,
            "STD",
            "EA",
            100_000,
        ),
        (
            "BRD500",
            "Bread 500g",
            "1234567890124",
            1_200_000,
            "ZERO",
            "EA",
            80_000,
        ),
        (
            "SGR1KG",
            "Sugar 1kg",
            "1234567890125",
            1_800_000,
            "STD",
            "KG",
            50_000,
        ),
        (
            "RCE5KG",
            "Rice 5kg",
            "1234567890126",
            9_500_000,
            "ZERO",
            "KG",
            40_000,
        ),
        (
            "TEA200",
            "Tea 200g",
            "1234567890127",
            950_000,
            "STD",
            "EA",
            120_000,
        ),
        (
            "EGG12",
            "Eggs Dozen",
            "1234567890128",
            3_600_000,
            "ZERO",
            "EA",
            60_000,
        ),
    ];
    let mut sku_count = 0;
    for (code, name, barcode, price_e4, tax, uom, qty_milli) in catalog {
        let tax_group = if tax == "STD" {
            tax_group_id.clone()
        } else {
            zero_tax_group_id.clone()
        };
        let uom_id = if uom == "KG" {
            uom_kg_id.clone()
        } else {
            uom_each_id.clone()
        };
        let item_id = uuid::Uuid::now_v7().to_string();
        let sku_id = uuid::Uuid::now_v7().to_string();
        let tenant_id = tenant_id.to_string();
        let branch_id = branch_id.clone();
        let price_list_id = price_list_id.clone();
        let code = code.to_string();
        let name = name.to_string();
        let barcode = barcode.to_string();
        db.write(move |conn| {
            let tx = conn.transaction()?;
            tx.execute(
                "INSERT INTO item (id, tenant_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
                rusqlite::params![item_id, tenant_id, code, name, uom_id, tax_group, now_ms],
            )?;
            tx.execute(
                "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)",
                rusqlite::params![sku_id, tenant_id, item_id, code, name, uom_id, tax_group, now_ms],
            )?;
            tx.execute(
                "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?5, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, sku_id, barcode, now_ms],
            )?;
            tx.execute(
                "INSERT INTO price_list_entry (id, tenant_id, price_list_id, sku_id, price_e4, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, price_list_id, sku_id, price_e4, now_ms],
            )?;
            tx.execute(
                "INSERT INTO stock_balance (id, tenant_id, branch_id, sku_id, qty_milli, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, branch_id, sku_id, qty_milli, now_ms],
            )?;
            tx.execute(
                "INSERT INTO stock_movement (id, tenant_id, branch_id, sku_id, type, qty_milli, unit_cost_e4, occurred_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 'opening', ?5, 0, ?6, ?6, ?6, 1)",
                rusqlite::params![uuid::Uuid::now_v7().to_string(), tenant_id, branch_id, sku_id, qty_milli, now_ms],
            )?;
            tx.commit()?;
            Ok(())
        })?;
        sku_count += 1;
    }

    // Tenant settings: PKR, exponent 2, cash rounding OFF, tax default
    // 18%, active price list = the seeded one.
    set_setting(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY,
        "PKR",
        now_ms,
    )?;
    set_setting(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
        "2",
        now_ms,
    )?;
    set_setting(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_CASH_ROUNDING_GRANULARITY_MINOR,
        "0",
        now_ms,
    )?;
    set_setting(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_TAX_DEFAULT_RATE_BP,
        "1800",
        now_ms,
    )?;
    set_setting(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_ACTIVE_PRICE_LIST,
        &price_list_id,
        now_ms,
    )?;

    Ok(SeedReport {
        branch_id,
        terminal_id,
        shift_id,
        cashier_username: "cashier".to_string(),
        manager_username: "manager".to_string(),
        price_list_id,
        sku_count,
    })
}
