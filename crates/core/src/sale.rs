//! The sale transaction (spec 001b-2 req. 3, 5, 6, 7, 9, 10).
//!
//! `complete_sale_cash` is ONE SQLite transaction writing the sale,
//! lines, per-component tax rows (`sale_line_tax`), payment, stock
//! movements, balances, aggregates, audit, outbox, receipt counter,
//! and print job. Totals come from the 001a domain functions — this
//! module never recomputes them. Stock balances are written ONLY by
//! the posting function ([`post_stock_movement`]), never directly.
//! The sale commits only if a shift is open and the caller holds
//! `sale.create`. It is IDEMPOTENT per `draft_id`: a second call with
//! the same `draft_id` returns the EXISTING sale (no second sale, no
//! second stock posting).
//!
//! Tax mode: the mode comes from the DATA — each line's tax group
//! carries `price_includes_tax` (0 = exclusive, 1 = inclusive), so a
//! sale's lines may mix modes and each line uses its own group's flag
//! ([`tax_spec_for_group`]). There is no sale-level mode parameter and
//! no tenant default (owner decision 2026-11-11).
//!
//! Timestamps: epoch-ms integers + the `now_ms` helper, consistent
//! with 001b-1 (owner decision 2026-11-11: epoch ms stays in the DB;
//! ISO-8601 UTC with `Z` is the wire/export format only, converted in
//! the `protocol` crate).
//!
//! Hardware never blocks the commit: the sale writes a `print_job`
//! row (status `queued`) and returns the [`PrintDoc`]; the caller
//! attempts the print afterwards via [`attempt_print`] (which maps an
//! offline printer to [`PrintStatus::Queued`], the stable code
//! `PRINT_QUEUED`). The cash drawer is kicked by [`kick_drawer`]
//! after the sale, best-effort.

use std::fmt::Write;

use rusqlite::{OptionalExtension, Transaction};
use shopcore_db::Db;
use shopcore_domain::{
    BillDiscount, CartResult, CashRounding, LineDiscount, LineInput, Money, Qty, Rate, TaxSpec,
    UnitPrice, compute_change, next_receipt_number, price_cart,
};
use shopcore_hardware::{
    CashDrawer, HardwareError, PrintDoc, PrintLine, PrintPayment, ReceiptPrinter,
};

use crate::CoreError;
use crate::audit::{AUDIT_RECEIPT_REPRINT, AUDIT_SALE_COMPLETED, AuditEvent, write_audit};
use crate::draft::{DraftLine, load_draft};
use crate::permissions::{PERM_SALE_CREATE, has_permission};
use crate::scratch::ScratchDb;
use crate::settings::{
    KEY_CASH_ROUNDING_GRANULARITY_MINOR, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT,
    get_setting_i64,
};
use crate::users::get_role;

/// Errors from the sale use cases. Each maps to a stable code via
/// [`SaleError::code`] (mapped to localized messages at the boundary).
#[derive(Debug, thiserror::Error)]
pub enum SaleError {
    /// A database error (actor, migration, or query).
    #[error("database error: {0}")]
    Db(#[from] shopcore_db::DbError),
    /// A domain error (pricing, tax, cash, receipt).
    #[error("domain error: {0}")]
    Domain(#[from] shopcore_domain::DomainError),
    /// A core error (audit, draft, PIN, settings, permissions).
    #[error("core error: {0}")]
    Core(#[from] CoreError),
    /// A hardware error (printer / drawer).
    #[error("hardware error: {0}")]
    Hardware(#[from] HardwareError),
    /// The requested row does not exist.
    #[error("not found: {0}")]
    NotFound(String),
    /// The caller passed an invalid argument.
    #[error("invalid input: {0}")]
    InvalidInput(String),
    /// No shift is open for this sale.
    #[error("no shift is open")]
    ShiftNotOpen,
    /// A line discount above the cashier's threshold needs a manager PIN.
    #[error("discount needs manager approval")]
    DiscountNeedsApproval,
    /// Cash tender is below the total.
    #[error("insufficient tender")]
    InsufficientTender,
    /// No `SKU` has this barcode.
    #[error("barcode not found: {0}")]
    BarcodeNotFound(String),
    /// The caller lacks a permission.
    #[error("permission denied: {0}")]
    PermissionDenied(String),
    /// A fault-injection point fired (tests only — never in production).
    #[error("fault injected at step {0}")]
    FaultInjected(&'static str),
}

// A `SQLite` failure is a database error: map it to the `Db` variant
// directly, so `?` on a `rusqlite` call works in one step inside the
// sale use cases (the `#[from]` on `Db` only covers `DbError`).
impl From<rusqlite::Error> for SaleError {
    fn from(err: rusqlite::Error) -> Self {
        Self::Db(shopcore_db::DbError::Sql(err))
    }
}

impl SaleError {
    /// The stable error code for this error (mapped to a localized
    /// message at the UI boundary).
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Db(_) => "DB_ERROR",
            Self::Domain(err) => match err {
                shopcore_domain::DomainError::InsufficientTender => "INSUFFICIENT_TENDER",
                _ => "DOMAIN_ERROR",
            },
            Self::Core(_) => "CORE_ERROR",
            Self::Hardware(_) => "HARDWARE_ERROR",
            Self::NotFound(_) => "NOT_FOUND",
            Self::InvalidInput(_) => "INVALID_INPUT",
            Self::ShiftNotOpen => "SHIFT_NOT_OPEN",
            Self::DiscountNeedsApproval => "DISCOUNT_NEEDS_APPROVAL",
            Self::InsufficientTender => "INSUFFICIENT_TENDER",
            Self::BarcodeNotFound(_) => "BARCODE_NOT_FOUND",
            Self::PermissionDenied(_) => "PERMISSION_DENIED",
            Self::FaultInjected(_) => "FAULT_INJECTED",
        }
    }
}

/// Whether the receipt printed or is queued (the stable code
/// `PRINT_QUEUED` is the queued case, mapped to a localized message at
/// the boundary). The sale itself always succeeds; only the print
/// queues.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrintStatus {
    /// The receipt printed.
    Printed,
    /// The printer was offline; the receipt is queued for reprint.
    Queued,
}

impl PrintStatus {
    /// The stable code for this status.
    #[must_use]
    pub fn code(&self) -> &'static str {
        match self {
            Self::Printed => "PRINTED",
            Self::Queued => "PRINT_QUEUED",
        }
    }
}

/// A fault-injection point for the crash tests (spec 001b-2, `11`).
/// The sale use case checks the injector at each commit step; a `Fail`
/// fault returns [`SaleError::FaultInjected`] (the transaction rolls
/// back), a `Sleep` fault pauses inside the transaction (so a real
/// process kill can land mid-commit). Production passes `None`.
#[derive(Debug, Clone, Copy)]
pub struct FaultInjector {
    /// The step to fail or sleep at (one of the `STEP_*` constants).
    pub step: &'static str,
    /// What to do at that step.
    pub kind: FaultKind,
}

/// What a fault-injection point does.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FaultKind {
    /// Fail the sale at this step (transaction rolls back).
    Fail,
    /// Sleep here, inside the transaction (kill window for the crash
    /// test). The duration is the sleep length.
    Sleep(std::time::Duration),
}

impl FaultInjector {
    /// Fail the sale at `step`.
    #[must_use]
    pub fn fail_at(step: &'static str) -> Self {
        Self {
            step,
            kind: FaultKind::Fail,
        }
    }

    /// Sleep at `step`, inside the transaction.
    #[must_use]
    pub fn sleep_at(step: &'static str, duration: std::time::Duration) -> Self {
        Self {
            step,
            kind: FaultKind::Sleep(duration),
        }
    }

    /// Check this step against the injector.
    fn check(&self, step: &str) -> Result<(), SaleError> {
        if self.step != step {
            return Ok(());
        }
        match self.kind {
            FaultKind::Fail => Err(SaleError::FaultInjected(self.step)),
            FaultKind::Sleep(duration) => {
                std::thread::sleep(duration);
                Ok(())
            }
        }
    }
}

