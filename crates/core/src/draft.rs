//! Draft cart use cases (spec 001b-2 req. 1, 4, 11).
//!
//! Every change to the cart goes through these use cases. The draft is
//! persisted in the SCRATCH database ([`ScratchDb`]) — never in the main
//! DB (req. 2) — and restored after crash/restart by [`restore_draft`].
//! The main-DB parts of these use cases (audit rows, PIN verification,
//! permission checks) go through the `DB` actor, like every other
//! main-DB access. Every draft-MUTATING use case writes through
//! [`ScratchDb::with_conn_write`], which re-arms the checkpoint timer
//! (ADR-034); reads use `with_conn` and do not.
//!
//! Discounts: a line discount is stored on the draft line as an AMOUNT
//! (`discount_minor`, the 001b-1 schema has no discount-kind column).
//! A percent discount is computed against the line gross AT APPLY TIME;
//! if the quantity changes afterwards, the amount stays (the cashier
//! re-applies the discount to re-price it). Applying a discount
//! REPLACES any previous discount on that line.
//!
//! Scanning the same SKU again increments the quantity when the unit
//! price matches; a different price opens a separate line (the price
//! locked at first scan is not silently re-priced).

use std::fmt::Write;

use rusqlite::OptionalExtension;
use shopcore_db::Db;
use shopcore_domain::{LineDiscount, Qty, UnitPrice, line_discount_amount, line_gross};

use crate::CoreError;
use crate::audit::{
    AUDIT_CART_CANCELLED, AUDIT_DISCOUNT_APPLIED, AUDIT_DISCOUNT_APPROVED, AUDIT_DISCOUNT_REJECTED,
    AUDIT_PIN_LOCKOUT, AuditEvent, write_audit_event,
};
use crate::permissions::{PERM_SALE_DISCOUNT_LINE, has_permission};
use crate::pin::PinError;
use crate::sale::SaleError;
use crate::scratch::ScratchDb;
use crate::settings::{KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, get_setting_i64};
use crate::users::{PinLockoutEvent, Role, User, approve_by_manager, get_role};

/// One line of a draft cart (mirrors the `draft_line` row).
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DraftLine {
    /// Line id (`UUIDv7`).
    pub id: String,
    /// The `SKU` id.
    pub sku_id: String,
    /// Quantity, milli-units.
    pub qty_milli: i64,
    /// Unit price at scale 10^4 (locked at scan time).
    pub unit_price_e4: i64,
    /// Line discount, minor units (0 = none).
    pub discount_minor: i64,
}

/// A draft cart: the header plus its lines.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct DraftCart {
    /// Draft id (`UUIDv7` — also the sale's `draft_id` when completed).
    pub id: String,
    /// The tenant.
    pub tenant_id: String,
    /// The branch.
    pub branch_id: String,
    /// The terminal.
    pub terminal_id: String,
    /// The open shift, if the draft was opened under one.
    pub shift_id: Option<String>,
    /// The cashier who opened the draft.
    pub user_id: Option<String>,
    /// Status (`"open"` while editable).
    pub status: String,
    /// The lines, in insertion order.
    pub lines: Vec<DraftLine>,
}

/// Read a draft (header + lines) from the scratch DB. `None` when the
/// draft does not exist.
fn read_draft(conn: &rusqlite::Connection, draft_id: &str) -> Result<Option<DraftCart>, CoreError> {
    let header: Option<DraftCart> = conn
        .query_row(
            "SELECT id, tenant_id, branch_id, terminal_id, shift_id, user_id, status FROM draft_sale WHERE id = ?1 AND deleted_at IS NULL",
            rusqlite::params![draft_id],
            |row| {
                Ok(DraftCart {
                    id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    branch_id: row.get(2)?,
                    terminal_id: row.get(3)?,
                    shift_id: row.get(4)?,
                    user_id: row.get(5)?,
                    status: row.get(6)?,
                    lines: Vec::new(),
                })
            },
        )
        .optional()?;
    let Some(mut cart) = header else {
        return Ok(None);
    };
    let mut stmt = conn.prepare(
        "SELECT id, sku_id, qty_milli, unit_price_e4, discount_minor FROM draft_line WHERE draft_sale_id = ?1 AND deleted_at IS NULL ORDER BY rowid ASC",
    )?;
    let rows = stmt.query_map(rusqlite::params![cart.id], |row| {
        Ok(DraftLine {
            id: row.get(0)?,
            sku_id: row.get(1)?,
            qty_milli: row.get(2)?,
            unit_price_e4: row.get(3)?,
            discount_minor: row.get(4)?,
        })
    })?;
    for row in rows {
        cart.lines.push(row?);
    }
    Ok(Some(cart))
}

