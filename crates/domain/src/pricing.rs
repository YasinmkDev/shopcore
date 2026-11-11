//! Line pricing, tax, and discounts — the checkout math.
//!
//! Implements the order of operations from `context/02-domain-model.md`:
//! (1) `unit_price × qty` → line gross (rounded to `Money`, via the `i128`
//! intermediate of `line_gross`); (2) line discount; (3) bill-discount
//! allocation (largest remainder); (4) tax on the discounted amount per
//! line; (5) bill tax = sum of line taxes, bill total = sum of line
//! totals, no bill-level re-rounding.
//!
//! Decisions applied here (spec 001a amendments, 2026-11-10):
//! - The `price_e4 × qty_milli` product uses an `i128` intermediate with
//!   a checked conversion back to `Money`.
//! - Multiple taxes round EACH component separately.
//! - Inclusive tax + discount: the discounted gross is the gross for the
//!   inclusive formula.
//! - A percent bill discount is a percent of the subtotal AFTER line
//!   discounts, rounded once, then largest-remainder allocated.
//!
//! Promotions are OUT of scope for this module (separate spec); nothing
//! here bakes in promotion hooks.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::money::Money;
use crate::price::UnitPrice;
use crate::qty::Qty;
use crate::rate::Rate;
use crate::rounding::{div_round_half_away, money_from_i128, ten_pow};

/// A discount applied to one line: none, a percent of the line gross, or
/// a fixed `Money` amount (which cannot exceed the line total).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum LineDiscount {
    /// No line discount.
    None,
    /// Percent of the line gross (`Rate` in basis points, at most 100%).
    Percent(Rate),
    /// Fixed amount; rejected when it exceeds the line total.
    Fixed(Money),
}

/// A discount applied to the whole bill: none, a percent of the subtotal
/// after line discounts, or a fixed `Money` amount (which cannot exceed
/// that subtotal).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum BillDiscount {
    /// No bill discount.
    None,
    /// Percent of the subtotal after line discounts, rounded once.
    Percent(Rate),
    /// Fixed amount; rejected when it exceeds the subtotal after line
    /// discounts.
    Fixed(Money),
}

/// How tax is computed for one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum TaxSpec {
    /// No tax, with the audit-visible reason (e.g. `"zero-rated food"`).
    Exempt {
        /// Why this line is tax-exempt.
        reason: String,
    },
    /// Tax added on top of the discounted amount: `tax = round(net ×
    /// rate)` per component, `total = net + tax`.
    Exclusive {
        /// One rate per tax component, in group order.
        rates: Vec<Rate>,
    },
    /// Tax included in the shelf price: `net = round(gross / (1 +
    /// rate))`, `tax = gross − net`, so net + tax always equals the
    /// (discounted) gross.
    Inclusive {
        /// One rate per tax component, in group order.
        rates: Vec<Rate>,
    },
}

/// One cart line as input to `price_cart`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineInput {
    /// Shelf price per unit, e4 fixed-point.
    pub unit_price: UnitPrice,
    /// Quantity, milli-units.
    pub qty: Qty,
    /// Line discount (percent or fixed).
    pub line_discount: LineDiscount,
    /// Tax spec (exclusive, inclusive, or exempt).
    pub tax: TaxSpec,
}

/// The tax split of one line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TaxBreakdown {
    /// Net of tax (exclusive: the discounted amount; inclusive: the
    /// gross stripped of tax).
    pub net: Money,
    /// Total tax for the line (sum of `components`).
    pub tax: Money,
    /// Per-component tax amounts, in the same order as the input rates.
    /// Empty for exempt lines.
    pub components: Vec<Money>,
    /// Line total (`net + tax`; for inclusive lines this equals the
    /// discounted gross).
    pub total: Money,
}

/// One priced cart line.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LineResult {
    /// Line gross (`unit_price × qty`, rounded to `Money`).
    pub gross: Money,
    /// Line discount applied.
    pub line_discount: Money,
    /// Bill discount allocated to this line.
    pub bill_discount: Money,
    /// Net of tax (see `TaxBreakdown::net`).
    pub net: Money,
    /// Total tax for the line.
    pub tax: Money,
    /// Per-component tax amounts, in the same order as the input rates.
    pub tax_components: Vec<Money>,
    /// Line total (`net + tax`).
    pub total: Money,
}

/// A priced cart.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct CartResult {
    /// Priced lines, in input order.
    pub lines: Vec<LineResult>,
    /// Sum of line grosses.
    pub subtotal: Money,
    /// Sum of line discounts.
    pub line_discounts: Money,
    /// Sum of allocated bill discounts.
    pub bill_discount: Money,
    /// Sum of line nets.
    pub net: Money,
    /// Sum of line taxes.
    pub tax: Money,
    /// Sum of line totals.
    pub total: Money,
}

