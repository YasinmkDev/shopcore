//! Tax-spec lookup shared by the sale path (spec 001b-2) and the cart
//! pricing path (spec 001c-1).
//!
//! Both need the same rule: a line's [`TaxSpec`] comes from its tax
//! group, with the mode (`price_includes_tax`) read from the data.
//! The lookup splits into a load step (needs a connection) and a pure
//! build step, so the cart path can load inside the `DB` actor's read
//! closure (which returns [`DbError`](shopcore_db::DbError)) and build
//! outside it.

use rusqlite::{Connection, OptionalExtension};
use shopcore_domain::{Rate, TaxSpec};

use crate::CoreError;

/// What the tax-spec build needs: the group's mode flag and its rate
/// rows, in group order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaxData {
    /// `true` when the group's shelf prices include tax.
    pub price_includes_tax: bool,
    /// `(tax_rate_id, rate_bp)` pairs, in group order.
    pub rate_pairs: Vec<(String, i64)>,
}

/// Load a group's tax data: the `price_includes_tax` flag and its
/// rate rows, ordered by creation (`created_at, id` — `UUIDv7` ids are
/// time-ordered, so the id breaks same-millisecond ties in creation
/// order).
///
/// # Errors
///
/// Returns [`CoreError::InvalidInput`] when the `SKU` has no tax group,
/// [`CoreError::TaxGroupNotFound`] when the group's row is missing or
/// soft-deleted (never a silent exclusive/zero-tax fallback), and
/// [`CoreError::Db`] if the read fails.
pub fn load_tax_data(
    conn: &Connection,
    tenant_id: &str,
    tax_group_id: Option<&str>,
) -> Result<TaxData, CoreError> {
    let Some(group_id) = tax_group_id else {
        return Err(CoreError::InvalidInput(
            "SKU has no tax group; tax cannot be computed".to_string(),
        ));
    };
    // The mode comes from the data (owner decision 2026-11-11). A
    // missing or deleted group row is an error, never a fallback: the
    // query cannot tell "never existed" from "soft-deleted", and both
    // mean the catalog is incomplete — pricing must fail loudly.
    let price_includes_tax: i64 = conn
        .query_row(
            "SELECT price_includes_tax FROM tax_group WHERE tenant_id = ?1 AND id = ?2 AND deleted_at IS NULL",
            rusqlite::params![tenant_id, group_id],
            |row| row.get(0),
        )
        .optional()?
        .ok_or_else(|| CoreError::TaxGroupNotFound(group_id.to_string()))?;
    let mut stmt = conn.prepare(
        "SELECT id, rate_bp FROM tax_rate WHERE tenant_id = ?1 AND tax_group_id = ?2 AND deleted_at IS NULL ORDER BY created_at, id",
    )?;
    let rows = stmt.query_map(rusqlite::params![tenant_id, group_id], |row| {
        Ok((row.get::<_, String>(0)?, row.get::<_, i64>(1)?))
    })?;
    let mut rate_pairs: Vec<(String, i64)> = Vec::new();
    for row in rows {
        rate_pairs.push(row?);
    }
    Ok(TaxData {
        price_includes_tax: price_includes_tax != 0,
        rate_pairs,
    })
}

/// Build a line's [`TaxSpec`] from loaded tax data, plus the rate
/// pairs for the `sale_line_tax` rows.
///
/// Rules: a group with NO rate rows, or with ALL rates zero, is
/// exempt (no `sale_line_tax` rows). Otherwise the rates apply per
/// the group's `price_includes_tax` flag (0 = exclusive, 1 =
/// inclusive) — the mode comes from the data, so a sale's lines may
/// mix modes.
///
/// # Errors
///
/// Returns [`CoreError::InvalidInput`] when a rate is out of range,
/// and [`CoreError::Domain`] when a rate cannot be parsed.
pub fn tax_spec_from_data(data: TaxData) -> Result<(TaxSpec, Vec<(String, i64)>), CoreError> {
    if data.rate_pairs.is_empty() {
        return Ok((
            TaxSpec::Exempt {
                reason: "no tax rates configured for group".to_string(),
            },
            Vec::new(),
        ));
    }
    if data.rate_pairs.iter().all(|(_, bp)| *bp == 0) {
        return Ok((
            TaxSpec::Exempt {
                reason: "zero-rated".to_string(),
            },
            Vec::new(),
        ));
    }
    let mut rates: Vec<Rate> = Vec::with_capacity(data.rate_pairs.len());
    for (_, bp) in &data.rate_pairs {
        let bp_u32 = u32::try_from(*bp)
            .map_err(|_| CoreError::InvalidInput(format!("tax rate {bp} bp is out of range")))?;
        rates.push(Rate::from_basis_points(bp_u32)?);
    }
    let spec = if data.price_includes_tax {
        TaxSpec::Inclusive { rates }
    } else {
        TaxSpec::Exclusive { rates }
    };
    Ok((spec, data.rate_pairs))
}

/// Build a line's [`TaxSpec`] from its tax group, plus the
/// `(tax_rate_id, rate_bp)` pairs (in group order) for the
/// `sale_line_tax` rows.
///
/// Rules: a `SKU` with NO tax group is an error, and a tax group
/// whose row is missing or deleted is an error
/// ([`CoreError::TaxGroupNotFound`]) — the catalog is incomplete and
/// tax cannot be computed without a group, and a `sale_line_tax` row
/// needs a `tax_rate_id`. A group that EXISTS with NO rate rows, or
/// with ALL rates zero, is exempt (no `sale_line_tax` rows) — that is
/// configured data, not a fallback. Otherwise the rates apply per the
/// group's `price_includes_tax` flag (0 = exclusive, 1 = inclusive) —
/// the mode comes from the data, so a sale's lines may mix modes.
///
/// Takes a [`Connection`] (not a transaction): the sale path passes
/// its transaction (which derefs to a connection) and the cart path
/// passes the read connection from the `DB` actor.
///
/// # Errors
///
/// Returns [`CoreError::InvalidInput`] when the `SKU` has no tax group
/// or a rate is out of range, [`CoreError::TaxGroupNotFound`] when the
/// group's row is missing or deleted, and [`CoreError::Db`] if the read
/// fails.
pub fn tax_spec_for_group(
    conn: &Connection,
    tenant_id: &str,
    tax_group_id: Option<&str>,
) -> Result<(TaxSpec, Vec<(String, i64)>), CoreError> {
    let data = load_tax_data(conn, tenant_id, tax_group_id)?;
    tax_spec_from_data(data)
}