/// Open a draft for a terminal: returns the terminal's existing open
/// draft when one exists (a restart restores it — see
/// [`restore_draft`]), otherwise creates a new one.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the scratch write fails.
pub fn open_draft(
    scratch: &ScratchDb,
    tenant_id: &str,
    branch_id: &str,
    terminal_id: &str,
    shift_id: Option<&str>,
    user_id: Option<&str>,
    now_ms: i64,
) -> Result<DraftCart, CoreError> {
    if let Some(existing) = load_open_draft(scratch, tenant_id, terminal_id)? {
        return Ok(existing);
    }
    let id = uuid::Uuid::now_v7().to_string();
    let tenant_id = tenant_id.to_string();
    let branch_id = branch_id.to_string();
    let terminal_id = terminal_id.to_string();
    let shift_id = shift_id.map(str::to_string);
    let user_id = user_id.map(str::to_string);
    let cart = DraftCart {
        id: id.clone(),
        tenant_id: tenant_id.clone(),
        branch_id: branch_id.clone(),
        terminal_id: terminal_id.clone(),
        shift_id: shift_id.clone(),
        user_id: user_id.clone(),
        status: "open".to_string(),
        lines: Vec::new(),
    };
    scratch.with_conn_write(|conn| {
        conn.execute(
            "INSERT INTO draft_sale (id, tenant_id, branch_id, terminal_id, shift_id, user_id, status, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'open', ?7, ?7, 1)",
            rusqlite::params![id, tenant_id, branch_id, terminal_id, shift_id, user_id, now_ms],
        )?;
        Ok(())
    })?;
    Ok(cart)
}

/// Load a draft by id (`None` when it does not exist).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the scratch read fails.
pub fn load_draft(scratch: &ScratchDb, draft_id: &str) -> Result<Option<DraftCart>, CoreError> {
    let draft_id = draft_id.to_string();
    scratch.with_conn(|conn| read_draft(conn, &draft_id))
}

/// Load a terminal's open draft (the most recently updated one),
/// `None` when the terminal has no open draft. Used by [`open_draft`]
/// and by the restart-restore path ([`restore_draft`]).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the scratch read fails.
pub fn load_open_draft(
    scratch: &ScratchDb,
    tenant_id: &str,
    terminal_id: &str,
) -> Result<Option<DraftCart>, CoreError> {
    let tenant_id = tenant_id.to_string();
    let terminal_id = terminal_id.to_string();
    scratch.with_conn(|conn| {
        let id: Option<String> = conn
            .query_row(
                "SELECT id FROM draft_sale WHERE tenant_id = ?1 AND terminal_id = ?2 AND status = 'open' AND deleted_at IS NULL ORDER BY updated_at DESC, rowid DESC LIMIT 1",
                rusqlite::params![tenant_id, terminal_id],
                |row| row.get(0),
            )
            .optional()?;
        match id {
            Some(id) => read_draft(conn, &id),
            None => Ok(None),
        }
    })
}