/// Line gross: `unit_price × qty` converted to `Money`.
///
/// The product `price_e4 × qty_milli` is computed in `i128` (an `i64`
/// product would overflow for large prices/quantities), scaled by the
/// currency exponent, and rounded half away from zero exactly once, then
/// converted back to `Money` with a checked conversion.
///
/// # Errors
///
/// Returns `DomainError::Overflow` when the `i128` intermediate or the
/// final `Money` conversion overflows.
pub fn line_gross(
    price: UnitPrice,
    qty: Qty,
    currency_exponent: u32,
) -> Result<Money, DomainError> {
    // price_e4 (10^-4 major) × qty_milli (10^-3 units) = 10^-7 major;
    // × 10^exponent = minor units. One rounding, at the end.
    let scale = ten_pow(currency_exponent).ok_or(DomainError::Overflow)?;
    let numerator = i128::from(price.e4())
        .checked_mul(i128::from(qty.milli()))
        .and_then(|product| product.checked_mul(scale))
        .ok_or(DomainError::Overflow)?;
    let minor = div_round_half_away(numerator, 10_000_000).ok_or(DomainError::Overflow)?;
    money_from_i128(minor)
}

/// Line discount amount for a line gross.
///
/// A percent discount is `round(gross × rate)`; a fixed discount is the
/// amount itself, capped by the line total.
///
/// # Errors
///
/// Returns `DomainError::InvalidDiscount` for a negative fixed amount or
/// a negative gross, `DomainError::DiscountExceedsLine` when a fixed
/// discount exceeds the gross, `DomainError::Overflow` on arithmetic
/// overflow.
pub fn line_discount_amount(gross: Money, discount: &LineDiscount) -> Result<Money, DomainError> {
    if gross.is_negative() {
        return Err(DomainError::InvalidDiscount);
    }
    match discount {
        LineDiscount::None => Ok(Money::ZERO),
        LineDiscount::Percent(rate) => {
            let amount = div_round_half_away(
                i128::from(gross.minor()) * i128::from(rate.basis_points()),
                10_000,
            )
            .ok_or(DomainError::Overflow)?;
            money_from_i128(amount)
        }
        LineDiscount::Fixed(amount) => {
            if amount.is_negative() {
                return Err(DomainError::InvalidDiscount);
            }
            if *amount > gross {
                return Err(DomainError::DiscountExceedsLine);
            }
            Ok(*amount)
        }
    }
}

/// Bill discount total: a percent bill discount is a percent of the
/// subtotal AFTER line discounts, rounded ONCE; a fixed bill discount is
/// the amount itself, capped by that subtotal.
///
/// # Errors
///
/// Returns `DomainError::InvalidDiscount` for a negative fixed amount or
/// a negative subtotal, `DomainError::DiscountExceedsBill` when a fixed
/// discount exceeds the subtotal, `DomainError::Overflow` on arithmetic
/// overflow.
pub fn bill_discount_total(
    subtotal_after_line_discounts: Money,
    discount: &BillDiscount,
) -> Result<Money, DomainError> {
    if subtotal_after_line_discounts.is_negative() {
        return Err(DomainError::InvalidDiscount);
    }
    match discount {
        BillDiscount::None => Ok(Money::ZERO),
        BillDiscount::Percent(rate) => {
            let amount = div_round_half_away(
                i128::from(subtotal_after_line_discounts.minor()) * i128::from(rate.basis_points()),
                10_000,
            )
            .ok_or(DomainError::Overflow)?;
            money_from_i128(amount)
        }
        BillDiscount::Fixed(amount) => {
            if amount.is_negative() {
                return Err(DomainError::InvalidDiscount);
            }
            if *amount > subtotal_after_line_discounts {
                return Err(DomainError::DiscountExceedsBill);
            }
            Ok(*amount)
        }
    }
}

/// Allocate a bill discount across lines proportionally by
/// post-line-discount amount, using largest remainder: each line gets
/// `floor(total × weight / sum(weights))`, and the leftover minor units
/// go one each to the lines with the largest remainders (ties go to the
/// earlier line), so the allocations sum exactly to `total`.
///
/// Zero-value lines get 0; an all-zero cart allocates all 0 (never
/// divides by zero).
///
/// # Errors
///
/// Returns `DomainError::InvalidDiscount` for a negative total or any
/// negative weight, `DomainError::Overflow` on arithmetic overflow.
pub fn allocate_bill_discount(
    total: Money,
    post_line_amounts: &[Money],
) -> Result<Vec<Money>, DomainError> {
    if total.is_negative() || post_line_amounts.iter().any(|m| m.is_negative()) {
        return Err(DomainError::InvalidDiscount);
    }
    let weights: Vec<i64> = post_line_amounts.iter().map(|m| m.minor()).collect();
    allocate_largest_remainder(total.minor(), &weights)
        .map(|allocated| allocated.into_iter().map(Money::from_minor).collect())
}

