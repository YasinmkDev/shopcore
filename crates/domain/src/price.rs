//! `UnitPrice` / `UnitCost`: i64 fixed-point, scale 10^4 of the major unit.
//!
//! Per `context/02-domain-model.md`: the 10^4 scale keeps weighted-average
//! cost precise. Conversion to `Money` happens only via the rounding rule
//! (see `crate::pricing::line_gross`), never by a bare division.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// A unit price in 10^-4 major units (`UnitPrice::from_e4(1_000_000)` is
/// 100.00 major units).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnitPrice(i64);

/// A unit cost in 10^-4 major units (same scale as `UnitPrice`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct UnitCost(i64);

impl UnitPrice {
    /// Scale: e4 units per major unit.
    pub const SCALE: i64 = 10_000;

    /// Zero price.
    pub const ZERO: UnitPrice = UnitPrice(0);

    /// Build from e4 units.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::NegativePrice` when `e4` is negative.
    pub fn from_e4(e4: i64) -> Result<UnitPrice, DomainError> {
        if e4 < 0 {
            return Err(DomainError::NegativePrice);
        }
        Ok(UnitPrice(e4))
    }

    /// The price in e4 units.
    #[must_use]
    pub const fn e4(self) -> i64 {
        self.0
    }

    /// True when the price is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl UnitCost {
    /// Scale: e4 units per major unit.
    pub const SCALE: i64 = 10_000;

    /// Zero cost.
    pub const ZERO: UnitCost = UnitCost(0);

    /// Build from e4 units.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::NegativePrice` when `e4` is negative.
    pub fn from_e4(e4: i64) -> Result<UnitCost, DomainError> {
        if e4 < 0 {
            return Err(DomainError::NegativePrice);
        }
        Ok(UnitCost(e4))
    }

    /// The cost in e4 units.
    #[must_use]
    pub const fn e4(self) -> i64 {
        self.0
    }

    /// True when the cost is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Default for UnitPrice {
    fn default() -> UnitPrice {
        UnitPrice::ZERO
    }
}

impl Default for UnitCost {
    fn default() -> UnitCost {
        UnitCost::ZERO
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn construction_rejects_negatives() {
        assert_eq!(UnitPrice::from_e4(-1), Err(DomainError::NegativePrice));
        assert_eq!(UnitCost::from_e4(-1), Err(DomainError::NegativePrice));
        assert_eq!(UnitPrice::from_e4(0), Ok(UnitPrice::ZERO));
        assert_eq!(UnitCost::from_e4(1_000_000), Ok(UnitCost(1_000_000)));
    }

    #[test]
    fn serde_round_trip() {
        let p = UnitPrice::from_e4(1_000_000).expect("100.00");
        let json = serde_json::to_string(&p).expect("serialize UnitPrice");
        assert_eq!(json, "1000000");
        let back: UnitPrice = serde_json::from_str(&json).expect("deserialize UnitPrice");
        assert_eq!(back, p);
    }
}