/// Add a line to a draft. A known barcode's price is resolved by the
/// caller (001b-1 `lookup_barcode`) and passed in — this use case does
/// not look up prices. Scanning the same `SKU` again increments the
/// quantity when the unit price matches; a different price opens a
/// separate line.
///
/// # Errors
///
/// Returns [`CoreError::InvalidInput`] when the draft does not exist,
/// the quantity is not positive, or the price is negative; and
/// [`CoreError::Db`] if the scratch write fails.
pub fn draft_add_line(
    scratch: &ScratchDb,
    draft_id: &str,
    sku_id: &str,
    qty_milli: i64,
    unit_price_e4: i64,
    now_ms: i64,
) -> Result<DraftLine, CoreError> {
    if qty_milli <= 0 {
        return Err(CoreError::InvalidInput(
            "quantity must be positive".to_string(),
        ));
    }
    if unit_price_e4 < 0 {
        return Err(CoreError::InvalidInput(
            "unit price must not be negative".to_string(),
        ));
    }
    let draft_id = draft_id.to_string();
    let sku_id = sku_id.to_string();
    scratch.with_conn_write(|conn| {
        let exists: bool = conn
            .query_row(
                "SELECT EXISTS(SELECT 1 FROM draft_sale WHERE id = ?1 AND status = 'open' AND deleted_at IS NULL)",
                rusqlite::params![draft_id],
                |row| row.get(0),
            )?;
        if !exists {
            return Err(CoreError::InvalidInput(format!(
                "draft {draft_id} does not exist or is not open"
            )));
        }
        // Same SKU + same price: increment the quantity (the 11
        // acceptance: scanning the same item increments quantity).
        let same: Option<(String, i64)> = conn
            .query_row(
                "SELECT id, qty_milli FROM draft_line WHERE draft_sale_id = ?1 AND sku_id = ?2 AND unit_price_e4 = ?3 AND deleted_at IS NULL ORDER BY rowid ASC LIMIT 1",
                rusqlite::params![draft_id, sku_id, unit_price_e4],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?;
        if let Some((line_id, old_qty)) = same {
            let new_qty = old_qty + qty_milli;
            conn.execute(
                "UPDATE draft_line SET qty_milli = ?1, updated_at = ?2 WHERE id = ?3",
                rusqlite::params![new_qty, now_ms, line_id],
            )?;
            conn.execute(
                "UPDATE draft_sale SET updated_at = ?1 WHERE id = ?2",
                rusqlite::params![now_ms, draft_id],
            )?;
            return Ok(DraftLine {
                id: line_id,
                sku_id,
                qty_milli: new_qty,
                unit_price_e4,
                discount_minor: 0,
            });
        }
        let line_id = uuid::Uuid::now_v7().to_string();
        // The tenant is copied from the draft header (the scratch DB
        // has no cross-table join for this in the hot path).
        let tenant_id: String = conn.query_row(
            "SELECT tenant_id FROM draft_sale WHERE id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO draft_line (id, tenant_id, draft_sale_id, sku_id, qty_milli, unit_price_e4, discount_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 0, ?7, ?7, 1)",
            rusqlite::params![line_id, tenant_id, draft_id, sku_id, qty_milli, unit_price_e4, now_ms],
        )?;
        conn.execute(
            "UPDATE draft_sale SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, draft_id],
        )?;
        Ok(DraftLine {
            id: line_id,
            sku_id,
            qty_milli,
            unit_price_e4,
            discount_minor: 0,
        })
    })
}

/// Set a line's quantity. A quantity of 0 removes the line (stashed for
/// single-level undo, like [`draft_remove_line`]). Returns the updated
/// line, or `None` when the line was removed.
///
/// # Errors
///
/// Returns [`CoreError::InvalidInput`] when the draft or line does not
/// exist or the quantity is negative; and [`CoreError::Db`] if the
/// scratch write fails.
pub fn draft_set_qty(
    scratch: &ScratchDb,
    draft_id: &str,
    line_id: &str,
    qty_milli: i64,
    now_ms: i64,
) -> Result<Option<DraftLine>, CoreError> {
    if qty_milli < 0 {
        return Err(CoreError::InvalidInput(
            "quantity must not be negative".to_string(),
        ));
    }
    if qty_milli == 0 {
        let removed = draft_remove_line(scratch, draft_id, line_id, now_ms)?;
        if removed {
            return Ok(None);
        }
        return Err(CoreError::InvalidInput(format!(
            "line {line_id} does not exist in draft {draft_id}"
        )));
    }
    let draft_id = draft_id.to_string();
    let line_id = line_id.to_string();
    scratch.with_conn_write(|conn| {
        let line: Option<DraftLine> = conn
            .query_row(
                "SELECT id, sku_id, qty_milli, unit_price_e4, discount_minor FROM draft_line WHERE id = ?1 AND draft_sale_id = ?2 AND deleted_at IS NULL",
                rusqlite::params![line_id, draft_id],
                |row| {
                    Ok(DraftLine {
                        id: row.get(0)?,
                        sku_id: row.get(1)?,
                        qty_milli: row.get(2)?,
                        unit_price_e4: row.get(3)?,
                        discount_minor: row.get(4)?,
                    })
                },
            )
            .optional()?;
        let Some(mut line) = line else {
            return Err(CoreError::InvalidInput(format!(
                "line {line_id} does not exist in draft {draft_id}"
            )));
        };
        conn.execute(
            "UPDATE draft_line SET qty_milli = ?1, updated_at = ?2 WHERE id = ?3",
            rusqlite::params![qty_milli, now_ms, line_id],
        )?;
        conn.execute(
            "UPDATE draft_sale SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, draft_id],
        )?;
        line.qty_milli = qty_milli;
        Ok(Some(line))
    })
}

/// Remove a line from a draft, stashing it in the single-level undo slot
/// (any previously removed line is overwritten — one level only).
/// Returns `true` when a line was removed, `false` when the line did
/// not exist.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the scratch write fails.
pub fn draft_remove_line(
    scratch: &ScratchDb,
    draft_id: &str,
    line_id: &str,
    now_ms: i64,
) -> Result<bool, CoreError> {
    let draft_id = draft_id.to_string();
    let line_id = line_id.to_string();
    scratch.with_conn_write(|conn| {
        let line: Option<DraftLine> = conn
            .query_row(
                "SELECT id, sku_id, qty_milli, unit_price_e4, discount_minor FROM draft_line WHERE id = ?1 AND draft_sale_id = ?2 AND deleted_at IS NULL",
                rusqlite::params![line_id, draft_id],
                |row| {
                    Ok(DraftLine {
                        id: row.get(0)?,
                        sku_id: row.get(1)?,
                        qty_milli: row.get(2)?,
                        unit_price_e4: row.get(3)?,
                        discount_minor: row.get(4)?,
                    })
                },
            )
            .optional()?;
        let Some(line) = line else {
            return Ok(false);
        };
        let tenant_id: String = conn.query_row(
            "SELECT tenant_id FROM draft_sale WHERE id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT OR REPLACE INTO draft_removed_line (draft_sale_id, id, tenant_id, sku_id, qty_milli, unit_price_e4, discount_minor, removed_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
            rusqlite::params![draft_id, line.id, tenant_id, line.sku_id, line.qty_milli, line.unit_price_e4, line.discount_minor, now_ms],
        )?;
        conn.execute(
            "DELETE FROM draft_line WHERE id = ?1",
            rusqlite::params![line.id],
        )?;
        conn.execute(
            "UPDATE draft_sale SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, draft_id],
        )?;
        Ok(true)
    })
}

/// Restore the most recently removed line (single-level undo). Returns
/// `true` when a line was restored; a second call is a no-op (`false`).
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the scratch write fails.
pub fn draft_restore_line(
    scratch: &ScratchDb,
    draft_id: &str,
    now_ms: i64,
) -> Result<bool, CoreError> {
    let draft_id = draft_id.to_string();
    scratch.with_conn_write(|conn| {
        let removed: Option<DraftLine> = conn
            .query_row(
                "SELECT id, sku_id, qty_milli, unit_price_e4, discount_minor FROM draft_removed_line WHERE draft_sale_id = ?1",
                rusqlite::params![draft_id],
                |row| {
                    Ok(DraftLine {
                        id: row.get(0)?,
                        sku_id: row.get(1)?,
                        qty_milli: row.get(2)?,
                        unit_price_e4: row.get(3)?,
                        discount_minor: row.get(4)?,
                    })
                },
            )
            .optional()?;
        let Some(line) = removed else {
            return Ok(false);
        };
        let tenant_id: String = conn.query_row(
            "SELECT tenant_id FROM draft_sale WHERE id = ?1",
            rusqlite::params![draft_id],
            |row| row.get(0),
        )?;
        conn.execute(
            "INSERT INTO draft_line (id, tenant_id, draft_sale_id, sku_id, qty_milli, unit_price_e4, discount_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1)",
            rusqlite::params![line.id, tenant_id, draft_id, line.sku_id, line.qty_milli, line.unit_price_e4, line.discount_minor, now_ms],
        )?;
        conn.execute(
            "DELETE FROM draft_removed_line WHERE draft_sale_id = ?1",
            rusqlite::params![draft_id],
        )?;
        conn.execute(
            "UPDATE draft_sale SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, draft_id],
        )?;
        Ok(true)
    })
}

/// Apply a line discount. The cashier's role ceiling
/// (`max_line_discount_bps`) is the threshold: a discount above it
/// requires a manager PIN (`approver = (manager_username, pin)` —
/// select-manager + PIN, never PIN-only, 001b-1). Without approval the
/// discount is REJECTED and audited (`discount.rejected`); with a PIN
/// it is APPROVED and audited with both users (`discount.approved`). A
/// within-threshold discount is applied and audited as
/// `discount.applied`. Returns the discount amount applied, in minor
/// units.
///
/// The approver must differ from the cashier (07: approver ≠ cashier
/// unless policy allows; no such policy exists yet, so self-approval
/// is rejected). A manager PIN failure that triggers a lockout also
/// persists the lockout event (`pin.lockout`, the 001b-1 seam).
///
/// # Errors
///
/// Returns [`SaleError::DiscountNeedsApproval`] when the discount is
/// above the cashier's threshold and no (valid) manager approval was
/// given; [`SaleError::PermissionDenied`] when the cashier lacks
/// `sale.discount.line`; [`SaleError::InvalidInput`] when the draft or
/// line does not exist or the discount exceeds the line total; and
/// [`SaleError::Core`] / [`SaleError::Db`] for PIN or database failures.
#[allow(clippy::too_many_arguments)]
pub fn draft_apply_discount(
    db: &Db,
    scratch: &ScratchDb,
    draft_id: &str,
    line_id: &str,
    discount: LineDiscount,
    cashier: &crate::users::User,
    approver: Option<(&str, &str)>,
    now_ms: i64,
) -> Result<i64, SaleError> {
    let draft = load_draft(scratch, draft_id)?
        .ok_or_else(|| SaleError::InvalidInput(format!("draft {draft_id} does not exist")))?;
    let line = draft
        .lines
        .iter()
        .find(|l| l.id == line_id)
        .ok_or_else(|| {
            SaleError::InvalidInput(format!("line {line_id} does not exist in draft {draft_id}"))
        })?
        .clone();
    let role = check_discount_permission(db, cashier)?;
    let (gross_minor, amount_minor) =
        compute_discount_amount(db, &cashier.tenant_id, &line, &discount)?;
    // Above the threshold? Compare exactly (i128, no rounding): the
    // discount is above the ceiling when amount × 10000 > ceiling × gross.
    let above = i128::from(amount_minor) * 10_000
        > i128::from(role.max_line_discount_bps) * i128::from(gross_minor);
    let mut approved_by: Option<String> = None;
    if above {
        let Some((manager_username, pin)) = approver else {
            reject_discount(
                db,
                &draft,
                cashier,
                &line,
                amount_minor,
                &format!(
                    "discount {amount_minor} minor is above the cashier ceiling of {} bps; no manager PIN given",
                    role.max_line_discount_bps
                ),
                now_ms,
            )?;
            return Err(SaleError::DiscountNeedsApproval);
        };
        approved_by = Some(approve_discount_by_manager(
            db,
            &DiscountAttempt {
                draft: &draft,
                cashier,
                line: &line,
                amount_minor,
                manager_username,
                pin,
                now_ms,
            },
        )?);
    }
    // Apply (replaces any previous discount on the line), then audit.
    scratch.with_conn_write(|conn| {
        conn.execute(
            "UPDATE draft_line SET discount_minor = ?1, updated_at = ?2 WHERE id = ?3 AND draft_sale_id = ?4",
            rusqlite::params![amount_minor, now_ms, line_id, draft_id],
        )?;
        conn.execute(
            "UPDATE draft_sale SET updated_at = ?1 WHERE id = ?2",
            rusqlite::params![now_ms, draft_id],
        )?;
        Ok(())
    })?;
    let (action, after) = match &approved_by {
        Some(manager_id) => (
            AUDIT_DISCOUNT_APPROVED,
            format!(
                "{{\"cashier_id\":\"{}\",\"manager_id\":\"{}\",\"discount_minor\":{}}}",
                cashier.id, manager_id, amount_minor
            ),
        ),
        None => (
            AUDIT_DISCOUNT_APPLIED,
            format!(
                "{{\"cashier_id\":\"{}\",\"discount_minor\":{}}}",
                cashier.id, amount_minor
            ),
        ),
    };
    write_audit_event(
        db,
        &AuditEvent {
            tenant_id: draft.tenant_id.clone(),
            terminal_id: draft.terminal_id.clone(),
            user_id: Some(cashier.id.clone()),
            action: action.to_string(),
            entity: Some("draft".to_string()),
            entity_id: Some(draft.id.clone()),
            before_json: Some(discount_summary_json(&line, amount_minor)),
            after_json: Some(after),
            reason: None,
            at_ms: now_ms,
        },
    )
    .map_err(SaleError::Core)?;
    Ok(amount_minor)
}

/// The cashier must hold `sale.discount.line`; returns their role
/// (whose `max_line_discount_bps` is the approval threshold).
fn check_discount_permission(db: &Db, cashier: &User) -> Result<Role, SaleError> {
    let role_id = cashier.role_id.clone().ok_or_else(|| {
        SaleError::PermissionDenied(format!("cashier {} has no role", cashier.username))
    })?;
    let role = get_role(db, &cashier.tenant_id, &role_id)?
        .ok_or_else(|| SaleError::PermissionDenied(format!("role {role_id} does not exist")))?;
    if !has_permission(&role, PERM_SALE_DISCOUNT_LINE) {
        return Err(SaleError::PermissionDenied(
            "sale.discount.line".to_string(),
        ));
    }
    Ok(role)
}

/// The discount amount, from the 001a domain (validates it against
/// the line total). Returns `(gross_minor, amount_minor)`.
fn compute_discount_amount(
    db: &Db,
    tenant_id: &str,
    line: &DraftLine,
    discount: &LineDiscount,
) -> Result<(i64, i64), SaleError> {
    let exponent = get_setting_i64(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
    )
    .map_err(SaleError::Core)?
    .unwrap_or(2);
    let exponent_u32 = u32::try_from(exponent).unwrap_or(2);
    let gross = line_gross(
        UnitPrice::from_e4(line.unit_price_e4).map_err(SaleError::Domain)?,
        Qty::from_milli(line.qty_milli).map_err(SaleError::Domain)?,
        exponent_u32,
    )
    .map_err(SaleError::Domain)?;
    let amount = line_discount_amount(gross, discount).map_err(SaleError::Domain)?;
    Ok((gross.minor(), amount.minor()))
}

/// Writes a `discount.rejected` audit row for a discount attempt.
fn reject_discount(
    db: &Db,
    draft: &DraftCart,
    cashier: &User,
    line: &DraftLine,
    amount_minor: i64,
    reason: &str,
    now_ms: i64,
) -> Result<(), SaleError> {
    write_audit_event(
        db,
        &AuditEvent {
            tenant_id: draft.tenant_id.clone(),
            terminal_id: draft.terminal_id.clone(),
            user_id: Some(cashier.id.clone()),
            action: AUDIT_DISCOUNT_REJECTED.to_string(),
            entity: Some("draft".to_string()),
            entity_id: Some(draft.id.clone()),
            before_json: Some(discount_summary_json(line, amount_minor)),
            after_json: None,
            reason: Some(reason.to_string()),
            at_ms: now_ms,
        },
    )
    .map_err(SaleError::Core)?;
    Ok(())
}

/// Persists the `pin.lockout` audit row for a lockout triggered by a
/// manager-PIN attempt (the 001b-1 seam).
fn persist_lockout_event(
    db: &Db,
    draft: &DraftCart,
    event: &PinLockoutEvent,
    now_ms: i64,
) -> Result<(), SaleError> {
    write_audit_event(
        db,
        &AuditEvent {
            tenant_id: event.tenant_id.clone(),
            terminal_id: draft.terminal_id.clone(),
            user_id: Some(event.user_id.clone()),
            action: AUDIT_PIN_LOCKOUT.to_string(),
            entity: Some("user".to_string()),
            entity_id: Some(event.user_id.clone()),
            before_json: None,
            after_json: Some(format!(
                "{{\"username\":\"{}\",\"locked_until_ms\":{},\"failed_attempts\":{}}}",
                event.username, event.locked_until_ms, event.failed_attempts
            )),
            reason: Some("too many failed PIN attempts".to_string()),
            at_ms: now_ms,
        },
    )
    .map_err(SaleError::Core)?;
    Ok(())
}

/// The inputs to a manager approval attempt (bundled so the helper
/// stays under clippy's argument limit).
struct DiscountAttempt<'a> {
    draft: &'a DraftCart,
    cashier: &'a User,
    line: &'a DraftLine,
    amount_minor: i64,
    manager_username: &'a str,
    pin: &'a str,
    now_ms: i64,
}

