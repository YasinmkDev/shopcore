//! Users, roles, PIN verification, lockout, and manager approval
//! (spec 001b-1 req. 6-8, `07-security-compliance`).
//!
//! PINs are hashed with `Argon2id` (see `pin.rs`) and never logged. The
//! lockout state lives on the `user` row (`failed_attempts`,
//! `locked_until`). Every lockout produces a [`PinLockoutEvent`] — the
//! seam 001b-2's audit writer consumes (001b-1 emits the event, 001b-2
//! persists it). Manager approval is ALWAYS select-manager + PIN: the
//! approver is chosen by username, then enters their PIN. There is no
//! bare-PIN approval path.

use rusqlite::OptionalExtension;
use shopcore_db::Db;

use crate::CoreError;
use crate::pin::{PinError, hash_pin, verify_against_dummy_hash, verify_pin_hash};
use crate::settings::{
    DEFAULT_PIN_LOCKOUT_DURATION_MINUTES, DEFAULT_PIN_LOCKOUT_MAX_FAILURES,
    KEY_PIN_LOCKOUT_DURATION_MINUTES, KEY_PIN_LOCKOUT_MAX_FAILURES, SCOPE_TENANT, get_setting_i64,
};

/// Minimum PIN length (07: PIN length >= 4).
pub const MIN_PIN_LEN: usize = 4;

/// A user row (without the PIN hash — the hash is loaded only inside
/// `verify_pin` and never leaves this module).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct User {
    /// `UUIDv7` id.
    pub id: String,
    /// The tenant this user belongs to.
    pub tenant_id: String,
    /// Login name, unique per tenant.
    pub username: String,
    /// The role id (`None` until a role is assigned).
    pub role_id: Option<String>,
    /// Consecutive failed PIN verifications since the last success.
    pub failed_attempts: i64,
    /// Epoch ms until which the user is locked out (`None` = not locked).
    pub locked_until_ms: Option<i64>,
}

/// A role row: a permission set plus the per-role line-discount ceiling
/// (the manager-PIN threshold in 001b-2 compares against it).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct Role {
    /// `UUIDv7` id.
    pub id: String,
    /// The tenant this role belongs to.
    pub tenant_id: String,
    /// Role name, unique per tenant (e.g. `Cashier`, `Store Manager`).
    pub name: String,
    /// Permission strings from the 07 list (e.g. `sale.create`).
    pub permissions: Vec<String>,
    /// The most this role may discount a line, in basis points.
    pub max_line_discount_bps: i64,
}

/// A PIN-lockout event. Emitted by [`verify_pin`] when a verification
/// triggers a lockout; 001b-2's audit writer persists it (the seam is
/// explicit: 001b-1 emits, 001b-2 persists).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PinLockoutEvent {
    /// The tenant the user belongs to.
    pub tenant_id: String,
    /// The locked-out user's id.
    pub user_id: String,
    /// The locked-out user's username.
    pub username: String,
    /// Epoch ms until which the user is locked out.
    pub locked_until_ms: i64,
    /// The failed-attempt count that triggered the lockout.
    pub failed_attempts: i64,
}

/// Load a user by username, plus the stored PIN hash (internal only).
/// The hash is returned alongside the user so it never lands in the
/// `User` struct (which is serializable and crosses module boundaries).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
fn load_user_row(
    db: &Db,
    tenant_id: &str,
    username: &str,
) -> Result<Option<(User, String)>, shopcore_db::DbError> {
    let tenant_id = tenant_id.to_string();
    let username = username.to_string();
    let row = db.read(move |conn| {
        let row = conn
            .query_row(
                "SELECT id, tenant_id, username, role_id, failed_attempts, locked_until, pin_hash FROM user WHERE tenant_id = ?1 AND username = ?2 AND deleted_at IS NULL",
                rusqlite::params![tenant_id, username],
                |row| {
                    Ok((
                        User {
                            id: row.get(0)?,
                            tenant_id: row.get(1)?,
                            username: row.get(2)?,
                            role_id: row.get(3)?,
                            failed_attempts: row.get(4)?,
                            locked_until_ms: row.get(5)?,
                        },
                        row.get::<_, String>(6)?,
                    ))
                },
            )
            .optional()?;
        Ok(row)
    })?;
    Ok(row)
}

/// Create a role.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the write fails, and
/// [`CoreError::InvalidInput`] if the permission list cannot be encoded.
pub fn create_role(
    db: &Db,
    tenant_id: &str,
    name: &str,
    permissions: &[&str],
    max_line_discount_bps: i64,
    now_ms: i64,
) -> Result<Role, CoreError> {
    let permissions_json = serde_json::to_string(permissions)
        .map_err(|_| CoreError::InvalidInput("permissions cannot be encoded".to_string()))?;
    let role = Role {
        id: uuid::Uuid::now_v7().to_string(),
        tenant_id: tenant_id.to_string(),
        name: name.to_string(),
        permissions: permissions.iter().map(|p| (*p).to_string()).collect(),
        max_line_discount_bps,
    };
    let row = role.clone();
    db.write(move |conn| {
        conn.execute(
            "INSERT INTO role (id, tenant_id, name, permissions_json, max_line_discount_bps, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1)",
            rusqlite::params![row.id, row.tenant_id, row.name, permissions_json, row.max_line_discount_bps, now_ms],
        )?;
        Ok(())
    })?;
    Ok(role)
}

