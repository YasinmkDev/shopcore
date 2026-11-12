//! Timestamp conversion: epoch-ms INTEGER <-> ISO-8601 UTC with `Z`.
//!
//! The single place in the codebase that formats or parses ISO-8601
//! (owner decision 2026-11-11, `context/04-data-and-sync.md`).
//! Timestamps are stored as epoch-ms INTEGER in the DB; ISO-8601 UTC
//! with `Z` is the wire/export format only. Anything that puts a
//! timestamp on the sync wire, in a payload, or in an export must go
//! through [`to_iso8601`] / [`from_iso8601`] — never hand-roll the
//! formatting again (the bench CLI's `iso_now` used to do exactly that).
//!
//! No date library: the civil-date math is Howard Hinnant's
//! days-from-civil / civil-from-days algorithm, hand-rolled with
//! [`i64::div_euclid`] / [`i64::rem_euclid`] so negative values
//! (pre-1970) behave correctly. Canonical form is exactly
//! `YYYY-MM-DDTHH:MM:SS.mmmZ` — always 3 fractional digits,
//! zero-padded; years outside 0000–9999 print with as many digits as
//! needed (negative years with a leading `-`).

/// Milliseconds in one day.
const MILLIS_PER_DAY: i64 = 86_400_000;
/// Seconds in one day.
const SECONDS_PER_DAY: i64 = 86_400;
/// Days from 0000-03-01 to 1970-01-01 (Hinnant's epoch shift).
const EPOCH_SHIFT_DAYS: i64 = 719_468;
/// Days in one 400-year era (Hinnant).
const DAYS_PER_ERA: i64 = 146_097;

/// Convert epoch milliseconds to ISO-8601 UTC with `Z`, e.g.
/// `2026-11-11T08:44:16.123Z`. Always emits exactly 3 fractional
/// digits (millisecond precision, zero-padded).
#[must_use]
pub fn to_iso8601(epoch_ms: i64) -> String {
    let secs = epoch_ms.div_euclid(1_000);
    let millis = epoch_ms.rem_euclid(1_000);
    let days = secs.div_euclid(SECONDS_PER_DAY);
    let secs_of_day = secs.rem_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    let hour = secs_of_day / 3_600;
    let minute = (secs_of_day % 3_600) / 60;
    let second = secs_of_day % 60;
    format!(
        "{year}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}.{millis:03}Z",
        year = format_year(year),
        month = month,
        day = day,
        hour = hour,
        minute = minute,
        second = second,
        millis = millis,
    )
}

/// Convert epoch milliseconds to the sale date `yyyymmdd` (UTC),
/// e.g. `20261112`. The receipt number and the daily aggregates use
/// this date (001b-2 `CompleteSaleCashInput.date_yyyymmdd`). Same
/// civil-date math as [`to_iso8601`]; the date is the UTC calendar
/// day, never the till's local day.
#[must_use]
pub fn yyyymmdd_utc(epoch_ms: i64) -> String {
    let secs = epoch_ms.div_euclid(1_000);
    let days = secs.div_euclid(SECONDS_PER_DAY);
    let (year, month, day) = civil_from_days(days);
    format!("{year:04}{month:02}{day:02}")
}

/// Format a year canonically: 4 digits, zero-padded, for 0–9999;
/// more digits as needed above 9999; a leading `-` plus the same
/// zero-padding for negative years.
fn format_year(year: i64) -> String {
    if year >= 0 {
        format!("{year:04}")
    } else {
        format!("-{:04}", year.unsigned_abs())
    }
}

/// Days since the Unix epoch -> proleptic Gregorian (year, month, day).
/// Howard Hinnant's civil-from-days; exact for every `i64` day count
/// reachable from an `i64` millisecond value.
fn civil_from_days(days: i64) -> (i64, i64, i64) {
    let shifted = days + EPOCH_SHIFT_DAYS;
    let era = shifted.div_euclid(DAYS_PER_ERA);
    let day_of_era = shifted.rem_euclid(DAYS_PER_ERA);
    let year_of_era =
        (day_of_era - day_of_era / 1_460 + day_of_era / 36_524 - day_of_era / 146_096) / 365;
    let year = year_of_era + era * 400;
    let day_of_year = day_of_era - (365 * year_of_era + year_of_era / 4 - year_of_era / 100);
    let month_prime = (5 * day_of_year + 2) / 153;
    let day = day_of_year - (153 * month_prime + 2) / 5 + 1;
    let month = if month_prime < 10 {
        month_prime + 3
    } else {
        month_prime - 9
    };
    let year = if month <= 2 { year + 1 } else { year };
    (year, month, day)
}