/// Fault-injection steps, in commit order. The crash tests inject a
/// failure at EACH of these and assert the database is unchanged.
pub const STEP_IDEMPOTENCY: &str = "idempotency";
/// The shift-open check.
pub const STEP_SHIFT: &str = "shift";
/// The pricing step (domain `price_cart` + cash rounding + change).
pub const STEP_PRICING: &str = "pricing";
/// The receipt-counter read + increment.
pub const STEP_RECEIPT_COUNTER: &str = "receipt_counter";
/// The `sale` row insert.
pub const STEP_SALE: &str = "sale";
/// The `sale_line` rows insert.
pub const STEP_LINES: &str = "lines";
/// The `sale_line_tax` rows insert.
pub const STEP_TAX_ROWS: &str = "tax_rows";
/// The `sale_payment` row insert.
pub const STEP_PAYMENT: &str = "payment";
/// The stock movements + balances (posting function).
pub const STEP_STOCK: &str = "stock";
/// The aggregates update.
pub const STEP_AGGREGATES: &str = "aggregates";
/// The audit row insert.
pub const STEP_AUDIT: &str = "audit";
/// The outbox rows insert.
pub const STEP_OUTBOX: &str = "outbox";
/// The `print_job` row insert.
pub const STEP_PRINT_JOB: &str = "print_job";
/// Just before `COMMIT` (the last chance to fail).
pub const STEP_COMMIT: &str = "commit";

/// Input to [`complete_sale_cash`]. All ids are strings; `now_ms` and
/// `date_yyyymmdd` are injected (fake clock in tests).
#[derive(Debug, Clone)]
pub struct CompleteSaleCashInput {
    /// The tenant.
    pub tenant_id: String,
    /// The branch.
    pub branch_id: String,
    /// The terminal.
    pub terminal_id: String,
    /// The branch code (for the receipt number).
    pub branch_code: String,
    /// The terminal code (for the receipt number).
    pub terminal_code: String,
    /// The open shift id.
    pub shift_id: String,
    /// The cashier user id (must hold `sale.create`).
    pub user_id: String,
    /// The draft to complete (its id is the sale's `draft_id`).
    pub draft_id: String,
    /// Cash tendered, minor units.
    pub tender_minor: i64,
    /// When, epoch ms.
    pub now_ms: i64,
    /// The sale date as `yyyymmdd` (for the receipt number and the
    /// daily aggregates).
    pub date_yyyymmdd: String,
}

/// A completed cash sale.
#[derive(Debug, Clone)]
pub struct SaleCompleted {
    /// The sale id.
    pub sale_id: String,
    /// The receipt number (`{branch}-{terminal}-{yyyymmdd}-{seq}`).
    pub receipt_no: String,
    /// The total actually charged (after cash rounding, if on).
    pub total_minor: i64,
    /// The change due (`tender − total`).
    pub change_minor: i64,
    /// The separate untaxed rounding line (`rounded − total`, 0 when
    /// rounding is off).
    pub rounding_line_minor: i64,
    /// The `print_job` row for the receipt.
    pub print_job_id: String,
    /// The receipt document (queued in `print_job`; the caller prints
    /// it via [`attempt_print`]).
    pub print_doc: PrintDoc,
    /// `true` until a print attempt succeeds (the receipt is queued).
    pub receipt_queued: bool,
    /// `true` when this call committed a NEW sale; `false` when it
    /// returned an already-completed sale (idempotent replay).
    pub is_fresh: bool,
}

impl SaleCompleted {
    /// `true` when this call returned an already-completed sale
    /// (idempotent replay) instead of committing a new one — the
    /// inverse of [`Self::is_fresh`]. The returned sale is the
    /// ORIGINAL sale, unchanged: same id, totals, and change as the
    /// first call, regardless of the tender this call passed.
    #[must_use]
    pub fn already_completed(&self) -> bool {
        !self.is_fresh
    }
}

/// The stock posting function — the ONLY writer of `stock_balance`
/// (AGENTS.md). Inserts one append-only `stock_movement` and updates
/// the balance in the same transaction. The balance is the sum of the
/// movements; a sale that takes it below zero still posts (negative
/// stock is ALLOWED and recorded — the `balance < 0` query flags it,
/// see [`negative_stock_skus`]).
///
/// Returns the new balance, in milli-units.
///
/// # Errors
///
/// Returns [`SaleError::Db`] if the movement insert or the balance
/// upsert fails.
#[allow(clippy::too_many_arguments)]
pub fn post_stock_movement(
    tx: &Transaction<'_>,
    tenant_id: &str,
    branch_id: &str,
    sku_id: &str,
    movement_type: &str,
    qty_milli: i64,
    unit_cost_e4: i64,
    ref_type: &str,
    ref_id: &str,
    now_ms: i64,
) -> Result<i64, SaleError> {
    let movement_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO stock_movement (id, tenant_id, branch_id, sku_id, type, qty_milli, unit_cost_e4, ref_type, ref_id, occurred_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, ?10, 1)",
        rusqlite::params![movement_id, tenant_id, branch_id, sku_id, movement_type, qty_milli, unit_cost_e4, ref_type, ref_id, now_ms],
    )?;
    // The balance is maintained ONLY here. A new balance row takes the
    // movement's cost as its average cost; an existing row keeps its
    // average cost (a sale does not re-price stock — the weighted
    // average moves on receipts, a later spec).
    let balance_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO stock_balance (id, tenant_id, branch_id, sku_id, qty_milli, avg_cost_e4, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1) ON CONFLICT(tenant_id, branch_id, sku_id) DO UPDATE SET qty_milli = qty_milli + excluded.qty_milli, updated_at = excluded.updated_at, version = stock_balance.version + 1",
        rusqlite::params![balance_id, tenant_id, branch_id, sku_id, qty_milli, unit_cost_e4, now_ms],
    )?;
    let balance: i64 = tx.query_row(
        "SELECT qty_milli FROM stock_balance WHERE tenant_id = ?1 AND branch_id = ?2 AND sku_id = ?3",
        rusqlite::params![tenant_id, branch_id, sku_id],
        |row| row.get(0),
    )?;
    Ok(balance)
}

/// SKUs whose balance is below zero (the "flagged" negative-stock
/// query — no new table, no new column). Returns `(sku_id,
/// qty_milli)` pairs.
///
/// # Errors
///
/// Returns [`CoreError::Db`] if the read fails.
pub fn negative_stock_skus(
    db: &Db,
    tenant_id: &str,
    branch_id: &str,
) -> Result<Vec<(String, i64)>, CoreError> {
    let tenant_id = tenant_id.to_string();
    let branch_id = branch_id.to_string();
    Ok(db.read(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT sku_id, qty_milli FROM stock_balance WHERE tenant_id = ?1 AND branch_id = ?2 AND qty_milli < 0 AND deleted_at IS NULL ORDER BY sku_id",
        )?;
        let rows = stmt.query_map(rusqlite::params![tenant_id, branch_id], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
        })?;
        let mut out = Vec::new();
        for row in rows {
            out.push(row?);
        }
        Ok(out)
    })?)
}

