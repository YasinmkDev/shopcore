//! The `settings` table (spec 001b-1 req. 12): scoped key/value.
//!
//! Scopes are `tenant` / `branch` / `terminal` (`scope_id` is the branch or
//! terminal id, empty for tenant scope). Values are `TEXT`; typed getters
//! parse them. Reads go through the `DB` actor's read pool, writes through
//! the actor — no direct access.

use rusqlite::OptionalExtension;
use shopcore_db::Db;

use crate::CoreError;

/// Scope: the whole tenant.
pub const SCOPE_TENANT: &str = "tenant";
/// Scope: one branch.
pub const SCOPE_BRANCH: &str = "branch";
/// Scope: one terminal.
pub const SCOPE_TERMINAL: &str = "terminal";

/// Tenant currency code (e.g. `PKR`) — ADR-017: one currency per tenant.
pub const KEY_TENANT_CURRENCY: &str = "tenant.currency";
/// Tenant currency minor-unit exponent (e.g. `2` for PKR).
pub const KEY_TENANT_CURRENCY_EXPONENT: &str = "tenant.currency_exponent";
/// Cash-rounding granularity in minor units (`0` = OFF, the default).
pub const KEY_CASH_ROUNDING_GRANULARITY_MINOR: &str = "cash_rounding.granularity_minor";
/// Default tax rate in basis points (e.g. `1800` = 18%).
pub const KEY_TAX_DEFAULT_RATE_BP: &str = "tax.default_rate_bp";
/// The active price list id for the tenant.
pub const KEY_ACTIVE_PRICE_LIST: &str = "catalog.active_price_list";
/// PIN lockout: failed verifications before lockout (default 5, req. 8).
pub const KEY_PIN_LOCKOUT_MAX_FAILURES: &str = "security.pin_lockout.max_failures";
/// PIN lockout: lockout duration in minutes (default 5, req. 8).
pub const KEY_PIN_LOCKOUT_DURATION_MINUTES: &str = "security.pin_lockout.duration_minutes";
/// Catalog version counter (req. 4, amendment 9): bumped by triggers on
/// every catalog write; the in-memory prefix index rebuilds on a mismatch.
pub const KEY_CATALOG_VERSION: &str = "catalog.version";

/// Default for [`KEY_PIN_LOCKOUT_MAX_FAILURES`].
pub const DEFAULT_PIN_LOCKOUT_MAX_FAILURES: i64 = 5;
/// Default for [`KEY_PIN_LOCKOUT_DURATION_MINUTES`].
pub const DEFAULT_PIN_LOCKOUT_DURATION_MINUTES: i64 = 5;

/// Read a setting. Returns `None` when the key is not set.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn get_setting(
    db: &Db,
    tenant_id: &str,
    scope: &str,
    scope_id: &str,
    key: &str,
) -> Result<Option<String>, CoreError> {
    let tenant_id = tenant_id.to_string();
    let scope = scope.to_string();
    let scope_id = scope_id.to_string();
    let key = key.to_string();
    let value = db.read(move |conn| {
        let value: Option<String> = conn
            .query_row(
                "SELECT value FROM settings WHERE tenant_id = ?1 AND scope = ?2 AND scope_id = ?3 AND key = ?4 AND deleted_at IS NULL",
                rusqlite::params![tenant_id, scope, scope_id, key],
                |row| row.get(0),
            )
            .optional()?;
        Ok(value)
    })?;
    Ok(value)
}

/// Read a setting as an `i64`. Returns `None` when the key is not set or
/// the value does not parse as an integer.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn get_setting_i64(
    db: &Db,
    tenant_id: &str,
    scope: &str,
    scope_id: &str,
    key: &str,
) -> Result<Option<i64>, CoreError> {
    Ok(get_setting(db, tenant_id, scope, scope_id, key)?.and_then(|v| v.parse().ok()))
}

/// Write a setting (insert or update). Creates the row with a fresh
/// `UUIDv7` id when it does not exist.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the write fails.
pub fn set_setting(
    db: &Db,
    tenant_id: &str,
    scope: &str,
    scope_id: &str,
    key: &str,
    value: &str,
    now_ms: i64,
) -> Result<(), CoreError> {
    let tenant_id = tenant_id.to_string();
    let scope = scope.to_string();
    let scope_id = scope_id.to_string();
    let key = key.to_string();
    let value = value.to_string();
    db.write(move |conn| {
        conn.execute(
            "INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)
             ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at, version = settings.version + 1",
            rusqlite::params![
                uuid::Uuid::now_v7().to_string(),
                tenant_id,
                scope,
                scope_id,
                key,
                value,
                now_ms
            ],
        )?;
        Ok(())
    })?;
    Ok(())
}

/// Read the catalog version counter for a tenant (0 when no catalog write
/// has happened yet).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn get_catalog_version(db: &Db, tenant_id: &str) -> Result<u64, CoreError> {
    Ok(
        get_setting_i64(db, tenant_id, SCOPE_TENANT, "", KEY_CATALOG_VERSION)?
            .map_or(0, |v| u64::try_from(v).unwrap_or(0)),
    )
}
