//! Checkout UI (spec 001c-1) request/response/event types.
//!
//! The DTOs the Tauri commands return and the events the core emits.
//! All integers travel as JSON numbers; the desktop command layer
//! range-checks every integer DTO to ≤ 2^53-1 (the safe-integer limit
//! for JSON numbers in JavaScript) — see the 001c-1 spec, amendment 3.
//! `ts-rs` types every `i64` here as `number`.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// One SKU found by search or barcode lookup, with its active price
/// (if a price list is configured and the SKU has a current entry).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SkuHitDto {
    /// The `SKU` id.
    pub sku_id: String,
    /// The parent `item` id.
    pub item_id: String,
    /// The `SKU` code.
    pub code: String,
    /// The `SKU` name.
    pub name: String,
    /// The tax group id (for the 001a tax lookup).
    pub tax_group_id: Option<String>,
    /// The active price in `e4` (price x 10^4), if one is configured.
    #[ts(type = "number | null")]
    pub price_e4: Option<i64>,
}

/// One line of the cart as the UI renders it: the draft line plus the
/// `SKU` name and the priced line total.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CartLineDto {
    /// Line id (`UUIDv7`).
    pub id: String,
    /// The `SKU` id.
    pub sku_id: String,
    /// The `SKU` display name.
    pub name: String,
    /// Quantity, milli-units.
    #[ts(type = "number")]
    pub qty_milli: i64,
    /// Unit price at scale 10^4 (locked at scan time).
    #[ts(type = "number")]
    pub unit_price_e4: i64,
    /// Line discount, minor units (0 = none).
    #[ts(type = "number")]
    pub discount_minor: i64,
    /// Priced line total (`net + tax`), minor units.
    #[ts(type = "number")]
    pub line_total_minor: i64,
}

/// The bill totals, all in minor units of the tenant currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CartTotalsDto {
    /// Sum of line grosses.
    #[ts(type = "number")]
    pub subtotal_minor: i64,
    /// Sum of line discounts.
    #[ts(type = "number")]
    pub discount_minor: i64,
    /// Sum of line nets after line discounts and bill-discount
    /// allocation — the pre-tax subtotal. `net_subtotal + tax ==
    /// total` in every tax mode (exclusive, inclusive, mixed); see
    /// the totals-display rule in `context/02-domain-model.md`.
    /// The UI never computes money: it displays this value as given.
    #[ts(type = "number")]
    pub net_subtotal_minor: i64,
    /// Sum of line taxes.
    #[ts(type = "number")]
    pub tax_minor: i64,
    /// Sum of line totals.
    #[ts(type = "number")]
    pub total_minor: i64,
}

/// The whole cart as the sell screen renders it: the open draft, its
/// priced lines, and the bill totals.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct CartViewDto {
    /// Draft id (`UUIDv7` — also the sale's `draft_id` when completed).
    pub draft_id: String,
    /// Priced lines, in insertion order.
    pub lines: Vec<CartLineDto>,
    /// Bill totals.
    pub totals: CartTotalsDto,
    /// The tenant currency's minor-unit exponent (2 for most
    /// currencies, 0 for JPY, 3 for KWD).
    pub currency_exponent: u32,
}

/// The actor the sell screen works as (spec 001c-1, amendment 4):
/// tenant, branch, terminal, cashier, and the open shift. In dev a
/// feature-gated auto-session fills it; real sign-in/lock is spec 001d.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SessionDto {
    /// The tenant.
    pub tenant_id: String,
    /// The branch id.
    pub branch_id: String,
    /// The branch code (e.g. `MAIN`).
    pub branch_code: String,
    /// The terminal id.
    pub terminal_id: String,
    /// The terminal code (e.g. `T1`).
    pub terminal_code: String,
    /// The cashier username, if one is signed in.
    pub username: Option<String>,
    /// The open shift id, if a shift is open.
    pub shift_id: Option<String>,
    /// The tenant currency code (e.g. `PKR`) — ADR-017.
    pub currency_code: String,
    /// The tenant currency's minor-unit exponent.
    pub currency_exponent: u32,
}

/// The till status the status bar renders (spec 001c-1, req. 7).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct StatusDto {
    /// Whether a shift is open on this terminal.
    pub shift_open: bool,
    /// Printer state: `"ready"` or `"offline"`.
    pub printer_status: String,
    /// Whether a sync server is configured. `false` until sync is
    /// actually configured — never a fake synced state.
    pub sync_configured: bool,
    /// True when this is a dev build (`dev-session` feature): the UI
    /// shows the "DEV BUILD — sample data" banner and the sample
    /// barcode list. Always `false` in a release build.
    pub dev_build: bool,
}