/// Build a line's [`TaxSpec`] from its tax group, plus the
/// `(tax_rate_id, rate_bp)` pairs (in group order) for the
/// `sale_line_tax` rows. Rates are ordered by creation
/// (`created_at, id` — `UUIDv7` ids are time-ordered, so the id breaks
/// same-millisecond ties in creation order).
///
/// Rules: a `SKU` with NO tax group is an error (the catalog is
/// incomplete — tax cannot be computed without a group, and a
/// `sale_line_tax` row needs a `tax_rate_id`). A group with NO rate
/// rows, or with ALL rates zero, is exempt (no `sale_line_tax` rows).
/// Otherwise the rates apply per the group's `price_includes_tax`
/// flag (0 = exclusive, 1 = inclusive) — the mode comes from the
/// data, so a sale's lines may mix modes.
///
/// # Errors
///
/// Returns [`SaleError::InvalidInput`] when the `SKU` has no tax group
/// or a rate is out of range, and [`SaleError::Db`] if the read fails.
pub fn tax_spec_for_group(
    tx: &Transaction<'_>,
    tenant_id: &str,
    tax_group_id: Option<&str>,
) -> Result<(TaxSpec, Vec<(String, i64)>), SaleError> {
    let Some(group_id) = tax_group_id else {
        return Err(SaleError::InvalidInput(
            "SKU has no tax group; tax cannot be computed".to_string(),
        ));
    };
    // The mode comes from the data (owner decision 2026-11-11): a
    // missing group row reads as exclusive (the column defaults to 0).
    let price_includes_tax: i64 = tx
        .query_row(
            "SELECT price_includes_tax FROM tax_group WHERE tenant_id = ?1 AND id = ?2 AND deleted_at IS NULL",
            rusqlite::params![tenant_id, group_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let mut stmt = tx.prepare(
        "SELECT id, rate_bp FROM tax_rate WHERE tenant_id = ?1 AND tax_group_id = ?2 AND deleted_at IS NULL ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(rusqlite::params![tenant_id, group_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut pairs: Vec<(String, i64)> = Vec::new();
    for row in rows {
        pairs.push(row?);
    }
    if pairs.is_empty() {
        return Ok((
            TaxSpec::Exempt {
                reason: "no tax rates configured for group".to_string(),
            },
            Vec::new(),
        ));
    }
    if pairs.iter().all(|(_, bp)| *bp == 0) {
        return Ok((
            TaxSpec::Exempt {
                reason: "zero-rated".to_string(),
            },
            Vec::new(),
        ));
    }
    let mut rates: Vec<Rate> = Vec::with_capacity(pairs.len());
    for (_, bp) in &pairs {
        let bp_u32 = u32::try_from(*bp)
            .map_err(|_| SaleError::InvalidInput(format!("tax rate {bp} bp is out of range")))?;
        rates.push(Rate::from_basis_points(bp_u32).map_err(SaleError::Domain)?);
    }
    let spec = if price_includes_tax != 0 {
        TaxSpec::Inclusive { rates }
    } else {
        TaxSpec::Exclusive { rates }
    };
    Ok((spec, pairs))
}

/// One priced line, with the `SKU` name and tax-rate pairs the sale
/// needs for its `sale_line_tax` rows.
struct PricedLine {
    /// The draft line.
    draft: DraftLine,
    /// The `SKU` display name (for the receipt).
    name: String,
    /// `(tax_rate_id, rate_bp)` pairs, in group order (empty for
    /// exempt lines).
    rate_pairs: Vec<(String, i64)>,
}

/// Build the receipt document for a sale.
#[allow(clippy::too_many_arguments)]
fn build_print_doc(
    branch_code: &str,
    terminal_code: &str,
    receipt_no: &str,
    lines: &[PrintLine],
    subtotal_minor: i64,
    tax_minor: i64,
    total_minor: i64,
    tender_minor: i64,
    rounding_line_minor: i64,
) -> PrintDoc {
    let mut footer = vec!["Thank you for shopping".to_string()];
    if rounding_line_minor != 0 {
        footer.push(format!("Cash rounding: {rounding_line_minor}"));
    }
    PrintDoc {
        header: vec![
            "ShopCore".to_string(),
            format!("Branch {branch_code} · Till {terminal_code}"),
            format!("Receipt {receipt_no}"),
        ],
        lines: lines.to_vec(),
        subtotal_minor,
        tax_minor,
        total_minor,
        payments: vec![PrintPayment {
            method: "Cash".to_string(),
            amount_minor: tender_minor,
        }],
        footer,
        code_payload: Some(receipt_no.to_string()),
    }
}

/// The owned context for one sale run (loaded before the transaction,
/// moved into the actor closure).
struct SaleCtx {
    tenant_id: String,
    branch_id: String,
    terminal_id: String,
    branch_code: String,
    terminal_code: String,
    shift_id: String,
    cashier_id: String,
    draft_id: String,
    tender_minor: i64,
    now_ms: i64,
    date_yyyymmdd: String,
    currency_exponent: u32,
    cash_rounding: CashRounding,
    draft_lines: Vec<DraftLine>,
}

/// The sale id already committed for a `draft_id`, if any (the
/// idempotency pre-check — the transaction re-checks inside the lock).
fn find_existing_sale(
    db: &Db,
    tenant_id: &str,
    draft_id: &str,
) -> Result<Option<String>, SaleError> {
    let tenant_id = tenant_id.to_string();
    let draft_id = draft_id.to_string();
    Ok(db.read(move |conn| {
        conn.query_row(
            "SELECT id FROM sale WHERE tenant_id = ?1 AND draft_id = ?2 AND deleted_at IS NULL",
            rusqlite::params![tenant_id, draft_id],
            |row| row.get(0),
        )
        .optional()
        .map_err(shopcore_db::DbError::Sql)
    })?)
}

/// The tenant currency exponent and cash-rounding granularity
/// (defaults: exponent 2, rounding OFF).
fn load_sale_settings(db: &Db, tenant_id: &str) -> Result<(u32, CashRounding), SaleError> {
    let exponent = get_setting_i64(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
    )
    .map_err(SaleError::Core)?
    .unwrap_or(2);
    let currency_exponent = u32::try_from(exponent).unwrap_or(2);
    let granularity = get_setting_i64(
        db,
        tenant_id,
        SCOPE_TENANT,
        "",
        KEY_CASH_ROUNDING_GRANULARITY_MINOR,
    )
    .map_err(SaleError::Core)?
    .unwrap_or(0);
    let cash_rounding = if granularity <= 0 {
        CashRounding::Off
    } else {
        CashRounding::on(granularity).map_err(SaleError::Domain)?
    };
    Ok((currency_exponent, cash_rounding))
}

/// Complete a cash sale: one transaction (see the module docs).
/// Idempotent per `draft_id` — a second call returns the existing sale.
///
/// The caller must not mutate the draft while this runs (the UI
/// disables scanning during the commit); the draft is deleted and
/// tombstoned after a fresh commit.
///
/// # Errors
///
/// Returns [`SaleError::ShiftNotOpen`] when no shift is open,
/// [`SaleError::PermissionDenied`] when the cashier lacks `sale.create`,
/// [`SaleError::InsufficientTender`] when the tender is below the
/// (rounded) total, [`SaleError::InvalidInput`] for an empty or missing
/// draft or a `SKU` with no tax group, and [`SaleError::Db`] /
/// [`SaleError::Domain`] for database or pricing failures.
pub fn complete_sale_cash(
    db: &Db,
    scratch: &ScratchDb,
    input: CompleteSaleCashInput,
    faults: Option<&FaultInjector>,
) -> Result<SaleCompleted, SaleError> {
    // Pre-transaction reads (through the actor's read pool).
    let cashier = load_user_by_id(db, &input.tenant_id, &input.user_id)?;
    let role_id = cashier.role_id.clone().ok_or_else(|| {
        SaleError::PermissionDenied(format!("cashier {} has no role", cashier.username))
    })?;
    let role = get_role(db, &input.tenant_id, &role_id)?
        .ok_or_else(|| SaleError::PermissionDenied(format!("role {role_id} does not exist")))?;
    if !has_permission(&role, PERM_SALE_CREATE) {
        return Err(SaleError::PermissionDenied(PERM_SALE_CREATE.to_string()));
    }
    // Idempotent replay (BEFORE the draft load): the draft is DELETED
    // after a fresh commit, so a second call with the same `draft_id`
    // must NOT require the draft to still exist. When a sale already
    // exists for this draft, return it — the new call's tender is
    // ignored (the replay recomputes the change from the stored
    // payment). The transaction's own idempotency check is the second
    // line of defence (a race between this read and the commit).
    if let Some(sale_id) = find_existing_sale(db, &input.tenant_id, &input.draft_id)? {
        return replay_completed_sale(
            db,
            &input.tenant_id,
            &sale_id,
            &input.branch_code,
            &input.terminal_code,
        );
    }
    let (currency_exponent, cash_rounding) = load_sale_settings(db, &input.tenant_id)?;
    let draft = load_draft(scratch, &input.draft_id)?.ok_or_else(|| {
        SaleError::InvalidInput(format!("draft {} does not exist", input.draft_id))
    })?;
    if draft.lines.is_empty() {
        return Err(SaleError::InvalidInput("draft has no lines".to_string()));
    }
    let ctx = SaleCtx {
        tenant_id: input.tenant_id,
        branch_id: input.branch_id,
        terminal_id: input.terminal_id,
        branch_code: input.branch_code,
        terminal_code: input.terminal_code,
        shift_id: input.shift_id,
        cashier_id: input.user_id,
        draft_id: input.draft_id,
        tender_minor: input.tender_minor,
        now_ms: input.now_ms,
        date_yyyymmdd: input.date_yyyymmdd,
        currency_exponent,
        cash_rounding,
        draft_lines: draft.lines,
    };
    // The actor closure below MOVES `ctx` in, so the post-commit
    // cleanup keeps the two fields it needs up front.
    let cleanup_draft_id = ctx.draft_id.clone();
    let cleanup_now_ms = ctx.now_ms;
    // `Db::write` returns `()` — the outcome travels out through a
    // shared cell (the same pattern as `record_failed_attempt` in
    // `users.rs`). The closure returns `Ok` for business outcomes
    // (the cell carries them); only a `SQLite` failure is a `DbError`.
    // A business error drops the transaction uncommitted (rollback).
    let outcome = std::sync::Arc::new(std::sync::Mutex::new(
        None::<Result<SaleCompleted, SaleError>>,
    ));
    let outcome_in = std::sync::Arc::clone(&outcome);
    let faults = faults.copied();
    db.write(move |conn| {
        let result = run_sale_transaction(conn, &ctx, faults.as_ref());
        if let Ok(mut slot) = outcome_in.lock() {
            *slot = Some(result);
        }
        Ok(())
    })
    .map_err(SaleError::Db)?;
    let result = outcome
        .lock()
        .map_err(|_| SaleError::InvalidInput("sale outcome cell poisoned".to_string()))?
        .take()
        .ok_or_else(|| SaleError::InvalidInput("sale did not return an outcome".to_string()))?;
    // Fresh commit: tombstone the draft (so a late flush can never
    // resurrect it) and delete it. Best-effort — the main DB's sale row
    // is the authoritative backstop, and a replay skips this entirely.
    if let Ok(completed) = &result
        && completed.is_fresh
    {
        let _ = scratch.tombstone(&cleanup_draft_id, &completed.sale_id, cleanup_now_ms);
        let _ = scratch.with_conn(|conn| {
            conn.execute(
                "DELETE FROM draft_removed_line WHERE draft_sale_id = ?1",
                rusqlite::params![cleanup_draft_id],
            )?;
            conn.execute(
                "DELETE FROM draft_line WHERE draft_sale_id = ?1",
                rusqlite::params![cleanup_draft_id],
            )?;
            conn.execute(
                "DELETE FROM draft_sale WHERE id = ?1",
                rusqlite::params![cleanup_draft_id],
            )?;
            Ok(())
        });
    }
    result
}

/// Load a user by id (the `users` module only exposes lookup by
/// username; the sale needs the cashier by id).
fn load_user_by_id(
    db: &Db,
    tenant_id: &str,
    user_id: &str,
) -> Result<crate::users::User, SaleError> {
    let tenant_id = tenant_id.to_string();
    let user_id = user_id.to_string();
    let user: Option<crate::users::User> = db.read({
        let user_id = user_id.clone();
        move |conn| {
        conn.query_row(
            "SELECT id, tenant_id, username, role_id, failed_attempts, locked_until FROM user WHERE tenant_id = ?1 AND id = ?2 AND deleted_at IS NULL",
            rusqlite::params![tenant_id, user_id],
            |row| {
                Ok(crate::users::User {
                    id: row.get(0)?,
                    tenant_id: row.get(1)?,
                    username: row.get(2)?,
                    role_id: row.get(3)?,
                    failed_attempts: row.get(4)?,
                    locked_until_ms: row.get(5)?,
                })
            },
        )
        .optional().map_err(shopcore_db::DbError::Sql)
    }})?;
    user.ok_or_else(|| SaleError::NotFound(format!("user {user_id} does not exist")))
}

/// The sale transaction itself. Runs on the actor thread, inside
/// `Db::write`. Every step is fault-injectable (see `STEP_*`).
/// Runs one fault check (a `None` injector is a no-op).
fn fault_check(faults: Option<&FaultInjector>, step: &str) -> Result<(), SaleError> {
    match faults {
        Some(injector) => injector.check(step),
        None => Ok(()),
    }
}

/// A draft priced by the domain, plus the cash-rounding outcome —
/// carried between the pricing section and the write sections.
struct PricedSale {
    priced: Vec<PricedLine>,
    cart: CartResult,
    total_minor: i64,
    change_minor: i64,
    rounding_line_minor: i64,
}

fn run_sale_transaction(
    conn: &mut rusqlite::Connection,
    ctx: &SaleCtx,
    faults: Option<&FaultInjector>,
) -> Result<SaleCompleted, SaleError> {
    let tx = conn.transaction()?;
    // Idempotency: a sale already exists for this draft → return it
    // (no second sale, no second stock posting). The stored payment
    // holds the ORIGINAL tender, so the change is recomputed from the
    // stored rows, ignoring the new call's tender.
    fault_check(faults, STEP_IDEMPOTENCY)?;
    if let Some(existing) = load_completed_sale(&tx, ctx)? {
        return Ok(existing);
    }
    check_shift_open(&tx, ctx, faults)?;
    let priced = price_draft_lines(&tx, ctx, faults)?;
    let receipt_no = next_receipt_no(&tx, ctx, faults)?;
    let sale_id = insert_sale_row(&tx, ctx, &priced, &receipt_no, faults)?;
    let print_lines = insert_sale_lines(&tx, ctx, &sale_id, &priced, faults)?;
    insert_sale_payment(&tx, ctx, &sale_id, faults)?;
    let new_balances = post_sale_stock(&tx, ctx, &priced, &sale_id, faults)?;
    update_sale_aggregates(&tx, ctx, &priced, faults)?;
    write_sale_audit(&tx, ctx, &sale_id, &receipt_no, &priced, faults)?;
    write_sale_outbox(
        &tx,
        ctx,
        &sale_id,
        &receipt_no,
        &priced,
        &new_balances,
        faults,
    )?;
    let (print_job_id, print_doc) = queue_sale_print_job(
        &tx,
        ctx,
        &sale_id,
        &receipt_no,
        &print_lines,
        &priced,
        faults,
    )?;
    fault_check(faults, STEP_COMMIT)?;
    tx.commit()?;
    Ok(SaleCompleted {
        sale_id,
        receipt_no,
        total_minor: priced.total_minor,
        change_minor: priced.change_minor,
        rounding_line_minor: priced.rounding_line_minor,
        print_job_id,
        print_doc,
        receipt_queued: true,
        is_fresh: true,
    })
}

/// The sale commits only if a shift is open.
fn check_shift_open(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    faults: Option<&FaultInjector>,
) -> Result<(), SaleError> {
    fault_check(faults, STEP_SHIFT)?;
    let shift_open: bool = tx
        .query_row(
            "SELECT EXISTS(SELECT 1 FROM shift WHERE id = ?1 AND tenant_id = ?2 AND status = 'open' AND deleted_at IS NULL)",
            rusqlite::params![ctx.shift_id, ctx.tenant_id],
            |row| row.get(0),
        )?;
    if !shift_open {
        return Err(SaleError::ShiftNotOpen);
    }
    Ok(())
}

/// Price the cart with the 001a domain functions (never recomputed
/// here). Each line's tax spec comes from its `SKU`'s tax group.
/// Applies cash rounding (OFF by default) and computes the change.
fn price_draft_lines(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    faults: Option<&FaultInjector>,
) -> Result<PricedSale, SaleError> {
    fault_check(faults, STEP_PRICING)?;
    let mut priced: Vec<PricedLine> = Vec::with_capacity(ctx.draft_lines.len());
    let mut inputs: Vec<LineInput> = Vec::with_capacity(ctx.draft_lines.len());
    for line in &ctx.draft_lines {
        let (name, tax_group_id): (String, Option<String>) = tx
            .query_row(
                "SELECT name, tax_group_id FROM sku WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
                rusqlite::params![line.sku_id, ctx.tenant_id],
                |row| Ok((row.get(0)?, row.get(1)?)),
            )
            .optional()?
            .ok_or_else(|| SaleError::NotFound(format!("sku {} does not exist", line.sku_id)))?;
        let (tax, rate_pairs) = tax_spec_for_group(tx, &ctx.tenant_id, tax_group_id.as_deref())?;
        let line_discount = if line.discount_minor > 0 {
            LineDiscount::Fixed(Money::from_minor(line.discount_minor))
        } else {
            LineDiscount::None
        };
        inputs.push(LineInput {
            unit_price: UnitPrice::from_e4(line.unit_price_e4).map_err(SaleError::Domain)?,
            qty: Qty::from_milli(line.qty_milli).map_err(SaleError::Domain)?,
            line_discount,
            tax: tax.clone(),
        });
        priced.push(PricedLine {
            draft: line.clone(),
            name,
            rate_pairs,
        });
    }
    let cart: CartResult = price_cart(&inputs, &BillDiscount::None, ctx.currency_exponent)
        .map_err(SaleError::Domain)?;
    // Cash rounding (OFF by default): a separate untaxed line on the
    // cash-tendered total after tax. The sale stores the ROUNDED total;
    // the rounding line is derivable (`total − (subtotal − discount +
    // tax)`) and is also returned on the output.
    let rounded = ctx
        .cash_rounding
        .apply(cart.total)
        .map_err(SaleError::Domain)?;
    let total_minor = rounded.rounded_total.minor();
    let change = compute_change(rounded.rounded_total, Money::from_minor(ctx.tender_minor))
        .map_err(SaleError::Domain)?;
    Ok(PricedSale {
        priced,
        cart,
        total_minor,
        change_minor: change.minor(),
        rounding_line_minor: rounded.rounding_line.minor(),
    })
}

/// The receipt number: the sequence comes from `receipt_counter`,
/// updated INSIDE this transaction.
fn next_receipt_no(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    faults: Option<&FaultInjector>,
) -> Result<String, SaleError> {
    fault_check(faults, STEP_RECEIPT_COUNTER)?;
    let last_seq: i64 = tx
        .query_row(
            "SELECT last_seq FROM receipt_counter WHERE tenant_id = ?1 AND terminal_id = ?2 AND date = ?3",
            rusqlite::params![ctx.tenant_id, ctx.terminal_id, ctx.date_yyyymmdd],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let last_seq_u32 = u32::try_from(last_seq).unwrap_or(u32::MAX);
    let receipt_no = next_receipt_number(
        &ctx.branch_code,
        &ctx.terminal_code,
        &ctx.date_yyyymmdd,
        last_seq_u32,
    )
    .map_err(SaleError::Domain)?;
    let counter_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO receipt_counter (id, tenant_id, terminal_id, date, last_seq, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?6, 1) ON CONFLICT(tenant_id, terminal_id, date) DO UPDATE SET last_seq = excluded.last_seq, updated_at = excluded.updated_at, version = receipt_counter.version + 1",
        rusqlite::params![counter_id, ctx.tenant_id, ctx.terminal_id, ctx.date_yyyymmdd, last_seq + 1, ctx.now_ms],
    )?;
    Ok(receipt_no)
}

/// Inserts the `sale` row. Returns its id.
fn insert_sale_row(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    priced: &PricedSale,
    receipt_no: &str,
    faults: Option<&FaultInjector>,
) -> Result<String, SaleError> {
    fault_check(faults, STEP_SALE)?;
    let sale_id = uuid::Uuid::now_v7().to_string();
    let discount_minor = priced.cart.line_discounts.minor() + priced.cart.bill_discount.minor();
    tx.execute(
        "INSERT INTO sale (id, tenant_id, branch_id, terminal_id, shift_id, receipt_no, status, draft_id, subtotal_minor, discount_minor, tax_minor, total_minor, completed_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, 'completed', ?7, ?8, ?9, ?10, ?11, ?12, ?12, ?12, 1)",
        rusqlite::params![sale_id, ctx.tenant_id, ctx.branch_id, ctx.terminal_id, ctx.shift_id, receipt_no, ctx.draft_id, priced.cart.subtotal.minor(), discount_minor, priced.cart.tax.minor(), priced.total_minor, ctx.now_ms],
    )?;
    Ok(sale_id)
}

/// Inserts the `sale_line` rows and the per-component `sale_line_tax`
/// rows. Returns the receipt lines (for the [`PrintDoc`]).
fn insert_sale_lines(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    sale_id: &str,
    priced: &PricedSale,
    faults: Option<&FaultInjector>,
) -> Result<Vec<PrintLine>, SaleError> {
    fault_check(faults, STEP_LINES)?;
    let mut print_lines: Vec<PrintLine> = Vec::with_capacity(priced.priced.len());
    let mut line_ids: Vec<(String, usize)> = Vec::with_capacity(priced.priced.len());
    for (index, priced_line) in priced.priced.iter().enumerate() {
        let result = priced.cart.lines.get(index).ok_or_else(|| {
            SaleError::InvalidInput("priced line missing for a draft line".to_string())
        })?;
        let line_id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO sale_line (id, tenant_id, sale_id, sku_id, qty_milli, unit_price_e4, discount_minor, tax_minor, total_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?10, 1)",
            rusqlite::params![line_id, ctx.tenant_id, sale_id, priced_line.draft.sku_id, priced_line.draft.qty_milli, priced_line.draft.unit_price_e4, result.line_discount.minor() + result.bill_discount.minor(), result.tax.minor(), result.total.minor(), ctx.now_ms],
        )?;
        line_ids.push((line_id, index));
        print_lines.push(PrintLine {
            name: priced_line.name.clone(),
            qty_milli: priced_line.draft.qty_milli,
            unit_price_e4: priced_line.draft.unit_price_e4,
            line_total_minor: result.total.minor(),
        });
    }
    fault_check(faults, STEP_TAX_ROWS)?;
    for (line_id, index) in &line_ids {
        let result = priced.cart.lines.get(*index).ok_or_else(|| {
            SaleError::InvalidInput("priced line missing for a tax row".to_string())
        })?;
        let priced_line = priced.priced.get(*index).ok_or_else(|| {
            SaleError::InvalidInput("priced line missing for a tax row".to_string())
        })?;
        // One row per tax component, in group order; the components
        // sum exactly to the line tax (the domain guarantees it).
        for (component_index, component) in result.tax_components.iter().enumerate() {
            let Some((rate_id, rate_bp)) = priced_line.rate_pairs.get(component_index) else {
                return Err(SaleError::InvalidInput(
                    "tax component has no rate row".to_string(),
                ));
            };
            let tax_row_id = uuid::Uuid::now_v7().to_string();
            tx.execute(
                "INSERT INTO sale_line_tax (id, tenant_id, sale_line_id, tax_rate_id, tax_minor, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?7, 1)",
                rusqlite::params![tax_row_id, ctx.tenant_id, line_id, rate_id, component.minor(), rate_bp, ctx.now_ms],
            )?;
        }
    }
    Ok(print_lines)
}

/// Inserts the `sale_payment` row. The amount stored is the TENDER
/// (cash handed over); the change is `tender − total`, recomputed from
/// these two columns.
fn insert_sale_payment(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    sale_id: &str,
    faults: Option<&FaultInjector>,
) -> Result<(), SaleError> {
    fault_check(faults, STEP_PAYMENT)?;
    let payment_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO sale_payment (id, tenant_id, sale_id, method, amount_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, 'cash', ?4, ?5, ?5, 1)",
        rusqlite::params![payment_id, ctx.tenant_id, sale_id, ctx.tender_minor, ctx.now_ms],
    )?;
    Ok(())
}

/// Posts one stock movement per line (balances ONLY via the posting
/// function). The movement's cost is the current average cost (0 when
/// the `SKU` has never been stocked). Returns `(sku_id, new_balance)`
/// pairs.
fn post_sale_stock(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    priced: &PricedSale,
    sale_id: &str,
    faults: Option<&FaultInjector>,
) -> Result<Vec<(String, i64)>, SaleError> {
    fault_check(faults, STEP_STOCK)?;
    let mut new_balances: Vec<(String, i64)> = Vec::with_capacity(priced.priced.len());
    for priced_line in &priced.priced {
        let avg_cost: i64 = tx
            .query_row(
                "SELECT avg_cost_e4 FROM stock_balance WHERE tenant_id = ?1 AND branch_id = ?2 AND sku_id = ?3",
                rusqlite::params![ctx.tenant_id, ctx.branch_id, priced_line.draft.sku_id],
                |row| row.get(0),
            )
            .optional()?
            .unwrap_or(0);
        let new_balance = post_stock_movement(
            tx,
            &ctx.tenant_id,
            &ctx.branch_id,
            &priced_line.draft.sku_id,
            "sale",
            -priced_line.draft.qty_milli,
            avg_cost,
            "sale",
            sale_id,
            ctx.now_ms,
        )?;
        new_balances.push((priced_line.draft.sku_id.clone(), new_balance));
    }
    Ok(new_balances)
}

/// Updates `daily_sales_summary`, `daily_item_sales`, and
/// `daily_cash_summary` in the same transaction.
fn update_sale_aggregates(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    priced: &PricedSale,
    faults: Option<&FaultInjector>,
) -> Result<(), SaleError> {
    fault_check(faults, STEP_AGGREGATES)?;
    let discount_minor = priced.cart.line_discounts.minor() + priced.cart.bill_discount.minor();
    let agg_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO daily_sales_summary (id, tenant_id, branch_id, date, sale_count, subtotal_minor, discount_minor, tax_minor, total_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 1, ?5, ?6, ?7, ?8, ?9, ?9, 1) ON CONFLICT(tenant_id, branch_id, date) DO UPDATE SET sale_count = sale_count + 1, subtotal_minor = subtotal_minor + excluded.subtotal_minor, discount_minor = discount_minor + excluded.discount_minor, tax_minor = tax_minor + excluded.tax_minor, total_minor = total_minor + excluded.total_minor, updated_at = excluded.updated_at, version = daily_sales_summary.version + 1",
        rusqlite::params![agg_id, ctx.tenant_id, ctx.branch_id, ctx.date_yyyymmdd, priced.cart.subtotal.minor(), discount_minor, priced.cart.tax.minor(), priced.total_minor, ctx.now_ms],
    )?;
    for (index, priced_line) in priced.priced.iter().enumerate() {
        let result = priced.cart.lines.get(index).ok_or_else(|| {
            SaleError::InvalidInput("priced line missing for an aggregate".to_string())
        })?;
        let item_agg_id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO daily_item_sales (id, tenant_id, branch_id, sku_id, date, qty_milli, total_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1) ON CONFLICT(tenant_id, branch_id, sku_id, date) DO UPDATE SET qty_milli = qty_milli + excluded.qty_milli, total_minor = total_minor + excluded.total_minor, updated_at = excluded.updated_at, version = daily_item_sales.version + 1",
            rusqlite::params![item_agg_id, ctx.tenant_id, ctx.branch_id, priced_line.draft.sku_id, ctx.date_yyyymmdd, priced_line.draft.qty_milli, result.total.minor(), ctx.now_ms],
        )?;
    }
    let cash_agg_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO daily_cash_summary (id, tenant_id, branch_id, terminal_id, date, opening_minor, cash_sales_minor, cash_in_minor, cash_out_minor, expected_minor, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, 0, 0, ?6, ?7, ?7, 1) ON CONFLICT(tenant_id, branch_id, date) DO UPDATE SET cash_sales_minor = cash_sales_minor + excluded.cash_sales_minor, expected_minor = expected_minor + excluded.expected_minor, updated_at = excluded.updated_at, version = daily_cash_summary.version + 1",
        rusqlite::params![cash_agg_id, ctx.tenant_id, ctx.branch_id, ctx.terminal_id, ctx.date_yyyymmdd, priced.total_minor, ctx.now_ms],
    )?;
    Ok(())
}

