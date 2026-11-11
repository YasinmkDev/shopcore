//! `Qty`: i64 fixed-point quantity, scale 10^3 (weights, litres).
//!
//! Per `context/02-domain-model.md`: whole items are multiples of 1000.
//! Quantities are never negative — construction rejects negatives, and
//! subtraction that would go negative returns `None`.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// Quantity in milli-units (scale 10^3).
///
/// The inner value is the count of milli-units; `Qty::from_whole(2)` is
/// two whole items. Ordering follows the numeric value.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Qty(i64);

impl Qty {
    /// Milli-units per whole unit.
    pub const SCALE: i64 = 1000;

    /// Zero quantity.
    pub const ZERO: Qty = Qty(0);

    /// Build from a count of milli-units.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::NegativeQuantity` when `milli` is negative.
    pub fn from_milli(milli: i64) -> Result<Qty, DomainError> {
        if milli < 0 {
            return Err(DomainError::NegativeQuantity);
        }
        Ok(Qty(milli))
    }

    /// Build from a count of whole units (checked `whole × 1000`).
    ///
    /// # Errors
    ///
    /// Returns `DomainError::NegativeQuantity` when `whole` is negative,
    /// `DomainError::Overflow` when `whole × 1000` overflows `i64`.
    pub fn from_whole(whole: i64) -> Result<Qty, DomainError> {
        if whole < 0 {
            return Err(DomainError::NegativeQuantity);
        }
        whole
            .checked_mul(Self::SCALE)
            .map(Qty)
            .ok_or(DomainError::Overflow)
    }

    /// The count of milli-units.
    #[must_use]
    pub const fn milli(self) -> i64 {
        self.0
    }

    /// Add, returning `None` on overflow instead of wrapping.
    #[must_use]
    pub fn checked_add(self, other: Qty) -> Option<Qty> {
        self.0.checked_add(other.0).map(Qty)
    }

    /// Subtract, returning `None` when the result would be negative.
    /// Subtraction of non-negative quantities cannot overflow `i64`.
    #[must_use]
    pub fn checked_sub(self, other: Qty) -> Option<Qty> {
        self.0.checked_sub(other.0).filter(|v| *v >= 0).map(Qty)
    }

    /// True when the quantity is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Default for Qty {
    fn default() -> Qty {
        Qty::ZERO
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn construction_rejects_negatives() {
        assert_eq!(Qty::from_milli(-1), Err(DomainError::NegativeQuantity));
        assert_eq!(Qty::from_whole(-1), Err(DomainError::NegativeQuantity));
        assert_eq!(Qty::from_milli(0), Ok(Qty::ZERO));
        assert_eq!(Qty::from_whole(2), Ok(Qty(2000)));
    }

    #[test]
    fn from_whole_overflow_is_an_error() {
        assert_eq!(Qty::from_whole(i64::MAX), Err(DomainError::Overflow));
        assert_eq!(
            Qty::from_whole(i64::MAX / 1000),
            Ok(Qty((i64::MAX / 1000) * 1000))
        );
    }

    #[test]
    fn checked_add_sub() {
        let two = Qty::from_whole(2).expect("2 whole");
        let one = Qty::from_whole(1).expect("1 whole");
        assert_eq!(two.checked_add(one), Some(Qty(3000)));
        assert_eq!(two.checked_sub(one), Some(Qty(1000)));
        assert_eq!(one.checked_sub(two), None);
        assert_eq!(
            Qty::from_milli(i64::MAX)
                .expect("max qty")
                .checked_add(Qty::from_milli(1).expect("1 milli")),
            None
        );
    }

    #[test]
    fn serde_round_trip() {
        for v in [0_i64, 1, 1000, 2_500, i64::MAX] {
            let q = Qty::from_milli(v).expect("non-negative");
            let json = serde_json::to_string(&q).expect("serialize Qty");
            assert_eq!(json, v.to_string(), "v={v}");
            let back: Qty = serde_json::from_str(&json).expect("deserialize Qty");
            assert_eq!(back, q, "v={v}");
        }
    }
}
