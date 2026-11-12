//! Cart pricing for the sell screen (spec 001c-1, req. 5).
//!
//! Totals recompute via the 001a domain functions after every change.
//! This module prices a draft cart WITHOUT committing anything: it
//! reads the `SKU` names + tax data from the main `DB`, builds the
//! domain `LineInput`s, and calls [`price_cart`] with no bill discount
//! (bill discounts land in 001c-2). The sale path (`sale.rs`) prices
//! the same way inside its transaction; the two must never disagree
//! about a cart's totals.

use rusqlite::OptionalExtension;
use shopcore_db::Db;
use shopcore_domain::{BillDiscount, LineDiscount, LineInput, Money, Qty, UnitPrice, price_cart};

use crate::CoreError;
use crate::draft::{DraftCart, DraftLine};
use crate::settings::{KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, get_setting_i64};
use crate::tax::{TaxData, load_tax_data, tax_spec_from_data};

/// One priced cart line: the draft line, its `SKU` name, and its
/// priced line total (`net + tax`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedCartLine {
    /// The draft line.
    pub line: DraftLine,
    /// The `SKU` display name.
    pub name: String,
    /// Priced line total (`net + tax`), minor units.
    pub line_total_minor: i64,
}

/// A priced cart: enriched lines + bill totals, all in minor units of
/// the tenant currency.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PricedCart {
    /// Priced lines, in draft order.
    pub lines: Vec<PricedCartLine>,
    /// Sum of line grosses.
    pub subtotal_minor: i64,
    /// Sum of line discounts.
    pub discount_minor: i64,
    /// Sum of line taxes.
    pub tax_minor: i64,
    /// Sum of line totals.
    pub total_minor: i64,
    /// The tenant currency's minor-unit exponent.
    pub currency_exponent: u32,
}

/// Price a draft cart: load each line's `SKU` name + tax data, build
/// the domain inputs, and price with [`price_cart`]. An empty cart
/// prices to all-zero totals.
///
/// # Errors
///
/// Returns [`CoreError::TaxGroupNotFound`] when a line's tax group
/// row is missing or deleted, [`CoreError::InvalidInput`] when a line
/// has no tax group or a stored discount exceeds its line,
/// [`CoreError::Domain`] on a pricing overflow, and [`CoreError::Db`]
/// if a read fails (including a draft line whose `SKU` does not
/// exist).
pub fn price_draft_cart(db: &Db, draft: &DraftCart) -> Result<PricedCart, CoreError> {
    let exponent = get_setting_i64(
        db,
        &draft.tenant_id,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
    )?
    .unwrap_or(2);
    let currency_exponent = u32::try_from(exponent).unwrap_or(2);

    // One read: the `SKU` name + tax data per line. The `DB` actor's
    // read closure returns `DbError`, so core errors from the load
    // step are mapped to `DbError::InvalidInput` with the message
    // preserved — EXCEPT a missing tax group, which maps to
    // `DbError::NotFound` so the code survives the round-trip (the
    // `SKU` query below only ever produces `InvalidInput`, so inside
    // this closure `NotFound` can only be the tax group; the mapping
    // back happens right after the `db.read`). The tax-spec build
    // runs outside the closure.
    let tenant_id = draft.tenant_id.clone();
    let draft_lines: Vec<DraftLine> = draft.lines.clone();
    let loaded: Vec<(DraftLine, String, TaxData)> = db
        .read(move |conn| {
            let mut out: Vec<(DraftLine, String, TaxData)> =
                Vec::with_capacity(draft_lines.len());
            for line in &draft_lines {
                let (name, tax_group_id): (String, Option<String>) = conn
                    .query_row(
                        "SELECT name, tax_group_id FROM sku WHERE id = ?1 AND tenant_id = ?2 AND deleted_at IS NULL",
                        rusqlite::params![line.sku_id, tenant_id],
                        |row| Ok((row.get(0)?, row.get(1)?)),
                    )
                    .optional()?
                    .ok_or_else(|| {
                        shopcore_db::DbError::InvalidInput(format!(
                            "sku {} does not exist",
                            line.sku_id
                        ))
                    })?;
                let tax_data = load_tax_data(conn, &tenant_id, tax_group_id.as_deref())
                    .map_err(|err| match err {
                        CoreError::TaxGroupNotFound(id) => {
                            shopcore_db::DbError::NotFound(id)
                        }
                        other => shopcore_db::DbError::InvalidInput(other.to_string()),
                    })?;
                out.push((line.clone(), name, tax_data));
            }
            Ok(out)
        })
        .map_err(|err| match err {
            shopcore_db::DbError::NotFound(id) => CoreError::TaxGroupNotFound(id),
            other => CoreError::Db(other),
        })?;

    let mut inputs: Vec<LineInput> = Vec::with_capacity(loaded.len());
    let mut priced_lines: Vec<PricedCartLine> = Vec::with_capacity(loaded.len());
    for (line, name, tax_data) in loaded {
        let (tax, _pairs) = tax_spec_from_data(tax_data)?;
        let line_discount = if line.discount_minor > 0 {
            LineDiscount::Fixed(Money::from_minor(line.discount_minor))
        } else {
            LineDiscount::None
        };
        inputs.push(LineInput {
            unit_price: UnitPrice::from_e4(line.unit_price_e4)?,
            qty: Qty::from_milli(line.qty_milli)?,
            line_discount,
            tax,
        });
        priced_lines.push(PricedCartLine {
            line,
            name,
            line_total_minor: 0, // filled from the priced result below
        });
    }
    let cart = price_cart(&inputs, &BillDiscount::None, currency_exponent)?;
    for (priced, result) in priced_lines.iter_mut().zip(cart.lines.iter()) {
        priced.line_total_minor = result.total.minor();
    }
    Ok(PricedCart {
        lines: priced_lines,
        subtotal_minor: cart.subtotal.minor(),
        // Bill discounts land in 001c-2; the bill discount here is
        // always `None`, so the discount total is the line discounts.
        discount_minor: cart.line_discounts.minor(),
        tax_minor: cart.tax.minor(),
        total_minor: cart.total.minor(),
        currency_exponent,
    })
}