/// Writes the chained `sale.completed` audit row (same transaction).
fn write_sale_audit(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    sale_id: &str,
    receipt_no: &str,
    priced: &PricedSale,
    faults: Option<&FaultInjector>,
) -> Result<(), SaleError> {
    fault_check(faults, STEP_AUDIT)?;
    let after_json = format!(
        "{{\"receipt_no\":\"{receipt_no}\",\"total_minor\":{},\"tender_minor\":{},\"change_minor\":{}}}",
        priced.total_minor, ctx.tender_minor, priced.change_minor
    );
    write_audit(
        tx,
        &AuditEvent {
            tenant_id: ctx.tenant_id.clone(),
            terminal_id: ctx.terminal_id.clone(),
            user_id: Some(ctx.cashier_id.clone()),
            action: AUDIT_SALE_COMPLETED.to_string(),
            entity: Some("sale".to_string()),
            entity_id: Some(sale_id.to_string()),
            before_json: Some(draft_summary_json(&ctx.draft_lines)),
            after_json: Some(after_json),
            reason: None,
            at_ms: ctx.now_ms,
        },
    )
    .map_err(SaleError::Core)?;
    Ok(())
}

/// Writes the outbox rows (same transaction): one for the sale, plus
/// one pending row per (branch, sku) whose stock moved. The outbox
/// table carries the spec-005 columns (owner decision 2026-11-11 —
/// the migration was rewritten): `branch_id`, `entity`, `stream`,
/// `payload`, `idempotency_key` (= the row's own `UUIDv7`). The sale
/// row goes on stream `sale`; each stock row goes on stream
/// `stock_movement` with the payload the server's apply reads
/// (`sku_id`, `qty_milli`, `occurred_at_ms`).
fn write_sale_outbox(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    sale_id: &str,
    receipt_no: &str,
    priced: &PricedSale,
    new_balances: &[(String, i64)],
    faults: Option<&FaultInjector>,
) -> Result<(), SaleError> {
    fault_check(faults, STEP_OUTBOX)?;
    let sale_payload = sale_payload_json(
        ctx,
        sale_id,
        receipt_no,
        &priced.cart,
        priced.total_minor,
        priced.change_minor,
    );
    let outbox_sale_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO outbox (id, tenant_id, branch_id, entity, stream, payload, idempotency_key, created_at, sent_at, attempts, updated_at, version) VALUES (?1, ?2, ?3, 'sale', 'sale', ?4, ?1, ?5, NULL, 0, ?5, 1)",
        rusqlite::params![outbox_sale_id, ctx.tenant_id, ctx.branch_id, sale_payload, ctx.now_ms],
    )?;
    // One pending row per (branch, sku) whose stock moved — a `SKU`
    // on two lines (different prices) still gets ONE stock row, with
    // the summed quantity delta and the final balance.
    let mut stock_rows: Vec<(String, i64, i64)> = Vec::new();
    for priced_line in &priced.priced {
        let delta = -priced_line.draft.qty_milli;
        let balance = new_balances
            .iter()
            .rev()
            .find(|(sku, _)| sku == &priced_line.draft.sku_id)
            .map_or(0, |(_, b)| *b);
        match stock_rows
            .iter_mut()
            .find(|(sku, _, _)| sku == &priced_line.draft.sku_id)
        {
            Some((_, qty, _)) => *qty += delta,
            None => stock_rows.push((priced_line.draft.sku_id.clone(), delta, balance)),
        }
    }
    for (sku_id, qty, balance) in &stock_rows {
        let stock_payload = format!(
            "{{\"sku_id\":\"{sku_id}\",\"qty_milli\":{qty},\"occurred_at_ms\":{},\"balance_minor\":{balance},\"sale_id\":\"{sale_id}\"}}",
            ctx.now_ms
        );
        let outbox_stock_id = uuid::Uuid::now_v7().to_string();
        tx.execute(
            "INSERT INTO outbox (id, tenant_id, branch_id, entity, stream, payload, idempotency_key, created_at, sent_at, attempts, updated_at, version) VALUES (?1, ?2, ?3, 'stock_movement', 'stock_movement', ?4, ?1, ?5, NULL, 0, ?5, 1)",
            rusqlite::params![outbox_stock_id, ctx.tenant_id, ctx.branch_id, stock_payload, ctx.now_ms],
        )?;
    }
    Ok(())
}

