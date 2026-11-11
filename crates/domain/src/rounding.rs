//! Rounding helpers for the domain crate.
//!
//! The global rule (`context/02-domain-model.md`): round half away from
//! zero, applied once per line at defined points. Never round intermediate
//! sums twice. All helpers work on `i128` intermediates so the
//! `price_e4 × qty_milli` product (spec 001a amendment 1) never overflows
//! an `i64`.

use crate::error::DomainError;
use crate::money::Money;

/// Divide `numer` by `denom` (which must be positive), rounding half away
/// from zero: `1.5 -> 2`, `-1.5 -> -2`, `1.4 -> 1`, `2.5 -> 3`.
///
/// Returns `None` when `denom` is not positive or the intermediate
/// addition overflows `i128`.
#[must_use]
pub(crate) fn div_round_half_away(numer: i128, denom: i128) -> Option<i128> {
    if denom <= 0 {
        return None;
    }
    // For a positive numerator, adding half the denominator then flooring
    // rounds half up (away from zero); for an odd denominator the exact
    // half is a .5 fraction, and the integer remainder can only reach it
    // from above, so `denom / 2` (floored) is the exact threshold.
    let half = denom / 2;
    if numer >= 0 {
        numer.checked_add(half)?.checked_div(denom)
    } else {
        let magnitude = numer.checked_neg()?;
        let rounded = magnitude.checked_add(half)?.checked_div(denom)?;
        rounded.checked_neg()
    }
}

/// Convert an `i128` amount to `Money`, checking it fits in `i64`.
pub(crate) fn money_from_i128(value: i128) -> Result<Money, DomainError> {
    i64::try_from(value)
        .map(Money::from_minor)
        .map_err(|_| DomainError::Overflow)
}

/// `10^exponent` as `i128`, checked.
pub(crate) fn ten_pow(exponent: u32) -> Option<i128> {
    10_i128.checked_pow(exponent)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn half_away_from_zero_positive() {
        assert_eq!(div_round_half_away(15, 10), Some(2));
        assert_eq!(div_round_half_away(25, 10), Some(3));
        assert_eq!(div_round_half_away(14, 10), Some(1));
        assert_eq!(div_round_half_away(5, 10), Some(1));
        assert_eq!(div_round_half_away(4, 10), Some(0));
        assert_eq!(div_round_half_away(0, 10), Some(0));
    }

    #[test]
    fn half_away_from_zero_negative() {
        assert_eq!(div_round_half_away(-15, 10), Some(-2));
        assert_eq!(div_round_half_away(-25, 10), Some(-3));
        assert_eq!(div_round_half_away(-14, 10), Some(-1));
        assert_eq!(div_round_half_away(-5, 10), Some(-1));
        assert_eq!(div_round_half_away(-4, 10), Some(0));
    }

    #[test]
    fn half_away_from_zero_odd_denominator() {
        // Exact half is a .5 fraction (e.g. 1.5/3); the integer remainder
        // reaches it only from above, so 5/3 = 1.667 -> 2 and 4/3 = 1.333 -> 1.
        assert_eq!(div_round_half_away(5, 3), Some(2));
        assert_eq!(div_round_half_away(4, 3), Some(1));
        assert_eq!(div_round_half_away(2, 3), Some(1));
        assert_eq!(div_round_half_away(1, 3), Some(0));
    }

    #[test]
    fn non_positive_denominator_is_none() {
        assert_eq!(div_round_half_away(10, 0), None);
        assert_eq!(div_round_half_away(10, -3), None);
    }

    #[test]
    fn overflow_is_none() {
        // i128::MAX + half the denominator overflows; i128::MIN cannot
        // be negated.
        assert_eq!(div_round_half_away(i128::MAX, 10), None);
        assert_eq!(div_round_half_away(i128::MIN, 1), None);
        // No overflow when the addition fits.
        assert_eq!(div_round_half_away(i128::MAX, 1), Some(i128::MAX));
    }

    #[test]
    fn money_from_i128_checked() {
        assert_eq!(
            money_from_i128(i128::from(i64::MAX)),
            Ok(Money::from_minor(i64::MAX))
        );
        assert_eq!(
            money_from_i128(i128::from(i64::MAX) + 1),
            Err(DomainError::Overflow)
        );
        assert_eq!(
            money_from_i128(i128::from(i64::MIN) - 1),
            Err(DomainError::Overflow)
        );
    }
}