/// Largest-remainder allocation over non-negative `i64` amounts.
///
/// Precondition (enforced by the public callers): `total >= 0` and every
/// weight `>= 0`. A violation is reported as `DomainError::Overflow`
/// rather than silently mis-allocating.
fn allocate_largest_remainder(total: i64, weights: &[i64]) -> Result<Vec<i64>, DomainError> {
    if total < 0 || weights.iter().any(|w| *w < 0) {
        return Err(DomainError::Overflow);
    }
    let mut out: Vec<i64> = Vec::with_capacity(weights.len());
    let weight_sum: i128 = weights.iter().map(|w| i128::from(*w)).sum();
    if total == 0 || weight_sum == 0 {
        out.resize(weights.len(), 0);
        return Ok(out);
    }
    let total_128 = i128::from(total);
    // (remainder, index) pairs for the leftover distribution.
    let mut remainders: Vec<(i128, usize)> = Vec::with_capacity(weights.len());
    let mut allocated_sum: i128 = 0;
    for (index, weight) in weights.iter().enumerate() {
        let share = total_128 * i128::from(*weight);
        let floor = share / weight_sum;
        // Remainder from the floor (one multiply, not a second division):
        // floor * weight_sum <= share by definition of floor division.
        let remainder = share - floor.checked_mul(weight_sum).ok_or(DomainError::Overflow)?;
        out.push(i64::try_from(floor).map_err(|_| DomainError::Overflow)?);
        remainders.push((remainder, index));
        allocated_sum += floor;
    }
    // 0 <= leftover < weights.len(): each floor is within one minor unit
    // of its exact share.
    let leftover = total_128 - allocated_sum;
    let extra = usize::try_from(leftover).map_err(|_| DomainError::Overflow)?;
    // Largest remainder first; ties go to the earlier line.
    remainders.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.cmp(&b.1)));
    for (_, index) in remainders.iter().take(extra) {
        let current = out.get_mut(*index).ok_or(DomainError::Overflow)?;
        *current = current.checked_add(1).ok_or(DomainError::Overflow)?;
    }
    Ok(out)
}

/// Tax for one line on its discounted amount.
///
/// - Exclusive: each component is `round(net × rate_i)` separately, and
///   the line tax is the sum of the rounded components.
/// - Inclusive: the net comes from the COMBINED rate (`net =
///   round(discounted × 10000 / (10000 + sum(rate_i)))`), so net + tax
///   always equals the discounted gross; the total tax is then split
///   across components proportionally by rate (largest remainder), so
///   the components sum exactly to the total tax.
/// - Exempt: tax 0, total = discounted amount.
///
/// # Errors
///
/// Returns `DomainError::Overflow` on arithmetic overflow, or when a
/// discounted amount is negative.
pub fn tax_for_line(discounted: Money, tax: &TaxSpec) -> Result<TaxBreakdown, DomainError> {
    if discounted.is_negative() {
        return Err(DomainError::Overflow);
    }
    match tax {
        TaxSpec::Exempt { .. } => Ok(TaxBreakdown {
            net: discounted,
            tax: Money::ZERO,
            components: Vec::new(),
            total: discounted,
        }),
        TaxSpec::Exclusive { rates } => {
            let mut components: Vec<Money> = Vec::with_capacity(rates.len());
            let mut tax_total = Money::ZERO;
            for rate in rates {
                let component = div_round_half_away(
                    i128::from(discounted.minor()) * i128::from(rate.basis_points()),
                    10_000,
                )
                .ok_or(DomainError::Overflow)
                .and_then(money_from_i128)?;
                components.push(component);
                tax_total = tax_total
                    .checked_add(component)
                    .ok_or(DomainError::Overflow)?;
            }
            let total = discounted
                .checked_add(tax_total)
                .ok_or(DomainError::Overflow)?;
            Ok(TaxBreakdown {
                net: discounted,
                tax: tax_total,
                components,
                total,
            })
        }
        TaxSpec::Inclusive { rates } => {
            let combined: i128 = rates.iter().map(|r| i128::from(r.basis_points())).sum();
            let net_minor =
                div_round_half_away(i128::from(discounted.minor()) * 10_000, 10_000 + combined)
                    .ok_or(DomainError::Overflow)?;
            let net = money_from_i128(net_minor)?;
            let total_tax = discounted.checked_sub(net).ok_or(DomainError::Overflow)?;
            // Split the total tax across components by rate so the
            // components sum exactly to the total tax.
            let rate_weights: Vec<i64> =
                rates.iter().map(|r| i64::from(r.basis_points())).collect();
            let component_minors = allocate_largest_remainder(total_tax.minor(), &rate_weights)?;
            let components: Vec<Money> = component_minors
                .into_iter()
                .map(Money::from_minor)
                .collect();
            Ok(TaxBreakdown {
                net,
                tax: total_tax,
                components,
                total: discounted,
            })
        }
    }
}