/// One sample barcode (dev builds only, 001c-1 follow-up 4): a row of
/// the dev seed catalog with its price and tax mode, for the DEV BUILD
/// banner's list. Prices are minor units of the tenant currency.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SampleBarcodeDto {
    /// The scannable barcode.
    pub barcode: String,
    /// The SKU name (e.g. `"Milk 1L"`).
    pub name: String,
    /// The shelf price in minor units (e.g. `25_000` = 250.00 PKR).
    #[ts(type = "number")]
    pub price_minor: i64,
    /// `"exclusive"`, `"inclusive"`, or `"zero"` (zero-rated).
    pub tax_mode: String,
    /// The tax rate in basis points (1800 = 18.00%).
    #[ts(type = "number")]
    pub rate_bp: i64,
}

/// `hardware.status` event payload (core → UI): one device's state.
/// First producer in 001c-1 is the desktop's mock printer status at
/// startup; real driver state changes land with spec 003.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct HardwareStatusPayload {
    /// The device (`"printer"`, `"scanner"`, `"drawer"`).
    pub device: String,
    /// The state (`"ready"`, `"offline"`).
    pub status: String,
}

/// `sale.completed` event payload (core → UI). The type lands in
/// 001c-1; the first emission lands in 001c-2 with
/// `complete_sale_cash` (no sale can complete before then).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SaleCompletedPayload {
    /// The sale id.
    pub sale_id: String,
    /// The receipt number.
    pub receipt_no: String,
    /// The sale total, minor units.
    #[ts(type = "number")]
    pub total_minor: i64,
}

/// One sold line on the receipt preview (spec 001c-2): the
/// `PrintDoc` line, rendered from the queued document only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReceiptLineDto {
    /// Item/SKU display name.
    pub name: String,
    /// Quantity, milli-units.
    #[ts(type = "number")]
    pub qty_milli: i64,
    /// Unit price at scale 10^4.
    #[ts(type = "number")]
    pub unit_price_e4: i64,
    /// Line total (`net + tax`), minor units.
    #[ts(type = "number")]
    pub line_total_minor: i64,
}

/// One payment on the receipt preview (spec 001c-2): the `PrintDoc`
/// payment, rendered from the queued document only.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReceiptPaymentDto {
    /// Payment method (`"Cash"` in 001c-2 — cash only).
    pub method: String,
    /// Amount tendered, minor units.
    #[ts(type = "number")]
    pub amount_minor: i64,
}

/// The receipt as the UI renders it (spec 001c-2 `ReceiptPreview`):
/// the queued `PrintDoc`, plus the change and the rounding line the
/// sale computed. The display rule (02) applies: the pre-tax
/// subtotal is `net_subtotal_minor + discount_minor`, so
/// `displayed subtotal − discounts + tax == total` in every tax
/// mode; the gross `subtotal_minor` is never displayed as the
/// subtotal line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReceiptDto {
    /// The receipt number (`{branch}-{terminal}-{yyyymmdd}-{seq}`).
    pub receipt_no: String,
    /// Header lines (shop name, branch, receipt number).
    pub header: Vec<String>,
    /// The sold lines.
    pub lines: Vec<ReceiptLineDto>,
    /// Sum of line grosses (shelf prices). Display-only context:
    /// for inclusive lines the gross already contains the tax, so
    /// this figure is NOT the displayed subtotal — see
    /// `net_subtotal_minor`.
    #[ts(type = "number")]
    pub subtotal_minor: i64,
    /// Sum of line nets after line discounts and bill-discount
    /// allocation (the domain's bill net).
    #[ts(type = "number")]
    pub net_subtotal_minor: i64,
    /// Sum of line + bill discounts, minor units.
    #[ts(type = "number")]
    pub discount_minor: i64,
    /// Total tax, minor units.
    #[ts(type = "number")]
    pub tax_minor: i64,
    /// Grand total, minor units.
    #[ts(type = "number")]
    pub total_minor: i64,
    /// Cash tendered, minor units.
    #[ts(type = "number")]
    pub tender_minor: i64,
    /// Change due (`tender − total`), minor units.
    #[ts(type = "number")]
    pub change_minor: i64,
    /// The separate untaxed rounding line (`rounded − total`, 0 when
    /// rounding is off).
    #[ts(type = "number")]
    pub rounding_line_minor: i64,
    /// How the sale was paid.
    pub payments: Vec<ReceiptPaymentDto>,
    /// Footer lines (thank-you, cash-rounding note when on).
    pub footer: Vec<String>,
}