/// Create a user with a PIN (hashed with `Argon2id` before storage).
///
/// # Errors
///
/// Returns [`PinError::PinTooShort`] if the PIN is shorter than
/// [`MIN_PIN_LEN`], and [`CoreError::Db`] if the write fails.
pub fn create_user(
    db: &Db,
    tenant_id: &str,
    username: &str,
    pin: &str,
    role_id: Option<&str>,
    now_ms: i64,
) -> Result<User, CoreError> {
    if pin.chars().count() < MIN_PIN_LEN {
        return Err(CoreError::Pin(PinError::PinTooShort { min: MIN_PIN_LEN }));
    }
    let pin_hash = hash_pin(pin)?;
    let user = User {
        id: uuid::Uuid::now_v7().to_string(),
        tenant_id: tenant_id.to_string(),
        username: username.to_string(),
        role_id: role_id.map(str::to_string),
        failed_attempts: 0,
        locked_until_ms: None,
    };
    let row = user.clone();
    db.write(move |conn| {
        conn.execute(
            "INSERT INTO user (id, tenant_id, username, pin_hash, role_id, failed_attempts, locked_until, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, 0, NULL, ?6, ?6, 1)",
            rusqlite::params![row.id, row.tenant_id, row.username, pin_hash, row.role_id, now_ms],
        )?;
        Ok(())
    })?;
    Ok(user)
}

/// Load a user by username (`None` when the username does not exist).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn get_user_by_username(
    db: &Db,
    tenant_id: &str,
    username: &str,
) -> Result<Option<User>, CoreError> {
    Ok(load_user_row(db, tenant_id, username)?.map(|(user, _)| user))
}

/// Load a role by id (`None` when the role does not exist).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn get_role(db: &Db, tenant_id: &str, role_id: &str) -> Result<Option<Role>, CoreError> {
    let tenant_id = tenant_id.to_string();
    let role_id = role_id.to_string();
    let row = db.read(move |conn| {
        let row = conn
            .query_row(
                "SELECT id, tenant_id, name, permissions_json, max_line_discount_bps FROM role WHERE tenant_id = ?1 AND id = ?2 AND deleted_at IS NULL",
                rusqlite::params![tenant_id, role_id],
                |row| {
                    let permissions_json: String = row.get(3)?;
                    let permissions: Vec<String> =
                        serde_json::from_str(&permissions_json).unwrap_or_default();
                    Ok(Role {
                        id: row.get(0)?,
                        tenant_id: row.get(1)?,
                        name: row.get(2)?,
                        permissions,
                        max_line_discount_bps: row.get(4)?,
                    })
                },
            )
            .optional()?;
        Ok(row)
    })?;
    Ok(row)
}

/// Verify a username + PIN. On success returns the user and resets the
/// lockout counters. On a wrong PIN increments the counter; after
/// `security.pin_lockout.max_failures` failures (default 5) the user is
/// locked out for `security.pin_lockout.duration_minutes` (default 5) and
/// the error carries the [`PinLockoutEvent`]. An unknown username costs
/// one `Argon2id` verify against the dummy hash
/// (`verify_against_dummy_hash`) and fails exactly like a wrong PIN, so
/// the timing does not reveal whether the user exists.
///
/// `now_ms` is the injected clock (epoch ms) — the caller supplies it so
/// the lockout is testable without waiting.
///
/// # Errors
///
/// Returns [`PinError::InvalidCredentials`] for a wrong PIN or an unknown
/// username, [`PinError::LockedOut`] when the user is (or just became)
/// locked out, [`PinError::InvalidHash`] when the stored hash is corrupt,
/// and [`PinError::Db`] when a counter write fails.
pub fn verify_pin(
    db: &Db,
    tenant_id: &str,
    username: &str,
    pin: &str,
    now_ms: i64,
) -> Result<User, PinError> {
    let Some((user, pin_hash)) = load_user_row(db, tenant_id, username)? else {
        // Unknown username: spend one Argon2id verify against the dummy
        // hash, then fail exactly like a wrong PIN (no user enumeration
        // via timing or error). The verify result is ignored — the dummy
        // PIN is never assigned to a user.
        let _ = verify_against_dummy_hash(pin)?;
        return Err(PinError::InvalidCredentials);
    };
    if let Some(until) = user.locked_until_ms
        && until > now_ms
    {
        return Err(PinError::LockedOut {
            until_ms: until,
            event: None,
        });
    }
    if !verify_pin_hash(pin, &pin_hash)? {
        return Err(record_failed_attempt(db, &user, now_ms)?);
    }
    reset_lockout(db, &user.id, now_ms)?;
    Ok(user)
}