/// Price a whole cart following the `02` order of operations.
///
/// `currency_exponent` is the tenant currency's minor-unit exponent
/// (2 for most currencies, 0 for JPY, 3 for KWD).
///
/// # Errors
///
/// Returns the first `DomainError` from any line: overflow, a line
/// discount exceeding its line, a bill discount exceeding the subtotal
/// after line discounts, or an invalid discount input.
pub fn price_cart(
    lines: &[LineInput],
    bill_discount: &BillDiscount,
    currency_exponent: u32,
) -> Result<CartResult, DomainError> {
    // Steps 1–2: gross + line discount per line.
    let mut grosses: Vec<Money> = Vec::with_capacity(lines.len());
    let mut line_discounts: Vec<Money> = Vec::with_capacity(lines.len());
    let mut post_line: Vec<Money> = Vec::with_capacity(lines.len());
    let mut gross_subtotal = Money::ZERO;
    let mut post_line_subtotal = Money::ZERO;
    for line in lines {
        let gross = line_gross(line.unit_price, line.qty, currency_exponent)?;
        let discount = line_discount_amount(gross, &line.line_discount)?;
        let post = gross.checked_sub(discount).ok_or(DomainError::Overflow)?;
        grosses.push(gross);
        line_discounts.push(discount);
        post_line.push(post);
        gross_subtotal = gross_subtotal
            .checked_add(gross)
            .ok_or(DomainError::Overflow)?;
        post_line_subtotal = post_line_subtotal
            .checked_add(post)
            .ok_or(DomainError::Overflow)?;
    }
    // Step 3: bill discount total (rounded once) + largest-remainder
    // allocation by post-line-discount amount.
    let bill_total = bill_discount_total(post_line_subtotal, bill_discount)?;
    let allocations = allocate_bill_discount(bill_total, &post_line)?;
    // Steps 4–5: tax per line, then bill sums (no bill-level re-rounding).
    let mut results: Vec<LineResult> = Vec::with_capacity(lines.len());
    let mut bill_net = Money::ZERO;
    let mut bill_tax = Money::ZERO;
    let mut bill_total_amount = Money::ZERO;
    let mut line_discount_sum = Money::ZERO;
    for (index, line) in lines.iter().enumerate() {
        let allocation = allocations
            .get(index)
            .copied()
            .ok_or(DomainError::Overflow)?;
        let post = post_line.get(index).copied().ok_or(DomainError::Overflow)?;
        let discounted = post.checked_sub(allocation).ok_or(DomainError::Overflow)?;
        let breakdown = tax_for_line(discounted, &line.tax)?;
        let gross = grosses.get(index).copied().ok_or(DomainError::Overflow)?;
        let line_discount = line_discounts
            .get(index)
            .copied()
            .ok_or(DomainError::Overflow)?;
        bill_net = bill_net
            .checked_add(breakdown.net)
            .ok_or(DomainError::Overflow)?;
        bill_tax = bill_tax
            .checked_add(breakdown.tax)
            .ok_or(DomainError::Overflow)?;
        bill_total_amount = bill_total_amount
            .checked_add(breakdown.total)
            .ok_or(DomainError::Overflow)?;
        line_discount_sum = line_discount_sum
            .checked_add(line_discount)
            .ok_or(DomainError::Overflow)?;
        results.push(LineResult {
            gross,
            line_discount,
            bill_discount: allocation,
            net: breakdown.net,
            tax: breakdown.tax,
            tax_components: breakdown.components,
            total: breakdown.total,
        });
    }
    Ok(CartResult {
        lines: results,
        subtotal: gross_subtotal,
        line_discounts: line_discount_sum,
        bill_discount: bill_total,
        net: bill_net,
        tax: bill_tax,
        total: bill_total_amount,
    })
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::cast_possible_wrap)]
mod tests {
    use super::*;
    use crate::cash::compute_change;

    fn price(major: i64) -> UnitPrice {
        // `major` whole units -> e4.
        UnitPrice::from_e4(major * 10_000).expect("test price fits")
    }

    fn qty_whole(whole: i64) -> Qty {
        Qty::from_whole(whole).expect("test qty fits")
    }

    fn rate(bp: u32) -> Rate {
        Rate::from_basis_points(bp).expect("test rate in range")
    }

    fn money(minor: i64) -> Money {
        Money::from_minor(minor)
    }

    fn exclusive(rates: &[u32]) -> TaxSpec {
        TaxSpec::Exclusive {
            rates: rates.iter().map(|bp| rate(*bp)).collect(),
        }
    }

    fn inclusive(rates: &[u32]) -> TaxSpec {
        TaxSpec::Inclusive {
            rates: rates.iter().map(|bp| rate(*bp)).collect(),
        }
    }

    #[test]
    fn line_gross_basic() {
        assert_eq!(line_gross(price(100), qty_whole(1), 2), Ok(money(10_000)));
        assert_eq!(
            line_gross(
                UnitPrice::from_e4(100_000).expect("10.00"),
                Qty::from_milli(2500).expect("2.5"),
                2
            ),
            Ok(money(2500))
        );
        assert_eq!(line_gross(price(0), qty_whole(5), 2), Ok(money(0)));
        // Exponent 0 (JPY): 100 major -> 100 minor.
        assert_eq!(line_gross(price(100), qty_whole(1), 0), Ok(money(100)));
        // Exponent 3 (KWD): 100 major -> 100_000 minor.
        assert_eq!(line_gross(price(100), qty_whole(1), 3), Ok(money(100_000)));
    }

    #[test]
    fn line_gross_i128_overflow_boundary() {
        // price_e4 = i64::MAX with qty 1000 units: the i64 product would
        // overflow, the i128 product fits, but the final Money does not.
        assert_eq!(
            line_gross(
                UnitPrice::from_e4(i64::MAX).expect("max price"),
                qty_whole(1000),
                2
            ),
            Err(DomainError::Overflow)
        );
        // i128 itself overflows: i64::MAX x i64::MAX x 100 > i128::MAX.
        assert_eq!(
            line_gross(
                UnitPrice::from_e4(i64::MAX).expect("max price"),
                Qty::from_milli(i64::MAX).expect("max qty"),
                2
            ),
            Err(DomainError::Overflow)
        );
        // Just inside the boundary: a huge but representable gross.
        // 100_000.00 major x 1000 units = 100_000_000.00 major
        // = 10_000_000_000 minor.
        let big = line_gross(
            UnitPrice::from_e4(1_000_000_000).expect("100000.00"),
            Qty::from_milli(1_000_000).expect("1000 units"),
            2,
        );
        assert_eq!(big, Ok(money(10_000_000_000)));
    }