/// The outcome of a receipt reprint (spec 001c-2): what
/// `reprint_receipt` returns, as the completion view renders it.
/// The reprint itself is an audit event (`receipt.reprint`, 07).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct ReprintDto {
    /// The sale id.
    pub sale_id: String,
    /// `true` once the print attempt succeeded; `false` while the
    /// receipt is still queued (printer offline).
    pub printed: bool,
}

/// A completed cash sale (spec 001c-2): what `complete_sale_cash`
/// returns, as the payment panel's completion view renders it. The
/// receipt is the queued `PrintDoc`; `receipt_queued` is `false`
/// once the print attempt after the commit succeeded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct SaleCompletedDto {
    /// The sale id.
    pub sale_id: String,
    /// The receipt number.
    pub receipt_no: String,
    /// The total actually charged (after cash rounding, if on).
    #[ts(type = "number")]
    pub total_minor: i64,
    /// The change due (`tender − total`).
    #[ts(type = "number")]
    pub change_minor: i64,
    /// The separate untaxed rounding line (`rounded − total`, 0 when
    /// rounding is off).
    #[ts(type = "number")]
    pub rounding_line_minor: i64,
    /// `true` until a print attempt succeeds (the receipt is queued).
    pub receipt_queued: bool,
    /// `true` when this call returned an already-completed sale
    /// (idempotent replay) instead of committing a new one.
    pub already_completed: bool,
    /// The receipt document for the preview.
    pub receipt: ReceiptDto,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn checkout_dtos_export_integers_as_number() {
        // Amendment 3: DTO integers are JSON numbers, typed `number`
        // in TypeScript (never strings, never `bigint` — the runtime
        // values are JSON numbers, range-checked to <= 2^53-1).
        let line =
            CartLineDto::export_to_string(&ts_rs::Config::default()).expect("export CartLineDto");
        assert!(
            line.contains("qty_milli: number"),
            "CartLineDto.qty_milli must export as number: {line}"
        );
        assert!(
            line.contains("unit_price_e4: number"),
            "CartLineDto.unit_price_e4 must export as number: {line}"
        );
        assert!(
            !line.contains("bigint"),
            "no i64 field may export as bigint: {line}"
        );
        let hit = SkuHitDto::export_to_string(&ts_rs::Config::default()).expect("export SkuHitDto");
        assert!(
            hit.contains("price_e4: number | null"),
            "SkuHitDto.price_e4 is Option<i64> — it must export as `number | null`, not `number`: {hit}"
        );
        let totals = CartTotalsDto::export_to_string(&ts_rs::Config::default())
            .expect("export CartTotalsDto");
        assert!(
            totals.contains("total_minor: number"),
            "CartTotalsDto.total_minor must export as number: {totals}"
        );
        assert!(
            totals.contains("net_subtotal_minor: number"),
            "CartTotalsDto.net_subtotal_minor must export as number: {totals}"
        );
        let view =
            CartViewDto::export_to_string(&ts_rs::Config::default()).expect("export CartViewDto");
        assert!(
            view.contains("currency_exponent: number"),
            "currency_exponent must export as number: {view}"
        );
        let completed = SaleCompletedDto::export_to_string(&ts_rs::Config::default())
            .expect("export SaleCompletedDto");
        assert!(
            completed.contains("total_minor: number"),
            "SaleCompletedDto.total_minor must export as number: {completed}"
        );
        assert!(
            completed.contains("change_minor: number"),
            "SaleCompletedDto.change_minor must export as number: {completed}"
        );
        assert!(
            completed.contains("receipt: ReceiptDto"),
            "SaleCompletedDto must carry the receipt preview: {completed}"
        );
        assert!(
            !completed.contains("bigint"),
            "no i64 field may export as bigint: {completed}"
        );
        let receipt =
            ReceiptDto::export_to_string(&ts_rs::Config::default()).expect("export ReceiptDto");
        assert!(
            receipt.contains("net_subtotal_minor: number"),
            "ReceiptDto.net_subtotal_minor must export as number: {receipt}"
        );
        assert!(
            receipt.contains("discount_minor: number"),
            "ReceiptDto.discount_minor must export as number: {receipt}"
        );
    }
}