/// Queues the receipt `print_job` (the receipt is QUEUED here; the
/// caller prints it after the commit — hardware never blocks the
/// commit). Returns the print job id and the [`PrintDoc`].
fn queue_sale_print_job(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
    sale_id: &str,
    receipt_no: &str,
    print_lines: &[PrintLine],
    priced: &PricedSale,
    faults: Option<&FaultInjector>,
) -> Result<(String, PrintDoc), SaleError> {
    fault_check(faults, STEP_PRINT_JOB)?;
    let print_doc = build_print_doc(
        &ctx.branch_code,
        &ctx.terminal_code,
        receipt_no,
        print_lines,
        priced.cart.subtotal.minor(),
        priced.cart.tax.minor(),
        priced.total_minor,
        ctx.tender_minor,
        priced.rounding_line_minor,
    );
    let payload_json = serde_json::to_string(&print_doc)
        .map_err(|_| SaleError::InvalidInput("print document cannot be encoded".to_string()))?;
    let print_job_id = uuid::Uuid::now_v7().to_string();
    tx.execute(
        "INSERT INTO print_job (id, tenant_id, terminal_id, sale_id, payload_json, status, attempts, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, 'queued', 0, ?6, ?6, 1)",
        rusqlite::params![print_job_id, ctx.tenant_id, ctx.terminal_id, sale_id, payload_json, ctx.now_ms],
    )?;
    Ok((print_job_id, print_doc))
}
/// The draft summary carried in the sale audit row's `before_json`.
fn draft_summary_json(lines: &[DraftLine]) -> String {
    let mut out = String::from("{\"lines\":[");
    for (i, line) in lines.iter().enumerate() {
        if i > 0 {
            out.push(',');
        }
        let _ = write!(
            out,
            "{{\"sku_id\":\"{}\",\"qty_milli\":{},\"unit_price_e4\":{},\"discount_minor\":{}}}",
            line.sku_id, line.qty_milli, line.unit_price_e4, line.discount_minor
        );
    }
    out.push_str("]}");
    out
}

