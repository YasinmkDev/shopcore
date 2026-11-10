//! `Money`: i64 minor units (cents/paisa) of the tenant's single currency.
//!
//! Per `context/02-domain-model.md` and ADR-017 (single currency per tenant
//! in v1), `Money` carries no currency field — two currencies would be
//! indistinguishable in the type system, so the tenant has exactly one.
//! The currency exponent (2 for most, 0 for JPY, 3 for KWD) comes from a
//! currency table; it is not part of this type.
//!
//! Arithmetic is **checked**: `checked_add` / `checked_sub` return `None`
//! on overflow instead of wrapping or panicking. There is no `Add` / `Sub`
//! impl on purpose — silent wrapping is the bug this type exists to prevent.

use serde::{Deserialize, Serialize};

/// Money in minor units (cents/paisa) of the tenant's single currency.
///
/// The inner value is the count of minor units; `Money::from_minor(100)` is
/// one major unit when the currency exponent is 2. Ordering follows the
/// numeric value, so `Money` can be sorted and compared directly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Money(i64);

impl Money {
    /// Zero money.
    pub const ZERO: Money = Money(0);

    /// Build from a count of minor units.
    #[must_use]
    pub const fn from_minor(minor: i64) -> Money {
        Money(minor)
    }

    /// The count of minor units.
    #[must_use]
    pub const fn minor(self) -> i64 {
        self.0
    }

    /// Add, returning `None` on overflow instead of wrapping.
    #[must_use]
    pub fn checked_add(self, other: Money) -> Option<Money> {
        self.0.checked_add(other.0).map(Money)
    }

    /// Subtract, returning `None` on overflow instead of wrapping.
    #[must_use]
    pub fn checked_sub(self, other: Money) -> Option<Money> {
        self.0.checked_sub(other.0).map(Money)
    }

    /// True when the amount is zero.
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.0 == 0
    }

    /// True when the amount is negative.
    #[must_use]
    pub const fn is_negative(self) -> bool {
        self.0 < 0
    }
}