    #[test]
    fn line_discount_amount_rules() {
        assert_eq!(
            line_discount_amount(money(10_000), &LineDiscount::Percent(rate(1000))),
            Ok(money(1000))
        );
        assert_eq!(
            line_discount_amount(money(10_000), &LineDiscount::Fixed(money(2500))),
            Ok(money(2500))
        );
        assert_eq!(
            line_discount_amount(money(10_000), &LineDiscount::Fixed(money(10_001))),
            Err(DomainError::DiscountExceedsLine)
        );
        assert_eq!(
            line_discount_amount(money(10_000), &LineDiscount::Fixed(money(-1))),
            Err(DomainError::InvalidDiscount)
        );
        // Percent rounds once: 9999 x 10% = 999.9 -> 1000.
        assert_eq!(
            line_discount_amount(money(9999), &LineDiscount::Percent(rate(1000))),
            Ok(money(1000))
        );
    }

    #[test]
    fn bill_discount_total_rules() {
        assert_eq!(
            bill_discount_total(money(15_000), &BillDiscount::Percent(rate(1000))),
            Ok(money(1500))
        );
        assert_eq!(
            bill_discount_total(money(15_000), &BillDiscount::Fixed(money(15_001))),
            Err(DomainError::DiscountExceedsBill)
        );
        assert_eq!(
            bill_discount_total(money(0), &BillDiscount::Percent(rate(1000))),
            Ok(money(0))
        );
    }

    #[test]
    fn allocate_bill_discount_largest_remainder() {
        let weights = [money(10_000), money(10_000), money(10_000)];
        assert_eq!(
            allocate_bill_discount(money(100), &weights),
            Ok(vec![money(34), money(33), money(33)])
        );
        // Zero-value lines get 0; the allocation still sums exactly.
        let weights = [money(10_000), money(0)];
        assert_eq!(
            allocate_bill_discount(money(1000), &weights),
            Ok(vec![money(1000), money(0)])
        );
        // All-zero cart allocates all 0, never divides by zero.
        let weights = [money(0), money(0)];
        assert_eq!(
            allocate_bill_discount(money(0), &weights),
            Ok(vec![money(0), money(0)])
        );
        // Empty cart.
        assert_eq!(allocate_bill_discount(money(0), &[]), Ok(Vec::new()));
    }

    #[test]
    fn tax_for_line_exclusive_multi_component() {
        // Golden: net 0.30 with 5% + 13% -> 0.02 + 0.04 = 0.06, NOT 0.05.
        let breakdown = tax_for_line(money(30), &exclusive(&[500, 1300])).expect("tax computes");
        assert_eq!(breakdown.tax, money(6));
        assert_eq!(breakdown.components, vec![money(2), money(4)]);
        assert_eq!(breakdown.net, money(30));
        assert_eq!(breakdown.total, money(36));
    }

    #[test]
    fn tax_for_line_inclusive_and_exempt() {
        let breakdown = tax_for_line(money(11_800), &inclusive(&[1800])).expect("tax computes");
        assert_eq!(breakdown.net, money(10_000));
        assert_eq!(breakdown.tax, money(1800));
        assert_eq!(breakdown.total, money(11_800));
        let breakdown = tax_for_line(money(9999), &inclusive(&[1800])).expect("tax computes");
        assert_eq!(breakdown.net, money(8474));
        assert_eq!(breakdown.tax, money(1525));
        let breakdown = tax_for_line(
            money(10_000),
            &TaxSpec::Exempt {
                reason: "zero-rated food".to_string(),
            },
        )
        .expect("tax computes");
        assert_eq!(breakdown.tax, Money::ZERO);
        assert_eq!(breakdown.components, Vec::<Money>::new());
        assert_eq!(breakdown.total, money(10_000));
    }

    #[test]
    fn tax_for_line_inclusive_multi_rate_split_sums_exactly() {
        // 118.00 inclusive @5%+13%: net 100.00, tax 18.00 split 5.00/13.00.
        let breakdown =
            tax_for_line(money(11_800), &inclusive(&[500, 1300])).expect("tax computes");
        assert_eq!(breakdown.net, money(10_000));
        assert_eq!(breakdown.tax, money(1800));
        assert_eq!(breakdown.components, vec![money(500), money(1300)]);
        assert_eq!(breakdown.total, money(11_800));
    }

    #[test]
    fn price_cart_rejects_discount_exceeding_line() {
        let lines = [LineInput {
            unit_price: price(100),
            qty: qty_whole(1),
            line_discount: LineDiscount::Fixed(money(10_001)),
            tax: exclusive(&[1800]),
        }];
        assert_eq!(
            price_cart(&lines, &BillDiscount::None, 2),
            Err(DomainError::DiscountExceedsLine)
        );
    }

    // ---- Golden dataset -------------------------------------------------