/// Verifies the manager PIN for an above-threshold discount. Returns
/// the manager's user id. A failure writes the `discount.rejected`
/// audit row (and persists the `pin.lockout` event when the attempt
/// triggered a lockout) and returns `DiscountNeedsApproval`.
fn approve_discount_by_manager(db: &Db, attempt: &DiscountAttempt) -> Result<String, SaleError> {
    let DiscountAttempt {
        draft,
        cashier,
        line,
        amount_minor,
        manager_username,
        pin,
        now_ms,
    } = attempt;
    match approve_by_manager(db, &draft.tenant_id, manager_username, pin, *now_ms) {
        Ok(manager) => {
            if manager.id == cashier.id {
                reject_discount(
                    db,
                    draft,
                    cashier,
                    line,
                    *amount_minor,
                    "self-approval is not allowed: the approver must differ from the cashier",
                    *now_ms,
                )?;
                return Err(SaleError::DiscountNeedsApproval);
            }
            Ok(manager.id)
        }
        Err(PinError::LockedOut { event, .. }) => {
            // Persist the lockout event (the 001b-1 seam), then the
            // rejection. A lockout triggered by THIS call carries the
            // event; an already-locked manager carries none.
            if let Some(ev) = event {
                persist_lockout_event(db, draft, &ev, *now_ms)?;
            }
            reject_discount(
                db,
                draft,
                cashier,
                line,
                *amount_minor,
                "manager PIN verification failed (locked out)",
                *now_ms,
            )?;
            Err(SaleError::DiscountNeedsApproval)
        }
        Err(PinError::InvalidCredentials) => {
            reject_discount(
                db,
                draft,
                cashier,
                line,
                *amount_minor,
                "manager PIN verification failed",
                *now_ms,
            )?;
            Err(SaleError::DiscountNeedsApproval)
        }
        Err(other) => Err(SaleError::Core(CoreError::Pin(other))),
    }
}
/// The discount summary carried in an audit row's `before_json`.
fn discount_summary_json(line: &DraftLine, discount_minor: i64) -> String {
    format!(
        "{{\"line_id\":\"{}\",\"sku_id\":\"{}\",\"qty_milli\":{},\"unit_price_e4\":{},\"discount_minor\":{}}}",
        line.id, line.sku_id, line.qty_milli, line.unit_price_e4, discount_minor
    )
}

