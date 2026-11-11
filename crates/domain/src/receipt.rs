//! Receipt numbers: `{branch_code}-{terminal_code}-{yyyymmdd}-{seq}`.
//!
//! Per `context/02-domain-model.md`: the number is generated locally and
//! is unique offline because the terminal code is part of it. The
//! sequence is per terminal per day; the generator takes the last
//! sequence (and the date) as injected inputs — no DB, no clock. The
//! caller resets the last sequence to 0 when the date changes (day
//! boundary rollover).

use crate::error::DomainError;

/// Next receipt number for a terminal on a day.
///
/// `yyyymmdd` must be exactly 8 ASCII digits; `branch_code` and
/// `terminal_code` must be non-empty. The sequence is `last_seq + 1`
/// (checked). Calendar correctness of the date is the caller's job.
///
/// # Errors
///
/// Returns `DomainError::InvalidReceiptInput` for an empty branch or
/// terminal code, or a date that is not exactly 8 ASCII digits;
/// `DomainError::Overflow` when `last_seq` is already `u32::MAX`.
pub fn next_receipt_number(
    branch_code: &str,
    terminal_code: &str,
    yyyymmdd: &str,
    last_seq: u32,
) -> Result<String, DomainError> {
    if branch_code.is_empty()
        || terminal_code.is_empty()
        || yyyymmdd.len() != 8
        || !yyyymmdd.bytes().all(|b| b.is_ascii_digit())
    {
        return Err(DomainError::InvalidReceiptInput);
    }
    let seq = last_seq.checked_add(1).ok_or(DomainError::Overflow)?;
    Ok(format!("{branch_code}-{terminal_code}-{yyyymmdd}-{seq}"))
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn format_and_sequence() {
        assert_eq!(
            next_receipt_number("BR1", "T01", "20261110", 41),
            Ok("BR1-T01-20261110-42".to_string())
        );
        assert_eq!(
            next_receipt_number("BR1", "T01", "20261110", 0),
            Ok("BR1-T01-20261110-1".to_string())
        );
    }

    #[test]
    fn day_boundary_rollover() {
        // New day: the caller passes last_seq 0, so the sequence restarts.
        assert_eq!(
            next_receipt_number("BR1", "T01", "20261111", 0),
            Ok("BR1-T01-20261111-1".to_string())
        );
        assert_eq!(
            next_receipt_number("BR1", "T01", "20261110", 999),
            Ok("BR1-T01-20261110-1000".to_string())
        );
    }

    #[test]
    fn invalid_inputs_are_errors() {
        assert_eq!(
            next_receipt_number("", "T01", "20261110", 0),
            Err(DomainError::InvalidReceiptInput)
        );
        assert_eq!(
            next_receipt_number("BR1", "", "20261110", 0),
            Err(DomainError::InvalidReceiptInput)
        );
        assert_eq!(
            next_receipt_number("BR1", "T01", "2026-11-10", 0),
            Err(DomainError::InvalidReceiptInput)
        );
        assert_eq!(
            next_receipt_number("BR1", "T01", "2026111", 0),
            Err(DomainError::InvalidReceiptInput)
        );
        assert_eq!(
            next_receipt_number("BR1", "T01", "2026111A", 0),
            Err(DomainError::InvalidReceiptInput)
        );
        assert_eq!(
            next_receipt_number("BR1", "T01", "20261110", u32::MAX),
            Err(DomainError::Overflow)
        );
    }
}