/// The sale payload for the outbox row (JSON; the sync worker pushes
/// it, and the server revalidates it with the same domain crate).
fn sale_payload_json(
    ctx: &SaleCtx,
    sale_id: &str,
    receipt_no: &str,
    cart: &CartResult,
    total_minor: i64,
    change_minor: i64,
) -> String {
    let mut lines = String::from("[");
    for (i, line) in ctx.draft_lines.iter().enumerate() {
        if i > 0 {
            lines.push(',');
        }
        let result = cart.lines.get(i);
        let (tax, total) = match result {
            Some(r) => (r.tax.minor(), r.total.minor()),
            None => (0, 0),
        };
        let _ = write!(
            lines,
            "{{\"sku_id\":\"{}\",\"qty_milli\":{},\"unit_price_e4\":{},\"discount_minor\":{},\"tax_minor\":{tax},\"total_minor\":{total}}}",
            line.sku_id, line.qty_milli, line.unit_price_e4, line.discount_minor
        );
    }
    lines.push(']');
    format!(
        "{{\"type\":\"sale\",\"sale_id\":\"{sale_id}\",\"receipt_no\":\"{receipt_no}\",\"branch_id\":\"{}\",\"terminal_id\":\"{}\",\"shift_id\":\"{}\",\"subtotal_minor\":{},\"discount_minor\":{},\"tax_minor\":{},\"total_minor\":{total_minor},\"tender_minor\":{},\"change_minor\":{change_minor},\"completed_at\":{},\"lines\":{lines}}}",
        ctx.branch_id,
        ctx.terminal_id,
        ctx.shift_id,
        cart.subtotal.minor(),
        cart.line_discounts.minor() + cart.bill_discount.minor(),
        cart.tax.minor(),
        ctx.tender_minor,
        ctx.now_ms
    )
}