/// Cancel a draft (discard it). If the cart had lines, an AUDIT row is
/// written (`cart.cancelled`, with the cart contents summarised).
/// Returns `true` when a draft was cancelled, `false` when there was
/// nothing to cancel.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the scratch or audit write fails.
pub fn draft_cancel(
    db: &Db,
    scratch: &ScratchDb,
    draft_id: &str,
    user_id: &str,
    now_ms: i64,
) -> Result<bool, CoreError> {
    let Some(draft) = load_draft(scratch, draft_id)? else {
        return Ok(false);
    };
    if !draft.lines.is_empty() {
        let summary = cart_summary_json(&draft);
        write_audit_event(
            db,
            &AuditEvent {
                tenant_id: draft.tenant_id.clone(),
                terminal_id: draft.terminal_id.clone(),
                user_id: Some(user_id.to_string()),
                action: AUDIT_CART_CANCELLED.to_string(),
                entity: Some("draft".to_string()),
                entity_id: Some(draft.id.clone()),
                before_json: Some(summary),
                after_json: Some("{\"status\":\"cancelled\"}".to_string()),
                reason: Some("cart cancelled by the cashier".to_string()),
                at_ms: now_ms,
            },
        )?;
    }
    let draft_id = draft_id.to_string();
    scratch.with_conn_write(|conn| {
        conn.execute(
            "DELETE FROM draft_removed_line WHERE draft_sale_id = ?1",
            rusqlite::params![draft_id],
        )?;
        conn.execute(
            "DELETE FROM draft_line WHERE draft_sale_id = ?1",
            rusqlite::params![draft_id],
        )?;
        conn.execute(
            "DELETE FROM draft_sale WHERE id = ?1",
            rusqlite::params![draft_id],
        )?;
        Ok(())
    })?;
    Ok(true)
}