impl Default for Money {
    fn default() -> Money {
        Money::ZERO
    }
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random generator (LCG) so property sweeps are
    /// reproducible without a proptest dependency. Fixed seed; the sequence
    /// is the same on every run and every machine.
    fn lcg_values(count: usize) -> Vec<i64> {
        let mut state: u64 = 0x9E37_79B9_7F4A_7C15;
        (0..count)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                // Reinterpret the top bits as i64 (bitcast, never a wrap).
                i64::from_ne_bytes((state >> 11).to_ne_bytes())
            })
            .collect()
    }

    /// Edge values every property must hold for.
    fn edge_values() -> Vec<i64> {
        vec![
            0,
            1,
            -1,
            2,
            -2,
            100,
            -100,
            i64::MAX,
            i64::MIN,
            i64::MAX - 1,
            i64::MIN + 1,
            i64::MAX / 2,
            i64::MIN / 2,
            1_000_000_000,
            -1_000_000_000,
        ]
    }

    #[test]
    fn checked_add_overflows_to_none() {
        assert_eq!(
            Money::from_minor(i64::MAX).checked_add(Money::from_minor(1)),
            None
        );
        assert_eq!(
            Money::from_minor(i64::MAX).checked_add(Money::from_minor(i64::MAX)),
            None
        );
        assert_eq!(
            Money::from_minor(i64::MIN).checked_add(Money::from_minor(-1)),
            None
        );
        assert_eq!(
            Money::from_minor(i64::MIN).checked_sub(Money::from_minor(1)),
            None
        );
        assert_eq!(
            Money::from_minor(i64::MIN).checked_sub(Money::from_minor(i64::MAX)),
            None
        );
        assert_eq!(
            Money::from_minor(i64::MAX).checked_sub(Money::from_minor(-1)),
            None
        );
    }

    #[test]
    fn checked_add_sub_succeed_below_overflow() {
        assert_eq!(
            Money::from_minor(100).checked_add(Money::from_minor(50)),
            Some(Money::from_minor(150))
        );
        assert_eq!(
            Money::from_minor(100).checked_sub(Money::from_minor(50)),
            Some(Money::from_minor(50))
        );
        assert_eq!(
            Money::from_minor(i64::MAX - 1).checked_add(Money::from_minor(1)),
            Some(Money::from_minor(i64::MAX))
        );
        assert_eq!(
            Money::from_minor(i64::MIN + 1).checked_sub(Money::from_minor(1)),
            Some(Money::from_minor(i64::MIN))
        );
    }

    #[test]
    fn zero_is_identity() {
        for v in edge_values() {
            let m = Money::from_minor(v);
            assert_eq!(m.checked_add(Money::ZERO), Some(m));
            assert_eq!(m.checked_sub(Money::ZERO), Some(m));
        }
    }

    #[test]
    fn add_sub_round_trip() {
        // (a + b) - b == a, and (a - b) + b == a, whenever no overflow.
        for a in edge_values() {
            for b in edge_values() {
                let ma = Money::from_minor(a);
                let mb = Money::from_minor(b);
                if let Some(sum) = ma.checked_add(mb) {
                    assert_eq!(sum.checked_sub(mb), Some(ma), "a={a} b={b}");
                }
                if let Some(diff) = ma.checked_sub(mb) {
                    assert_eq!(diff.checked_add(mb), Some(ma), "a={a} b={b}");
                }
            }
        }
    }

    #[test]
    fn add_is_commutative_and_associative() {
        for a in edge_values() {
            for b in edge_values() {
                let ma = Money::from_minor(a);
                let mb = Money::from_minor(b);
                assert_eq!(ma.checked_add(mb), mb.checked_add(ma), "a={a} b={b}");
            }
        }
        // Associativity on a triple that stays in range.
        let (x, y, z) = (
            Money::from_minor(1_000),
            Money::from_minor(2_000),
            Money::from_minor(3_000),
        );
        let left = x.checked_add(y).and_then(|s| s.checked_add(z));
        let right = y.checked_add(z).and_then(|s| x.checked_add(s));
        assert_eq!(left, right);
        assert_eq!(left, Some(Money::from_minor(6_000)));
    }

    #[test]
    fn ordering_matches_minor_value() {
        let mut values = edge_values();
        values.sort_unstable();
        let mut monies: Vec<Money> = edge_values().into_iter().map(Money::from_minor).collect();
        monies.sort();
        let sorted_minors: Vec<i64> = monies.into_iter().map(Money::minor).collect();
        assert_eq!(sorted_minors, values);
        assert!(Money::from_minor(-5) < Money::from_minor(5));
        assert_eq!(Money::from_minor(0), Money::ZERO);
        assert!(Money::from_minor(i64::MAX) > Money::from_minor(i64::MIN));
    }

    #[test]
    fn serde_round_trip() {
        for v in edge_values() {
            let m = Money::from_minor(v);
            let json = serde_json::to_string(&m).expect("serialize Money");
            // Newtype serializes as the bare integer, not an object.
            assert_eq!(json, v.to_string(), "v={v}");
            let back: Money = serde_json::from_str(&json).expect("deserialize Money");
            assert_eq!(back, m, "v={v}");
        }
    }

    #[test]
    fn property_sweep_deterministic() {
        // 10k pseudo-random pairs: round-trip + ordering invariants hold.
        let values = lcg_values(10_000);
        for w in values.windows(2) {
            let (Some(&a), Some(&b)) = (w.first(), w.get(1)) else {
                continue;
            };
            let ma = Money::from_minor(a);
            let mb = Money::from_minor(b);
            if let Some(sum) = ma.checked_add(mb) {
                assert_eq!(sum.checked_sub(mb), Some(ma), "a={a} b={b}");
            }
            // Ordering is total and consistent with i64.
            assert_eq!(ma.cmp(&mb), a.cmp(&b), "a={a} b={b}");
            assert_eq!(ma.eq(&mb), a == b, "a={a} b={b}");
        }
        // from_minor / minor round-trip on the same sweep.
        for v in values {
            assert_eq!(Money::from_minor(v).minor(), v);
        }
    }

    #[test]
    fn is_zero_and_is_negative() {
        assert!(Money::ZERO.is_zero());
        assert!(!Money::from_minor(1).is_zero());
        assert!(!Money::from_minor(-1).is_zero());
        assert!(Money::from_minor(-1).is_negative());
        assert!(!Money::from_minor(0).is_negative());
        assert!(!Money::from_minor(1).is_negative());
    }
}