    #[derive(serde::Deserialize)]
    struct GoldenFile {
        currency_exponent: u32,
        cart_cases: Vec<GoldenCartCase>,
    }

    #[derive(serde::Deserialize)]
    struct GoldenCartCase {
        name: String,
        lines: Vec<GoldenLine>,
        bill_discount: Option<GoldenDiscount>,
        expected: GoldenCartExpected,
    }

    #[derive(serde::Deserialize)]
    struct GoldenLine {
        price_e4: i64,
        qty_milli: i64,
        line_discount: Option<GoldenDiscount>,
        tax: GoldenTax,
    }

    #[derive(serde::Deserialize)]
    #[serde(tag = "kind")]
    enum GoldenDiscount {
        #[serde(rename = "percent")]
        Percent { rate_bp: u32 },
        #[serde(rename = "fixed")]
        Fixed { minor: i64 },
    }

    #[derive(serde::Deserialize)]
    #[serde(tag = "kind")]
    enum GoldenTax {
        #[serde(rename = "exclusive")]
        Exclusive { rates_bp: Vec<u32> },
        #[serde(rename = "inclusive")]
        Inclusive { rates_bp: Vec<u32> },
        #[serde(rename = "exempt")]
        Exempt { reason: String },
    }

    #[derive(serde::Deserialize)]
    struct GoldenCartExpected {
        lines: Vec<GoldenLineExpected>,
        subtotal: i64,
        line_discounts: i64,
        bill_discount: i64,
        net: i64,
        tax: i64,
        total: i64,
    }

    #[derive(serde::Deserialize)]
    struct GoldenLineExpected {
        gross: i64,
        line_discount: i64,
        bill_discount: i64,
        net: i64,
        tax: i64,
        tax_components: Vec<i64>,
        total: i64,
    }

    fn golden_discount_to_line(d: &GoldenDiscount) -> LineDiscount {
        match d {
            GoldenDiscount::Percent { rate_bp } => {
                LineDiscount::Percent(Rate::from_basis_points(*rate_bp).expect("golden rate"))
            }
            GoldenDiscount::Fixed { minor } => LineDiscount::Fixed(Money::from_minor(*minor)),
        }
    }

    fn golden_discount_to_bill(d: &GoldenDiscount) -> BillDiscount {
        match d {
            GoldenDiscount::Percent { rate_bp } => {
                BillDiscount::Percent(Rate::from_basis_points(*rate_bp).expect("golden rate"))
            }
            GoldenDiscount::Fixed { minor } => BillDiscount::Fixed(Money::from_minor(*minor)),
        }
    }

    fn golden_tax_to_spec(t: &GoldenTax) -> TaxSpec {
        match t {
            GoldenTax::Exclusive { rates_bp } => TaxSpec::Exclusive {
                rates: rates_bp
                    .iter()
                    .map(|bp| Rate::from_basis_points(*bp).expect("golden rate"))
                    .collect(),
            },
            GoldenTax::Inclusive { rates_bp } => TaxSpec::Inclusive {
                rates: rates_bp
                    .iter()
                    .map(|bp| Rate::from_basis_points(*bp).expect("golden rate"))
                    .collect(),
            },
            GoldenTax::Exempt { reason } => TaxSpec::Exempt {
                reason: reason.clone(),
            },
        }
    }

    #[test]
    fn golden_cart_cases() {
        let file: GoldenFile =
            serde_json::from_str(include_str!("../../../tests/golden/001a-checkout.json"))
                .expect("golden file parses");
        assert_eq!(file.currency_exponent, 2, "golden file uses exponent 2");
        assert!(
            !file.cart_cases.is_empty(),
            "golden file must contain cart cases"
        );
        for case in &file.cart_cases {
            let lines: Vec<LineInput> = case
                .lines
                .iter()
                .map(|l| LineInput {
                    unit_price: UnitPrice::from_e4(l.price_e4).expect("golden price"),
                    qty: Qty::from_milli(l.qty_milli).expect("golden qty"),
                    line_discount: l
                        .line_discount
                        .as_ref()
                        .map_or(LineDiscount::None, golden_discount_to_line),
                    tax: golden_tax_to_spec(&l.tax),
                })
                .collect();
            let bill_discount = case
                .bill_discount
                .as_ref()
                .map_or(BillDiscount::None, golden_discount_to_bill);
            let computed = price_cart(&lines, &bill_discount, file.currency_exponent)
                .expect("golden case prices");
            let expected_lines: Vec<LineResult> = case
                .expected
                .lines
                .iter()
                .map(|l| LineResult {
                    gross: money(l.gross),
                    line_discount: money(l.line_discount),
                    bill_discount: money(l.bill_discount),
                    net: money(l.net),
                    tax: money(l.tax),
                    tax_components: l.tax_components.iter().map(|m| money(*m)).collect(),
                    total: money(l.total),
                })
                .collect();
            let expected = CartResult {
                lines: expected_lines,
                subtotal: money(case.expected.subtotal),
                line_discounts: money(case.expected.line_discounts),
                bill_discount: money(case.expected.bill_discount),
                net: money(case.expected.net),
                tax: money(case.expected.tax),
                total: money(case.expected.total),
            };
            assert_eq!(computed, expected, "golden case failed: {}", case.name);
        }
    }