/// The cart summary carried in a cancel audit row's `before_json`.
fn cart_summary_json(draft: &DraftCart) -> String {
    let mut lines = String::from("[");
    for (i, line) in draft.lines.iter().enumerate() {
        if i > 0 {
            lines.push(',');
        }
        let _ = write!(
            lines,
            "{{\"line_id\":\"{}\",\"sku_id\":\"{}\",\"qty_milli\":{},\"unit_price_e4\":{},\"discount_minor\":{}}}",
            line.id, line.sku_id, line.qty_milli, line.unit_price_e4, line.discount_minor
        );
    }
    lines.push(']');
    format!(
        "{{\"draft_id\":\"{}\",\"line_count\":{},\"lines\":{}}}",
        draft.id,
        draft.lines.len(),
        lines
    )
}

/// Restore a terminal's open draft after crash/restart. Discards a
/// draft whose sale already exists (tombstoned, or a sale row with the
/// same `draft_id` in the main DB) — a late scratch-DB flush must NOT
/// resurrect a completed draft. Returns the draft to restore, or `None`
/// when there is nothing (or nothing left) to restore.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if a read or the discard write fails.
pub fn restore_draft(
    db: &Db,
    scratch: &ScratchDb,
    tenant_id: &str,
    terminal_id: &str,
) -> Result<Option<DraftCart>, CoreError> {
    let Some(draft) = load_open_draft(scratch, tenant_id, terminal_id)? else {
        return Ok(None);
    };
    // Tombstoned? Discard it (it completed; the flush arrived late).
    if scratch.is_tombstoned(&draft.id)? {
        discard_draft(scratch, &draft.id)?;
        return Ok(None);
    }
    // A sale already exists for this draft? Discard it and tombstone it
    // (the sale row is authoritative — the tombstone makes the discard
    // durable in the scratch DB too).
    let tenant_id = tenant_id.to_string();
    let draft_id = draft.id.clone();
    let sale_exists: bool = db.read(move |conn| {
        let n: i64 = conn.query_row(
            "SELECT COUNT(*) FROM sale WHERE tenant_id = ?1 AND draft_id = ?2 AND deleted_at IS NULL",
            rusqlite::params![tenant_id, draft_id],
            |row| row.get(0),
        )?;
        Ok(n > 0)
    })?;
    if sale_exists {
        let now_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(0));
        // The sale id for the tombstone (best-effort: the discard is
        // what matters; a missing sale id must not block it).
        let sale_id: Option<String> = db.read({
            let tenant_id = draft.tenant_id.clone();
            let draft_id = draft.id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT id FROM sale WHERE tenant_id = ?1 AND draft_id = ?2 AND deleted_at IS NULL",
                    rusqlite::params![tenant_id, draft_id],
                    |row| row.get(0),
                )
                .optional().map_err(shopcore_db::DbError::Sql)
            }
        })?;
        if let Some(sale_id) = sale_id {
            let _ = scratch.tombstone(&draft.id, &sale_id, now_ms);
        }
        discard_draft(scratch, &draft.id)?;
        return Ok(None);
    }
    Ok(Some(draft))
}

/// Delete a draft and its lines + undo slot from the scratch DB.
fn discard_draft(scratch: &ScratchDb, draft_id: &str) -> Result<(), CoreError> {
    let draft_id = draft_id.to_string();
    scratch.with_conn_write(|conn| {
        conn.execute(
            "DELETE FROM draft_removed_line WHERE draft_sale_id = ?1",
            rusqlite::params![draft_id],
        )?;
        conn.execute(
            "DELETE FROM draft_line WHERE draft_sale_id = ?1",
            rusqlite::params![draft_id],
        )?;
        conn.execute(
            "DELETE FROM draft_sale WHERE id = ?1",
            rusqlite::params![draft_id],
        )?;
        Ok(())
    })
}
