//! `Rate`: u32 tax/discount rate in basis points (1800 = 18.00%).
//!
//! Per `context/02-domain-model.md`: rates are basis points, range
//! 0–10000 (0%–100%). Construction rejects anything above 10000.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;

/// A rate in basis points (10000 = 100%).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Rate(u32);

impl Rate {
    /// Basis points in 100%.
    pub const MAX_BASIS_POINTS: u32 = 10_000;

    /// Zero rate.
    pub const ZERO: Rate = Rate(0);

    /// Build from basis points.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidRate` when `basis_points` exceeds 10000.
    pub fn from_basis_points(basis_points: u32) -> Result<Rate, DomainError> {
        if basis_points > Self::MAX_BASIS_POINTS {
            return Err(DomainError::InvalidRate);
        }
        Ok(Rate(basis_points))
    }

    /// Build from a whole percent (1 = 1.00%).
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidRate` when `percent` exceeds 100.
    pub fn from_percent(percent: u32) -> Result<Rate, DomainError> {
        percent
            .checked_mul(100)
            .filter(|bp| *bp <= Self::MAX_BASIS_POINTS)
            .map(Rate)
            .ok_or(DomainError::InvalidRate)
    }

    /// The rate in basis points.
    #[must_use]
    pub const fn basis_points(self) -> u32 {
        self.0
    }

    /// True when the rate is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }
}

impl Default for Rate {
    fn default() -> Rate {
        Rate::ZERO
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn construction_rejects_above_100_percent() {
        assert_eq!(
            Rate::from_basis_points(10_001),
            Err(DomainError::InvalidRate)
        );
        assert_eq!(Rate::from_basis_points(10_000), Ok(Rate(10_000)));
        assert_eq!(Rate::from_basis_points(0), Ok(Rate::ZERO));
        assert_eq!(Rate::from_percent(101), Err(DomainError::InvalidRate));
        assert_eq!(Rate::from_percent(100), Ok(Rate(10_000)));
        assert_eq!(Rate::from_percent(18), Ok(Rate(1800)));
    }

    #[test]
    fn serde_round_trip() {
        for bp in [0_u32, 500, 1800, 10_000] {
            let r = Rate::from_basis_points(bp).expect("valid rate");
            let json = serde_json::to_string(&r).expect("serialize Rate");
            assert_eq!(json, bp.to_string(), "bp={bp}");
            let back: Rate = serde_json::from_str(&json).expect("deserialize Rate");
            assert_eq!(back, r, "bp={bp}");
        }
    }
}