    // ---- Property tests (fixed-seed LCG, >= 10,000 cases) -------------

    /// Deterministic pseudo-random generator (same LCG constants as
    /// `Money`): fixed seed, so a failing case is reproducible from the
    /// seed printed in the assertion message.
    struct Lcg(u64);

    impl Lcg {
        fn new(seed: u64) -> Self {
            Self(seed)
        }

        fn next_u64(&mut self) -> u64 {
            self.0 = self
                .0
                .wrapping_mul(6_364_136_223_846_793_005)
                .wrapping_add(1_442_695_040_888_963_407);
            self.0
        }

        /// Value in `[0, bound)`. Callers always pass `bound > 0`.
        fn below(&mut self, bound: u64) -> u64 {
            self.next_u64() % bound
        }

        /// Value in `[0, bound)` as `u32`.
        fn below_u32(&mut self, bound: u32) -> u32 {
            u32::try_from(self.next_u64() % u64::from(bound)).expect("bounded value fits u32")
        }

        /// Value in `[0, bound)` as `usize`.
        fn below_usize(&mut self, bound: usize) -> usize {
            usize::try_from(self.next_u64() % bound as u64).expect("bounded value fits usize")
        }
    }

    struct RandomCart {
        lines: Vec<LineInput>,
        bill_discount: BillDiscount,
    }

    fn random_rates(rng: &mut Lcg) -> Vec<Rate> {
        let n = rng.below_usize(4); // 0..=3 components
        (0..n).map(|_| rate(rng.below_u32(2501))).collect()
    }

    fn random_cart(rng: &mut Lcg) -> RandomCart {
        let n_lines = 1 + rng.below_usize(6); // 1..=6 lines
        let mut lines: Vec<LineInput> = Vec::with_capacity(n_lines);
        let mut post_line_subtotal = Money::ZERO;
        for _ in 0..n_lines {
            let price_e4 = match rng.below(10) {
                0 => 0,
                1 => rng.below(1_000_000_000) as i64, // up to 100_000.00
                _ => rng.below(1_000_000) as i64,     // up to 100.00
            };
            let qty_milli: i64 = match rng.below(10) {
                0 => 0, // zero-value line
                1 => 2500,
                _ => (1 + rng.below(5)) as i64 * 1000,
            };
            let unit_price = UnitPrice::from_e4(price_e4).expect("generated price is non-negative");
            let qty = Qty::from_milli(qty_milli).expect("generated qty is non-negative");
            let gross = line_gross(unit_price, qty, 2).expect("generated gross fits");
            let line_discount = match rng.below(3) {
                0 => LineDiscount::None,
                1 => LineDiscount::Percent(rate(rng.below_u32(10_001))),
                _ => {
                    let fixed = if gross.is_zero() {
                        0
                    } else {
                        rng.below(u64::try_from(gross.minor()).expect("gross is non-negative") + 1)
                            as i64
                    };
                    LineDiscount::Fixed(money(fixed))
                }
            };
            let discount_amount =
                line_discount_amount(gross, &line_discount).expect("generated discount fits");
            let post = gross
                .checked_sub(discount_amount)
                .expect("discount cannot exceed gross");
            post_line_subtotal = post_line_subtotal
                .checked_add(post)
                .expect("generated subtotal fits");
            let tax = match rng.below(3) {
                0 => TaxSpec::Exempt {
                    reason: "test-exempt".to_string(),
                },
                1 => TaxSpec::Exclusive {
                    rates: random_rates(rng),
                },
                _ => TaxSpec::Inclusive {
                    rates: random_rates(rng),
                },
            };
            lines.push(LineInput {
                unit_price,
                qty,
                line_discount,
                tax,
            });
        }
        let bill_discount = match rng.below(3) {
            0 => BillDiscount::None,
            1 => BillDiscount::Percent(rate(rng.below_u32(10_001))),
            _ => {
                let fixed = if post_line_subtotal.is_zero() {
                    0
                } else {
                    rng.below(
                        u64::try_from(post_line_subtotal.minor())
                            .expect("subtotal is non-negative")
                            + 1,
                    ) as i64
                };
                BillDiscount::Fixed(money(fixed))
            }
        };
        RandomCart {
            lines,
            bill_discount,
        }
    }