/// Proleptic Gregorian (year, month, day) -> days since the Unix epoch.
/// Howard Hinnant's days-from-civil. Returns `None` if the result
/// would overflow `i64` (only for absurd year magnitudes).
fn days_from_civil(year: i64, month: i64, day: i64) -> Option<i64> {
    let year_adj = if month <= 2 { year - 1 } else { year };
    let era = year_adj.div_euclid(400);
    let year_of_era = year_adj.rem_euclid(400);
    let month_prime = if month > 2 { month - 3 } else { month + 9 };
    let day_of_year = (153 * month_prime + 2) / 5 + day - 1;
    let day_of_era = year_of_era
        .checked_mul(365)?
        .checked_add(year_of_era / 4)?
        .checked_sub(year_of_era / 100)?
        .checked_add(day_of_year)?;
    era.checked_mul(DAYS_PER_ERA)?
        .checked_add(day_of_era)?
        .checked_sub(EPOCH_SHIFT_DAYS)
}

/// Whether `year` is a leap year in the proleptic Gregorian calendar.
fn is_leap_year(year: i64) -> bool {
    year.rem_euclid(4) == 0 && (year.rem_euclid(100) != 0 || year.rem_euclid(400) == 0)
}

/// Days in `month` (1–12) of `year`; 0 for an out-of-range month.
fn days_in_month(year: i64, month: i64) -> i64 {
    match month {
        1 | 3 | 5 | 7 | 8 | 10 | 12 => 31,
        4 | 6 | 9 | 11 => 30,
        2 => {
            if is_leap_year(year) {
                29
            } else {
                28
            }
        }
        _ => 0,
    }
}

/// Parse exactly what [`to_iso8601`] emits: `YYYY-MM-DDTHH:MM:SS.mmmZ`
/// (years outside 0000–9999 with as many digits as needed, negative
/// years with a leading `-`). Returns `None` for anything else —
/// missing `Z`, wrong separators, wrong field widths, out-of-range
/// month/day/hour/minute/second, a non-existent date (e.g. 2023-02-29),
/// or a value outside the `i64` millisecond range. Never panics.
#[must_use]
pub fn from_iso8601(text: &str) -> Option<i64> {
    let text = text.strip_suffix('Z')?;
    let (date, time) = text.split_once('T')?;
    let (year_text, month_day) = split_year(date)?;
    let year = parse_year(year_text)?;
    let (month_text, day_text) = month_day.split_once('-')?;
    let month = parse_fixed_digits(month_text, 2)?;
    let day = parse_fixed_digits(day_text, 2)?;
    let (hms, millis_text) = time.split_once('.')?;
    let millis = parse_fixed_digits(millis_text, 3)?;
    let mut hms_parts = hms.split(':');
    let hour = parse_fixed_digits(hms_parts.next()?, 2)?;
    let minute = parse_fixed_digits(hms_parts.next()?, 2)?;
    let second = parse_fixed_digits(hms_parts.next()?, 2)?;
    if hms_parts.next().is_some() {
        return None;
    }
    if !(1..=12).contains(&month) || day < 1 || day > days_in_month(year, month) {
        return None;
    }
    if hour > 23 || minute > 59 || second > 59 {
        return None;
    }
    let days = days_from_civil(year, month, day)?;
    days.checked_mul(MILLIS_PER_DAY)?
        .checked_add(hour.checked_mul(3_600_000)?)?
        .checked_add(minute.checked_mul(60_000)?)?
        .checked_add(second.checked_mul(1_000)?)?
        .checked_add(millis)
}

