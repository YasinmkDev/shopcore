//! Cash rounding and change — cash-only payment math.
//!
//! Cash rounding (spec 001a amendment 4) is DEFAULT OFF: the granularity
//! is the minor unit, so no rounding happens. When a tenant turns it ON,
//! rounding applies ONLY to the cash-tendered total AFTER tax, as a
//! separate UNTAXED line, never hidden in prices. Change is `tender −
//! total` on the final (possibly rounded) total.

use serde::{Deserialize, Serialize};

use crate::error::DomainError;
use crate::money::Money;
use crate::rounding::{div_round_half_away, money_from_i128};

/// Cash-rounding setting for a tenant.
///
/// The granularity is a count of minor units (5 = 0.05, 10 = 0.10).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub enum CashRounding {
    /// Default: no rounding (granularity = minor unit).
    #[default]
    Off,
    /// Rounding on: round the cash-tendered total to the nearest
    /// `granularity_minor` minor units.
    On {
        /// Rounding granularity in minor units; must be positive.
        granularity_minor: i64,
    },
}

impl CashRounding {
    /// Build a rounding-on setting.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::InvalidCashRounding` when
    /// `granularity_minor` is zero or negative.
    pub fn on(granularity_minor: i64) -> Result<CashRounding, DomainError> {
        if granularity_minor <= 0 {
            return Err(DomainError::InvalidCashRounding);
        }
        Ok(CashRounding::On { granularity_minor })
    }

    /// Apply rounding to a cash-tendered total.
    ///
    /// Returns the rounded total and the separate rounding line
    /// (`rounded − total`, which may be negative, zero, or positive).
    /// With rounding off, the total is unchanged and the line is zero.
    /// The line is untaxed — it is never part of the taxable amount.
    ///
    /// # Errors
    ///
    /// Returns `DomainError::Overflow` on arithmetic overflow.
    pub fn apply(self, total: Money) -> Result<CashRoundingResult, DomainError> {
        match self {
            CashRounding::Off => Ok(CashRoundingResult {
                rounded_total: total,
                rounding_line: Money::ZERO,
            }),
            CashRounding::On { granularity_minor } => {
                let rounded_minor =
                    div_round_half_away(i128::from(total.minor()), i128::from(granularity_minor))
                        .ok_or(DomainError::Overflow)?
                        .checked_mul(i128::from(granularity_minor))
                        .ok_or(DomainError::Overflow)?;
                let rounded_total = money_from_i128(rounded_minor)?;
                let rounding_line = rounded_total
                    .checked_sub(total)
                    .ok_or(DomainError::Overflow)?;
                Ok(CashRoundingResult {
                    rounded_total,
                    rounding_line,
                })
            }
        }
    }
}

/// The result of applying cash rounding.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct CashRoundingResult {
    /// The total after rounding.
    pub rounded_total: Money,
    /// The separate untaxed rounding line (`rounded_total − total`).
    pub rounding_line: Money,
}

/// Change for a cash payment: `tender − total`.
///
/// Pass the FINAL total (after cash rounding, if any). Non-cash
/// overpayment is rejected by the payment spec, not here.
///
/// # Errors
///
/// Returns `DomainError::InsufficientTender` when tender is below total,
/// `DomainError::Overflow` on arithmetic overflow.
pub fn compute_change(total: Money, tender: Money) -> Result<Money, DomainError> {
    if tender < total {
        return Err(DomainError::InsufficientTender);
    }
    tender.checked_sub(total).ok_or(DomainError::Overflow)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::cast_possible_wrap)]
mod tests {
    use super::*;

    fn money(minor: i64) -> Money {
        Money::from_minor(minor)
    }

    #[test]
    fn rounding_default_is_off() {
        let setting = CashRounding::default();
        assert_eq!(setting, CashRounding::Off);
        let result = setting.apply(money(5232)).expect("off never fails");
        assert_eq!(result.rounded_total, money(5232));
        assert_eq!(result.rounding_line, Money::ZERO);
    }

    #[test]
    fn rounding_on_goldens() {
        // 52.32 -> 52.30 at 0.05.
        let result = CashRounding::on(5)
            .expect("granularity 5")
            .apply(money(5232))
            .expect("rounding applies");
        assert_eq!(result.rounded_total, money(5230));
        assert_eq!(result.rounding_line, money(-2));
        // 52.33 -> 52.35 at 0.05.
        let result = CashRounding::on(5)
            .expect("granularity 5")
            .apply(money(5233))
            .expect("rounding applies");
        assert_eq!(result.rounded_total, money(5235));
        assert_eq!(result.rounding_line, money(2));
        // 52.35 -> 52.40 at 0.10 (half away from zero: 523.5 -> 524).
        let result = CashRounding::on(10)
            .expect("granularity 10")
            .apply(money(5235))
            .expect("rounding applies");
        assert_eq!(result.rounded_total, money(5240));
        assert_eq!(result.rounding_line, money(5));
        // An already-rounded total is unchanged.
        let result = CashRounding::on(5)
            .expect("granularity 5")
            .apply(money(5230))
            .expect("rounding applies");
        assert_eq!(result.rounded_total, money(5230));
        assert_eq!(result.rounding_line, Money::ZERO);
    }

