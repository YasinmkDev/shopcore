//! `DomainError`: the error type for pure domain failures.
//!
//! Every failure in `crates/domain` is a value of this enum — never a
//! panic, never a wrap, never a silent clamp. Callers at the edges map
//! these to stable error codes (see `context/09-engineering-standards.md`).

use std::error::Error;
use std::fmt;

/// Failures of the pure domain functions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DomainError {
    /// A checked computation overflowed the target integer type.
    Overflow,
    /// A quantity was negative at construction.
    NegativeQuantity,
    /// A price or cost was negative at construction.
    NegativePrice,
    /// A tax rate exceeded 10000 basis points (100%).
    InvalidRate,
    /// A line discount exceeded the line total.
    DiscountExceedsLine,
    /// A discount input was invalid (negative fixed amount, or a negative
    /// gross/subtotal passed to a discount function).
    InvalidDiscount,
    /// A fixed bill discount exceeded the subtotal after line discounts.
    DiscountExceedsBill,
    /// Cash tender was below the total.
    InsufficientTender,
    /// A receipt-number input was malformed (empty branch/terminal code,
    /// or a date that is not exactly 8 ASCII digits).
    InvalidReceiptInput,
    /// A cash-rounding granularity was zero or negative.
    InvalidCashRounding,
}

impl fmt::Display for DomainError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        let message = match self {
            DomainError::Overflow => "checked arithmetic overflowed",
            DomainError::NegativeQuantity => "quantity must not be negative",
            DomainError::NegativePrice => "price must not be negative",
            DomainError::InvalidRate => "tax rate must be at most 10000 basis points",
            DomainError::DiscountExceedsLine => "line discount exceeds the line total",
            DomainError::InvalidDiscount => "discount input is invalid",
            DomainError::DiscountExceedsBill => {
                "fixed bill discount exceeds the subtotal after line discounts"
            }
            DomainError::InsufficientTender => "cash tender is below the total",
            DomainError::InvalidReceiptInput => "invalid receipt-number input",
            DomainError::InvalidCashRounding => "cash-rounding granularity must be positive",
        };
        f.write_str(message)
    }
}

impl Error for DomainError {}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn display_messages_are_stable() {
        assert_eq!(
            DomainError::Overflow.to_string(),
            "checked arithmetic overflowed"
        );
        assert_eq!(
            DomainError::InsufficientTender.to_string(),
            "cash tender is below the total"
        );
        assert_eq!(
            DomainError::DiscountExceedsLine.to_string(),
            "line discount exceeds the line total"
        );
    }

    #[test]
    fn implements_std_error() {
        let error: &dyn Error = &DomainError::Overflow;
        assert_eq!(error.to_string(), "checked arithmetic overflowed");
    }
}