/// Split a `YYYY-MM-DD` date (year possibly negative) into the year
/// text (including a leading `-` when present) and the `MM-DD` rest.
fn split_year(date: &str) -> Option<(&str, &str)> {
    let (negative, rest) = match date.strip_prefix('-') {
        Some(rest) => (true, rest),
        None => (false, date),
    };
    let (year_digits, month_day) = rest.split_once('-')?;
    if year_digits.is_empty() || !year_digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let year_text = if negative {
        date.get(..year_digits.len() + 1)?
    } else {
        year_digits
    };
    Some((year_text, month_day))
}

/// Parse a year in the canonical form [`to_iso8601`] emits: at least
/// 4 digits (zero-padded to 4 when the value needs fewer), no leading
/// zero when more than 4 digits are present, optional leading `-`.
fn parse_year(text: &str) -> Option<i64> {
    let (negative, digits) = match text.strip_prefix('-') {
        Some(digits) => (true, digits),
        None => (false, text),
    };
    if digits.len() < 4 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    if digits.len() > 4 && digits.as_bytes().first() == Some(&b'0') {
        return None;
    }
    let magnitude: i64 = digits.parse().ok()?;
    if negative {
        magnitude.checked_neg()
    } else {
        Some(magnitude)
    }
}

/// Parse an all-digit field of exactly `width` digits (e.g. `MM`,
/// `SS`, `mmm`); `None` for any other shape.
fn parse_fixed_digits(text: &str, width: usize) -> Option<i64> {
    if text.len() != width || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    text.parse().ok()
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    /// Deterministic pseudo-random generator (same LCG constants as the
    /// `Money` / `pricing` property tests): fixed seed, so a failing
    /// case is reproducible from the seed printed in the assertion.
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
    }

    /// Fixed seed base for the property sweep (same pattern as
    /// `pricing`: one deterministic seed per case).
    const BASE_SEED: u64 = 0x51A0_BEEF_CAFE_2026;

    /// Random sweep range: 1900-01-01 to 2100-01-01 in epoch ms.
    /// Covers pre-1970 (negative) values and the far future.
    const RANGE_START_MS: i64 = -2_208_988_800_000;
    const RANGE_END_MS: i64 = 4_102_444_800_000;

    #[test]
    fn known_vectors() {
        let cases: &[(i64, &str)] = &[
            (0, "1970-01-01T00:00:00.000Z"),
            (1, "1970-01-01T00:00:00.001Z"),
            (999, "1970-01-01T00:00:00.999Z"),
            (1_000, "1970-01-01T00:00:01.000Z"),
            (1_001, "1970-01-01T00:00:01.001Z"),
            (1_999, "1970-01-01T00:00:01.999Z"),
            (2_000, "1970-01-01T00:00:02.000Z"),
            (2_001, "1970-01-01T00:00:02.001Z"),
            (-1, "1969-12-31T23:59:59.999Z"),
            (-999, "1969-12-31T23:59:59.001Z"),
            (-1_000, "1969-12-31T23:59:59.000Z"),
            (-1_001, "1969-12-31T23:59:58.999Z"),
            (-86_400_000, "1969-12-31T00:00:00.000Z"),
            (951_782_400_000, "2000-02-29T00:00:00.000Z"),
            (1_709_164_800_000, "2024-02-29T00:00:00.000Z"),
            (1_767_225_600_000, "2026-01-01T00:00:00.000Z"),
            (1_790_000_000_123, "2026-09-21T14:13:20.123Z"),
            (4_102_444_800_000, "2100-01-01T00:00:00.000Z"),
            (4_102_444_800_123, "2100-01-01T00:00:00.123Z"),
        ];
        for (ms, expected) in cases {
            assert_eq!(to_iso8601(*ms), *expected, "ms={ms}");
            assert_eq!(from_iso8601(expected), Some(*ms), "text={expected}");
        }
    }

    #[test]
    fn yyyymmdd_utc_matches_the_iso_date() {
        // Known vectors (same instants as `known_vectors` above) plus
        // the UTC day boundary: 23:59:59.999 is still the same day.
        let cases: &[(i64, &str)] = &[
            (0, "19700101"),
            (-1, "19691231"),
            (-86_400_000, "19691231"),
            (951_782_400_000, "20000229"),
            (1_709_164_800_000, "20240229"),
            (1_767_225_600_000, "20260101"),
            (1_790_000_000_123, "20260921"),
            (86_399_999, "19700101"),
            (86_400_000, "19700102"),
        ];
        for (ms, expected) in cases {
            assert_eq!(yyyymmdd_utc(*ms), *expected, "ms={ms}");
            // Cross-check against the ISO form: the date part, dashes
            // removed, must agree.
            let iso = to_iso8601(*ms);
            let iso_date: String = iso.chars().take(10).filter(|c| *c != '-').collect();
            assert_eq!(iso_date, *expected, "ms={ms}");
        }
    }

    #[test]
    fn from_iso8601_rejects_anything_else() {
        let bad: &[&str] = &[
            "",
            "1970-01-01",
            "1970-01-01T00:00:00Z",          // no fractional digits
            "1970-01-01T00:00:00.00Z",       // 2 fractional digits
            "1970-01-01T00:00:00.0000Z",     // 4 fractional digits
            "1970-01-01T00:00:00.000",       // no Z
            "1970-01-01T00:00:00.000z",      // lowercase z
            "1970-01-01t00:00:00.000Z",      // lowercase t
            "1970-01-01 00:00:00.000Z",      // space, not T
            "1970-01-01T00:00:00.000Z ",     // trailing space
            " 1970-01-01T00:00:00.000Z",     // leading space
            "1970/01/01T00:00:00.000Z",      // wrong date separator
            "970-01-01T00:00:00.000Z",       // 3-digit year
            "02070-01-01T00:00:00.000Z",     // 5-digit year with leading zero
            "1970-13-01T00:00:00.000Z",      // month 13
            "1970-00-01T00:00:00.000Z",      // month 00
            "1970-01-32T00:00:00.000Z",      // day 32
            "1970-01-00T00:00:00.000Z",      // day 00
            "2023-02-29T00:00:00.000Z",      // 2023 is not a leap year
            "2024-02-30T00:00:00.000Z",      // Feb never has 30 days
            "1970-04-31T00:00:00.000Z",      // April has 30 days
            "1970-01-01T24:00:00.000Z",      // hour 24
            "1970-01-01T00:60:00.000Z",      // minute 60
            "1970-01-01T00:00:60.000Z",      // second 60 (no leap seconds)
            "1970-01-01T00:00:00.000+00:00", // offset, not Z
            "1970-01-01T00:00.000Z",         // missing seconds
            "1970-01-01T00:00:00.000ZZ",     // double Z
            "1970-1-01T00:00:00.000Z",       // 1-digit month
            "1970-01-1T00:00:00.000Z",       // 1-digit day
        ];
        for text in bad {
            assert_eq!(from_iso8601(text), None, "must reject {text:?}");
        }
    }

    #[test]
    fn round_trip_property_fixed_seed_lcg() {
        // Edge values first: epoch, second boundaries, pre-1970, leap
        // days, far future.
        let edges: &[i64] = &[
            0,
            999,
            1_000,
            1_001,
            -1,
            -999,
            -1_000,
            -1_001,
            951_782_400_000,
            4_102_444_800_000,
            i64::MIN / 1_000_000,
            i64::MAX / 1_000_000,
        ];
        for ms in edges {
            let text = to_iso8601(*ms);
            assert_eq!(from_iso8601(&text), Some(*ms), "edge ms={ms} text={text}");
        }
        // 10,000 pseudo-random cases in 1900–2100, one deterministic
        // seed per case; a failure prints its seed.
        for case in 0..10_000u64 {
            let seed = BASE_SEED + case;
            let mut rng = Lcg::new(seed);
            let span = (RANGE_END_MS - RANGE_START_MS) as u64;
            let offset = rng.next_u64() % span;
            let ms = RANGE_START_MS + i64::try_from(offset).expect("offset fits i64");
            let text = to_iso8601(ms);
            assert_eq!(
                from_iso8601(&text),
                Some(ms),
                "seed={seed}: round-trip failed for ms={ms} text={text}"
            );
        }
    }
}