/// The pre-transaction twin of [`load_completed_sale`]: rebuild a
/// completed sale's output from the actor's read pool (used by the
/// idempotent replay in [`complete_sale_cash`] — the draft is already
/// deleted by then, so the replay cannot go through the transaction).
/// `is_fresh` is always `false` here.
fn replay_completed_sale(
    db: &Db,
    tenant_id: &str,
    sale_id: &str,
    branch_code: &str,
    terminal_code: &str,
) -> Result<SaleCompleted, SaleError> {
    let tenant_id = tenant_id.to_string();
    let sale_id = sale_id.to_string();
    let row: Option<(String, i64, i64, i64, i64)> = db.read({
        let tenant_id = tenant_id.clone();
        let sale_id = sale_id.clone();
        move |conn| {
            conn.query_row(
                "SELECT receipt_no, subtotal_minor, discount_minor, tax_minor, total_minor FROM sale WHERE tenant_id = ?1 AND id = ?2 AND deleted_at IS NULL",
                rusqlite::params![tenant_id, sale_id],
                |row| {
                    Ok((
                        row.get(0)?,
                        row.get(1)?,
                        row.get(2)?,
                        row.get(3)?,
                        row.get(4)?,
                    ))
                },
            )
            .optional()
            .map_err(shopcore_db::DbError::Sql)
        }
    })?;
    let Some((receipt_no, subtotal, discount, tax, total)) = row else {
        return Err(SaleError::NotFound(format!(
            "sale {sale_id} does not exist"
        )));
    };
    let tender: i64 = db.read({
        let tenant_id = tenant_id.clone();
        let sale_id = sale_id.clone();
        move |conn| {
            conn.query_row(
                "SELECT amount_minor FROM sale_payment WHERE tenant_id = ?1 AND sale_id = ?2 AND method = 'cash' ORDER BY rowid ASC LIMIT 1",
                rusqlite::params![tenant_id, sale_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(shopcore_db::DbError::Sql)
        }
    })?
    .unwrap_or(0);
    let change_minor = tender - total;
    let rounding_line_minor = total - (subtotal - discount + tax);
    let print_lines = load_replay_print_lines(db, &tenant_id, &sale_id)?;
    let (print_job_id, receipt_queued): (String, bool) = db
        .read({
            let tenant_id = tenant_id.clone();
            let sale_id = sale_id.clone();
            move |conn| {
                conn.query_row(
                    "SELECT id, status FROM print_job WHERE tenant_id = ?1 AND sale_id = ?2 ORDER BY rowid DESC LIMIT 1",
                    rusqlite::params![tenant_id, sale_id],
                    |row| {
                        let id: String = row.get(0)?;
                        let status: String = row.get(1)?;
                        Ok((id, status == "queued"))
                    },
                )
                .optional()
                .map_err(shopcore_db::DbError::Sql)
            }
        })?
        .unwrap_or_default();
    let print_doc = build_print_doc(
        branch_code,
        terminal_code,
        &receipt_no,
        &print_lines,
        subtotal,
        tax,
        total,
        tender,
        rounding_line_minor,
    );
    Ok(SaleCompleted {
        sale_id,
        receipt_no,
        total_minor: total,
        change_minor,
        rounding_line_minor,
        print_job_id,
        print_doc,
        receipt_queued,
        is_fresh: false,
    })
}

/// The receipt lines of a completed sale (for the rebuilt
/// [`PrintDoc`]).
fn load_replay_print_lines(
    db: &Db,
    tenant_id: &str,
    sale_id: &str,
) -> Result<Vec<PrintLine>, SaleError> {
    let tenant_id = tenant_id.to_string();
    let sale_id = sale_id.to_string();
    Ok(db.read(move |conn| {
        let mut stmt = conn.prepare(
            "SELECT l.qty_milli, l.unit_price_e4, l.total_minor, s.name FROM sale_line l JOIN sku s ON s.id = l.sku_id WHERE l.tenant_id = ?1 AND l.sale_id = ?2 ORDER BY l.rowid ASC",
        )?;
        let line_rows = stmt.query_map(rusqlite::params![tenant_id, sale_id], |row| {
            Ok(PrintLine {
                name: row.get::<_, String>(3)?,
                qty_milli: row.get(0)?,
                unit_price_e4: row.get(1)?,
                line_total_minor: row.get(2)?,
            })
        })?;
        let mut out = Vec::new();
        for line_row in line_rows {
            out.push(line_row?);
        }
        Ok(out)
    })?)
}

/// Load an already-completed sale for a `draft_id` (idempotent replay).
/// Returns `None` when no sale exists for the draft.
fn load_completed_sale(
    tx: &Transaction<'_>,
    ctx: &SaleCtx,
) -> Result<Option<SaleCompleted>, SaleError> {
    let row: Option<(String, String, i64, i64, i64, i64)> = tx
        .query_row(
            "SELECT id, receipt_no, subtotal_minor, discount_minor, tax_minor, total_minor FROM sale WHERE tenant_id = ?1 AND draft_id = ?2 AND deleted_at IS NULL",
            rusqlite::params![ctx.tenant_id, ctx.draft_id],
            |row| {
                Ok((
                    row.get(0)?,
                    row.get(1)?,
                    row.get(2)?,
                    row.get(3)?,
                    row.get(4)?,
                    row.get(5)?,
                ))
            },
        )
        .optional()?;
    let Some((sale_id, receipt_no, subtotal, discount, tax, total)) = row else {
        return Ok(None);
    };
    let tender: i64 = tx
        .query_row(
            "SELECT amount_minor FROM sale_payment WHERE tenant_id = ?1 AND sale_id = ?2 AND method = 'cash' ORDER BY rowid ASC LIMIT 1",
            rusqlite::params![ctx.tenant_id, sale_id],
            |row| row.get(0),
        )
        .optional()?
        .unwrap_or(0);
    let change_minor = tender - total;
    let rounding_line_minor = total - (subtotal - discount + tax);
    let mut stmt = tx.prepare(
        "SELECT l.qty_milli, l.unit_price_e4, l.total_minor, s.name FROM sale_line l JOIN sku s ON s.id = l.sku_id WHERE l.tenant_id = ?1 AND l.sale_id = ?2 ORDER BY l.rowid ASC",
    )?;
    let line_rows = stmt.query_map(rusqlite::params![ctx.tenant_id, sale_id], |row| {
        Ok(PrintLine {
            name: row.get::<_, String>(3)?,
            qty_milli: row.get(0)?,
            unit_price_e4: row.get(1)?,
            line_total_minor: row.get(2)?,
        })
    })?;
    let mut print_lines = Vec::new();
    for line_row in line_rows {
        print_lines.push(line_row?);
    }
    let (print_job_id, receipt_queued): (String, bool) = tx
        .query_row(
            "SELECT id, status FROM print_job WHERE tenant_id = ?1 AND sale_id = ?2 ORDER BY rowid DESC LIMIT 1",
            rusqlite::params![ctx.tenant_id, sale_id],
            |row| {
                let id: String = row.get(0)?;
                let status: String = row.get(1)?;
                Ok((id, status == "queued"))
            },
        )
        .optional()?
        .unwrap_or_default();
    let print_doc = build_print_doc(
        &ctx.branch_code,
        &ctx.terminal_code,
        &receipt_no,
        &print_lines,
        subtotal,
        tax,
        total,
        tender,
        rounding_line_minor,
    );
    Ok(Some(SaleCompleted {
        sale_id,
        receipt_no,
        total_minor: total,
        change_minor,
        rounding_line_minor,
        print_job_id,
        print_doc,
        receipt_queued,
        is_fresh: false,
    }))
}

/// Attempt to print a queued receipt. On success the `print_job` is
/// marked `printed`; on [`HardwareError::Offline`] the job stays
/// `queued` and [`PrintStatus::Queued`] (`PRINT_QUEUED`) is returned —
/// the sale is unaffected either way.
///
/// Call from a blocking context (the `DB` actor calls block the
/// calling thread briefly); the printer itself is async.
///
/// # Errors
///
/// Returns [`SaleError::NotFound`] when the print job does not exist,
/// [`SaleError::InvalidInput`] when the stored document cannot be
/// decoded, and [`SaleError::Hardware`] for non-offline printer
/// failures.
pub async fn attempt_print<P: ReceiptPrinter>(
    db: &Db,
    print_job_id: &str,
    printer: &mut P,
) -> Result<PrintStatus, SaleError> {
    let print_job_id = print_job_id.to_string();
    let payload: Option<String> = db.read({
        let print_job_id = print_job_id.clone();
        move |conn| {
            conn.query_row(
                "SELECT payload_json FROM print_job WHERE id = ?1",
                rusqlite::params![print_job_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(shopcore_db::DbError::Sql)
        }
    })?;
    let Some(payload) = payload else {
        return Err(SaleError::NotFound(format!(
            "print job {print_job_id} does not exist"
        )));
    };
    let doc: PrintDoc = serde_json::from_str(&payload).map_err(|_| {
        SaleError::InvalidInput("stored print document is not valid JSON".to_string())
    })?;
    match printer.print(&doc).await {
        Ok(()) => {
            db.write(move |conn| {
                conn.execute(
                    "UPDATE print_job SET status = 'printed', attempts = attempts + 1 WHERE id = ?1",
                    rusqlite::params![print_job_id],
                )?;
                Ok(())
            })
            .map_err(SaleError::Db)?;
            Ok(PrintStatus::Printed)
        }
        Err(HardwareError::Offline(_)) => {
            db.write(move |conn| {
                conn.execute(
                    "UPDATE print_job SET attempts = attempts + 1 WHERE id = ?1",
                    rusqlite::params![print_job_id],
                )?;
                Ok(())
            })
            .map_err(SaleError::Db)?;
            Ok(PrintStatus::Queued)
        }
        Err(other) => Err(SaleError::Hardware(other)),
    }
}

/// Reprint a sale's receipt: attempts the print for the sale's latest
/// `print_job` and writes a `receipt.reprint` audit row (the reprint
/// is an audit event in this spec). Returns the print status.
///
/// # Errors
///
/// Same as [`attempt_print`], plus [`SaleError::Db`] if the audit
/// write fails.
pub async fn reprint_receipt<P: ReceiptPrinter>(
    db: &Db,
    sale_id: &str,
    user_id: &str,
    terminal_id: &str,
    printer: &mut P,
    now_ms: i64,
) -> Result<PrintStatus, SaleError> {
    let sale_id = sale_id.to_string();
    let job_id: Option<String> = db.read({
        let sale_id = sale_id.clone();
        move |conn| {
            conn.query_row(
                "SELECT id FROM print_job WHERE sale_id = ?1 ORDER BY rowid DESC LIMIT 1",
                rusqlite::params![sale_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(shopcore_db::DbError::Sql)
        }
    })?;
    let Some(job_id) = job_id else {
        return Err(SaleError::NotFound(format!(
            "no print job for sale {sale_id}"
        )));
    };
    let status = attempt_print(db, &job_id, printer).await?;
    let tenant_id: Option<String> = db.read({
        let sale_id = sale_id.clone();
        move |conn| {
            conn.query_row(
                "SELECT tenant_id FROM sale WHERE id = ?1",
                rusqlite::params![sale_id],
                |row| row.get(0),
            )
            .optional()
            .map_err(shopcore_db::DbError::Sql)
        }
    })?;
    let Some(tenant_id) = tenant_id else {
        return Err(SaleError::NotFound(format!(
            "sale {sale_id} does not exist"
        )));
    };
    crate::audit::write_audit_event(
        db,
        &AuditEvent {
            tenant_id,
            terminal_id: terminal_id.to_string(),
            user_id: Some(user_id.to_string()),
            action: AUDIT_RECEIPT_REPRINT.to_string(),
            entity: Some("sale".to_string()),
            entity_id: Some(sale_id),
            before_json: None,
            after_json: Some(format!(
                "{{\"print_job_id\":\"{job_id}\",\"status\":\"{}\"}}",
                status.code()
            )),
            reason: Some("receipt reprint requested".to_string()),
            at_ms: now_ms,
        },
    )
    .map_err(SaleError::Core)?;
    Ok(status)
}

/// Kick the cash drawer open after a cash sale. Best-effort: a drawer
/// failure never fails the sale (the caller logs it and moves on).
///
/// # Errors
///
/// Returns [`SaleError::Hardware`] when the drawer cannot be opened.
pub async fn kick_drawer<D: CashDrawer>(drawer: &mut D) -> Result<(), SaleError> {
    drawer.open().await.map_err(SaleError::Hardware)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn fault_injector_fails_and_sleeps() {
        let fail = FaultInjector::fail_at(STEP_SALE);
        assert!(
            matches!(
                fail.check(STEP_SALE),
                Err(SaleError::FaultInjected(STEP_SALE))
            ),
            "the injected step fails"
        );
        assert!(fail.check(STEP_LINES).is_ok(), "other steps pass");
        let sleep = FaultInjector::sleep_at(STEP_COMMIT, std::time::Duration::from_millis(1));
        assert!(sleep.check(STEP_COMMIT).is_ok(), "sleep passes");
    }

    #[test]
    fn sale_error_codes_are_stable() {
        assert_eq!(SaleError::ShiftNotOpen.code(), "SHIFT_NOT_OPEN");
        assert_eq!(
            SaleError::DiscountNeedsApproval.code(),
            "DISCOUNT_NEEDS_APPROVAL"
        );
        assert_eq!(SaleError::InsufficientTender.code(), "INSUFFICIENT_TENDER");
        assert_eq!(
            SaleError::BarcodeNotFound("123".to_string()).code(),
            "BARCODE_NOT_FOUND"
        );
        assert_eq!(PrintStatus::Queued.code(), "PRINT_QUEUED");
        assert_eq!(PrintStatus::Printed.code(), "PRINTED");
    }
}