/// Verify a username + PIN off the calling thread (via `spawn_blocking`).
/// This is the form UI-adjacent code must use — the sync [`verify_pin`]
/// is CPU-bound and must never run on the `Tauri` main thread or inside
/// an async task.
///
/// # Errors
///
/// Same as [`verify_pin`], plus [`PinError::VerifyFailed`] if the
/// blocking task itself fails.
pub async fn verify_pin_async(
    db: Db,
    tenant_id: String,
    username: String,
    pin: String,
    now_ms: i64,
) -> Result<User, PinError> {
    tokio::task::spawn_blocking(move || verify_pin(&db, &tenant_id, &username, &pin, now_ms))
        .await
        .map_err(|_| PinError::VerifyFailed)?
}

/// Approve as a manager: the approver is chosen by username FIRST, then
/// enters their PIN. A bare PIN with no named approver is rejected
/// ([`PinError::ManagerNotSelected`]) — there is no API that verifies a
/// PIN without a username.
///
/// # Errors
///
/// Same as [`verify_pin`], plus [`PinError::ManagerNotSelected`] when no
/// manager username was given.
pub fn approve_by_manager(
    db: &Db,
    tenant_id: &str,
    manager_username: &str,
    pin: &str,
    now_ms: i64,
) -> Result<User, PinError> {
    if manager_username.trim().is_empty() {
        return Err(PinError::ManagerNotSelected);
    }
    verify_pin(db, tenant_id, manager_username, pin, now_ms)
}

/// Record a failed verification: increment the counter atomically on the
/// actor thread (the count is re-read inside the write, so concurrent
/// attempts cannot lose an increment). Returns the error the caller must
/// surface: `InvalidCredentials`, or `LockedOut` (with the event) when
/// this failure hit the threshold.
/// Map a `CoreError` (only its `Db` variant is reachable here) into a
/// `PinError` without losing the underlying database error.
fn pin_db(err: CoreError) -> PinError {
    match err {
        CoreError::Db(db) => PinError::Db(db),
        other => PinError::Db(shopcore_db::DbError::InvalidInput(other.to_string())),
    }
}

fn record_failed_attempt(db: &Db, user: &User, now_ms: i64) -> Result<PinError, PinError> {
    let max_failures = get_setting_i64(
        db,
        &user.tenant_id,
        SCOPE_TENANT,
        "",
        KEY_PIN_LOCKOUT_MAX_FAILURES,
    )
    .map_err(pin_db)?
    .unwrap_or(DEFAULT_PIN_LOCKOUT_MAX_FAILURES);
    let duration_minutes = get_setting_i64(
        db,
        &user.tenant_id,
        SCOPE_TENANT,
        "",
        KEY_PIN_LOCKOUT_DURATION_MINUTES,
    )
    .map_err(pin_db)?
    .unwrap_or(DEFAULT_PIN_LOCKOUT_DURATION_MINUTES);
    // The write closure cannot return a value (`Db::write` returns `()`),
    // so the outcome travels out through a shared cell.
    let outcome = std::sync::Arc::new(std::sync::Mutex::new(None::<PinError>));
    let outcome_in = std::sync::Arc::clone(&outcome);
    let user_id = user.id.clone();
    let tenant_id = user.tenant_id.clone();
    let username = user.username.clone();
    db.write(move |conn| {
        let attempts: i64 = conn.query_row(
            "SELECT failed_attempts FROM user WHERE id = ?1",
            rusqlite::params![user_id],
            |row| row.get(0),
        )?;
        let attempts = attempts + 1;
        if attempts >= max_failures {
            let until_ms = now_ms + duration_minutes * 60_000;
            conn.execute(
                "UPDATE user SET failed_attempts = 0, locked_until = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![until_ms, now_ms, user_id],
            )?;
            let event = PinLockoutEvent {
                tenant_id: tenant_id.clone(),
                user_id: user_id.clone(),
                username: username.clone(),
                locked_until_ms: until_ms,
                failed_attempts: attempts,
            };
            if let Ok(mut slot) = outcome_in.lock() {
                *slot = Some(PinError::LockedOut {
                    until_ms,
                    event: Some(event),
                });
            }
        } else {
            conn.execute(
                "UPDATE user SET failed_attempts = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![attempts, now_ms, user_id],
            )?;
            if let Ok(mut slot) = outcome_in.lock() {
                *slot = Some(PinError::InvalidCredentials);
            }
        }
        Ok(())
    })
    .map_err(PinError::Db)?;
    Ok(outcome
        .lock()
        .map_err(|_| {
            PinError::Db(shopcore_db::DbError::InvalidInput(
                "lockout cell poisoned".to_string(),
            ))
        })?
        .take()
        .unwrap_or(PinError::InvalidCredentials))
}

/// Reset a user's lockout counters after a successful verification.
///
/// # Errors
///
/// Returns [`PinError::Db`] if the write fails.
fn reset_lockout(db: &Db, user_id: &str, now_ms: i64) -> Result<(), PinError> {
    let user_id = user_id.to_string();
    db.write(move |conn| {
        conn.execute(
            "UPDATE user SET failed_attempts = 0, locked_until = NULL, updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, user_id],
        )?;
        Ok(())
    })
    .map_err(PinError::Db)
}