    #[test]
    fn rounding_granularity_must_be_positive() {
        assert_eq!(CashRounding::on(0), Err(DomainError::InvalidCashRounding));
        assert_eq!(CashRounding::on(-5), Err(DomainError::InvalidCashRounding));
    }

    #[test]
    fn compute_change_rules() {
        assert_eq!(compute_change(money(5232), money(6000)), Ok(money(768)));
        assert_eq!(compute_change(money(5232), money(5232)), Ok(Money::ZERO));
        assert_eq!(
            compute_change(money(5232), money(5231)),
            Err(DomainError::InsufficientTender)
        );
    }

    // ---- Golden dataset (cash section of the same file) -----------------

    #[derive(serde::Deserialize)]
    struct CashGoldenFile {
        cash_cases: Vec<GoldenCashCase>,
    }

    #[derive(serde::Deserialize)]
    struct GoldenCashCase {
        name: String,
        total: i64,
        tender: i64,
        rounding: Option<GoldenRounding>,
        expected: GoldenCashExpected,
    }

    #[derive(serde::Deserialize)]
    struct GoldenRounding {
        granularity_minor: i64,
    }

    #[derive(serde::Deserialize)]
    struct GoldenCashExpected {
        rounded_total: i64,
        rounding_line: i64,
        change: i64,
    }

    #[test]
    fn golden_cash_cases() {
        let file: CashGoldenFile =
            serde_json::from_str(include_str!("../../../tests/golden/001a-checkout.json"))
                .expect("golden file parses");
        assert!(!file.cash_cases.is_empty(), "golden file needs cash cases");
        for case in &file.cash_cases {
            let setting = match &case.rounding {
                Some(r) => CashRounding::on(r.granularity_minor).expect("golden granularity"),
                None => CashRounding::Off,
            };
            let applied = setting
                .apply(money(case.total))
                .expect("golden rounding applies");
            assert_eq!(
                applied.rounded_total,
                money(case.expected.rounded_total),
                "golden case failed: {}",
                case.name
            );
            assert_eq!(
                applied.rounding_line,
                money(case.expected.rounding_line),
                "golden case failed: {}",
                case.name
            );
            let change = compute_change(applied.rounded_total, money(case.tender))
                .expect("golden tender covers total");
            assert_eq!(
                change,
                money(case.expected.change),
                "golden case failed: {}",
                case.name
            );
        }
    }

    // ---- Property tests -------------------------------------------------

    /// Same LCG constants as `Money` and `pricing` tests.
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

        fn below(&mut self, bound: u64) -> u64 {
            self.next_u64() % bound
        }
    }

    #[test]
    fn property_rounding_idempotent_10k() {
        const CASES: usize = 10_000;
        const BASE_SEED: u64 = 0x001A_CA54;
        for case in 0..CASES {
            let seed = BASE_SEED + case as u64;
            let mut rng = Lcg::new(seed);
            // Totals 0..100_000.00 minor, granularities 1, 2, 5, 10, 25, 50, 100.
            let total = money(rng.below(10_000_000) as i64);
            let granularities = [1_i64, 2, 5, 10, 25, 50, 100];
            let granularity_index =
                usize::try_from(rng.below(7)).expect("index below 7 fits in usize");
            let granularity = *granularities
                .get(granularity_index)
                .expect("index in range");
            let setting = CashRounding::on(granularity).expect("granularity positive");
            let first = setting.apply(total).expect("rounding applies");
            // Idempotent: rounding an already-rounded total changes nothing.
            let second = setting
                .apply(first.rounded_total)
                .expect("rounding applies");
            assert_eq!(
                second.rounded_total, first.rounded_total,
                "seed={seed}: rounding not idempotent"
            );
            assert_eq!(
                second.rounding_line,
                Money::ZERO,
                "seed={seed}: second rounding line not zero"
            );
            // The rounding line is at most half the granularity away.
            let line = first.rounding_line.minor();
            assert!(
                line.abs() * 2 <= granularity,
                "seed={seed}: rounding line {line} too far for granularity {granularity}"
            );
            // Rounded total is a multiple of the granularity.
            assert_eq!(
                first.rounded_total.minor() % granularity,
                0,
                "seed={seed}: rounded total not a multiple of granularity"
            );
        }
    }
}