    #[test]
    fn property_cart_invariants_10k() {
        const CASES: usize = 10_000;
        const BASE_SEED: u64 = 0x001A_0000;
        for case in 0..CASES {
            // One deterministic seed per case; a failure prints it so the
            // exact cart can be reproduced.
            let seed = BASE_SEED + case as u64;
            let mut rng = Lcg::new(seed);
            let cart = random_cart(&mut rng);
            let priced = price_cart(&cart.lines, &cart.bill_discount, 2);
            assert!(priced.is_ok(), "seed={seed}: price_cart failed: {priced:?}");
            let priced = priced.expect("checked ok above");
            let mut sum_totals = Money::ZERO;
            let mut sum_nets = Money::ZERO;
            let mut sum_taxes = Money::ZERO;
            let mut sum_gross = Money::ZERO;
            let mut sum_line_disc = Money::ZERO;
            let mut sum_bill_disc = Money::ZERO;
            for (index, line) in priced.lines.iter().enumerate() {
                assert!(
                    !line.gross.is_negative()
                        && !line.net.is_negative()
                        && !line.tax.is_negative()
                        && !line.total.is_negative(),
                    "seed={seed} line={index}: negative amount in {line:?}"
                );
                // total == net + tax per line.
                assert_eq!(
                    line.total,
                    line.net.checked_add(line.tax).expect("line total fits"),
                    "seed={seed} line={index}"
                );
                // Components sum to the line tax.
                let component_sum = line.tax_components.iter().fold(Money::ZERO, |acc, c| {
                    acc.checked_add(*c).expect("components fit")
                });
                assert_eq!(component_sum, line.tax, "seed={seed} line={index}");
                // The discounted amount is the net (exclusive/exempt) or
                // the total (inclusive: net + tax == discounted gross).
                let input = cart.lines.get(index).expect("input line exists");
                let discounted = line
                    .gross
                    .checked_sub(line.line_discount)
                    .expect("discount fits")
                    .checked_sub(line.bill_discount)
                    .expect("allocation fits");
                match input.tax {
                    TaxSpec::Exclusive { .. } | TaxSpec::Exempt { .. } => {
                        assert_eq!(discounted, line.net, "seed={seed} line={index}");
                    }
                    TaxSpec::Inclusive { .. } => {
                        assert_eq!(discounted, line.total, "seed={seed} line={index}");
                    }
                }
                sum_totals = sum_totals.checked_add(line.total).expect("bill total fits");
                sum_nets = sum_nets.checked_add(line.net).expect("bill net fits");
                sum_taxes = sum_taxes.checked_add(line.tax).expect("bill tax fits");
                sum_gross = sum_gross.checked_add(line.gross).expect("bill gross fits");
                sum_line_disc = sum_line_disc
                    .checked_add(line.line_discount)
                    .expect("bill line discounts fit");
                sum_bill_disc = sum_bill_disc
                    .checked_add(line.bill_discount)
                    .expect("bill allocations fit");
            }
            assert_eq!(sum_totals, priced.total, "seed={seed}");
            assert_eq!(
                sum_nets.checked_add(sum_taxes).expect("bill net+tax fits"),
                priced.total,
                "seed={seed}"
            );
            assert_eq!(sum_bill_disc, priced.bill_discount, "seed={seed}");
            assert_eq!(sum_gross, priced.subtotal, "seed={seed}");
            assert_eq!(sum_line_disc, priced.line_discounts, "seed={seed}");
            // Change: tender above total gives the exact extra; tender
            // one minor unit short is rejected.
            let extra = money(1234);
            let tender = priced.total.checked_add(extra).expect("tender fits");
            assert_eq!(
                compute_change(priced.total, tender).expect("tender covers total"),
                extra,
                "seed={seed}"
            );
            if !priced.total.is_zero() {
                let short = priced
                    .total
                    .checked_sub(money(1))
                    .expect("total above zero");
                assert_eq!(
                    compute_change(priced.total, short),
                    Err(DomainError::InsufficientTender),
                    "seed={seed}"
                );
            }
        }
    }

    // ---- Bench (loose / relative threshold, spec 002 pattern) ----------

    #[derive(serde::Deserialize)]
    struct Baseline {
        mean_ns_per_cart: u64,
    }

    #[test]
    fn bench_200_line_cart_loose_threshold() {
        use std::time::Instant;
        let mut rng = Lcg::new(0x001A_B00C);
        let mut lines: Vec<LineInput> = Vec::with_capacity(200);
        for _ in 0..200 {
            let cart = random_cart(&mut rng);
            for line in cart.lines {
                if lines.len() < 200 {
                    lines.push(line);
                }
            }
        }
        lines.truncate(200);
        let bill_discount = BillDiscount::Percent(rate(1000));
        // Warm up once so the timed loop measures steady state.
        let _ = price_cart(&lines, &bill_discount, 2).expect("bench cart prices");
        let iterations: u32 = 500;
        let start = Instant::now();
        for _ in 0..iterations {
            let _ = price_cart(&lines, &bill_discount, 2).expect("bench cart prices");
        }
        let elapsed = start.elapsed();
        let mean_ns = elapsed.as_nanos() / u128::from(iterations);
        eprintln!("bench 200-line cart: mean {mean_ns} ns per cart over {iterations} iterations");
        // Loose absolute bound: millisecond order per cart. Measured on
        // this machine: ~0.62 ms in debug (sub-millisecond order),
        // ~0.06 ms in release. The bound is deliberately 3x the debug
        // mean so a slow shared CI runner does not flake.
        assert!(
            mean_ns < 2_000_000,
            "200-line cart pricing took {mean_ns} ns per cart, over the 2 ms loose bound"
        );
        // Relative tripwire vs the stored baseline (spec 002 pattern): fail
        // only on a > 5x regression, so a slow CI runner does not flake.
        let baseline: Baseline =
            serde_json::from_str(include_str!("../baseline/pricing-baseline.json"))
                .expect("baseline parses");
        let allowed = u128::from(baseline.mean_ns_per_cart) * 5;
        assert!(
            mean_ns <= allowed,
            "200-line cart pricing {mean_ns} ns exceeds 5x baseline {} ns",
            baseline.mean_ns_per_cart
        );
    }
}
