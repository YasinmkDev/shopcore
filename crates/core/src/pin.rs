//! PIN hashing + verification (spec 001b-1 req. 7-8, `07-security-compliance`).
//!
//! Argon2id with the OWASP-minimum parameters (19 MiB memory, 2
//! iterations, 1 lane). The measured verification time is recorded in
//! ADR-032. Verification is CPU-bound and MUST run off the UI thread: the
//! sync functions here are called from `tokio::task::spawn_blocking` (see
//! `verify_pin_async` in `users.rs`) or from another blocking thread —
//! never from an async task, and never from the `Tauri` main thread.

use argon2::password_hash::{Error as PhcError, phc::PasswordHash};
use argon2::{Algorithm, Argon2, Params, PasswordHasher, PasswordVerifier, Version};

/// Argon2id memory cost in KiB (19 MiB — the OWASP minimum).
pub const ARGON2_M_COST_KIB: u32 = 19_456;
/// Argon2id time cost in iterations (the OWASP minimum).
pub const ARGON2_T_COST: u32 = 2;
/// Argon2id parallelism in lanes (the OWASP minimum).
pub const ARGON2_P_COST: u32 = 1;

/// The PIN the dummy hash is made from. Never assigned to a user; it exists
/// only so `verify_against_dummy_hash` can spend one `Argon2id` verify
/// (the same cost as a wrong-PIN verify) for an unknown username.
const DUMMY_PIN: &str = "0000-dummy-pin-never-assigned";

/// Errors from PIN hashing / verification.
#[derive(Debug, thiserror::Error)]
pub enum PinError {
    /// Wrong PIN, or the username does not exist (deliberately the same
    /// error — the caller must not reveal which).
    #[error("invalid PIN or unknown user")]
    InvalidCredentials,

    /// The user is locked out until `until_ms` (epoch ms). `event` is
    /// `Some` only when THIS call triggered the lockout (the event is the
    /// seam 001b-2's audit writer consumes); it is `None` when the user
    /// was already locked out.
    #[error("PIN locked out until {until_ms} (epoch ms)")]
    LockedOut {
        until_ms: i64,
        event: Option<crate::users::PinLockoutEvent>,
    },

    /// Manager approval was attempted without naming a manager (PIN-only
    /// scanning is rejected — req. 8).
    #[error("manager approval requires a named manager (PIN-only scanning is rejected)")]
    ManagerNotSelected,

    /// The PIN is shorter than the minimum length (07: PIN length >= 4).
    #[error("PIN must be at least {min} characters")]
    PinTooShort {
        /// The minimum PIN length.
        min: usize,
    },

    /// Hashing failed (bad parameters or no randomness).
    #[error("PIN hashing failed")]
    HashFailed,

    /// The stored PIN hash is not a valid PHC string.
    #[error("stored PIN hash is invalid")]
    InvalidHash,

    /// Verification failed for a reason other than a wrong PIN.
    #[error("PIN verification failed")]
    VerifyFailed,

    /// A database error (lockout counters live on the `user` row).
    #[error("database error: {0}")]
    Db(#[from] shopcore_db::DbError),
}

/// The `Argon2id` instance for this spec's parameters.
///
/// # Errors
///
/// Returns [`PinError::HashFailed`] if the parameters are rejected.
fn argon2id() -> Result<Argon2<'static>, PinError> {
    let params = Params::new(ARGON2_M_COST_KIB, ARGON2_T_COST, ARGON2_P_COST, None)
        .map_err(|_| PinError::HashFailed)?;
    Ok(Argon2::new(Algorithm::Argon2id, Version::V0x13, params))
}

/// Hash a PIN with a fresh random salt. Returns the PHC string
/// (`$argon2id$v=19$m=19456,t=2,p=1$...`) stored on the `user` row.
///
/// # Errors
///
/// Returns [`PinError::HashFailed`] if hashing fails.
pub fn hash_pin(pin: &str) -> Result<String, PinError> {
    // `hash_password` generates a fresh random salt (password-hash 0.6
    // API — no `SaltString` handling needed at the call site).
    let hash = argon2id()?
        .hash_password(pin.as_bytes())
        .map_err(|_| PinError::HashFailed)?;
    Ok(hash.to_string())
}

/// Verify a PIN against a stored PHC hash. Returns `true` for a match,
/// `false` for a wrong PIN.
///
/// # Errors
///
/// Returns [`PinError::InvalidHash`] if the stored hash is not a valid
/// PHC string, and [`PinError::VerifyFailed`] for any other failure.
pub fn verify_pin_hash(pin: &str, phc: &str) -> Result<bool, PinError> {
    let parsed = PasswordHash::new(phc).map_err(|_| PinError::InvalidHash)?;
    match argon2id()?.verify_password(pin.as_bytes(), &parsed) {
        Ok(()) => Ok(true),
        Err(PhcError::PasswordInvalid) => Ok(false),
        Err(_) => Err(PinError::VerifyFailed),
    }
}

/// A dummy PHC hash, computed once on first use, used only to spend one
/// Argon2id verify for an unknown username (req. 8). The hash is of
/// `DUMMY_PIN`; the supplied PIN is verified against it and the result is
/// ignored by the caller (an unknown user always fails with
/// `InvalidCredentials`, exactly like a wrong PIN).
/// (`OnceLock::get_or_try_init` is still unstable, so the `Result` is
/// stored and a failed init is cached as `HashFailed` — the parameters
/// are fixed, so a failed init cannot succeed on retry.)
static DUMMY_HASH: std::sync::OnceLock<Result<String, PinError>> =
    std::sync::OnceLock::new();

/// The dummy hash, computing it once from `DUMMY_PIN` if needed.
///
/// # Errors
/// Returns [`PinError::HashFailed`] if the dummy hash cannot be built.
fn dummy_hash() -> Result<&'static str, PinError> {
    match DUMMY_HASH.get_or_init(|| hash_pin(DUMMY_PIN)) {
        Ok(hash) => Ok(hash.as_str()),
        Err(_) => Err(PinError::HashFailed),
    }
}

/// Test/diagnostic hook: how many Argon2 verifies have run against the
/// dummy hash (the unknown-user path). Always compiled — integration tests
/// link this crate without `cfg(test)`, so a `#[cfg(test)]`-gated counter
/// would be invisible to them — but only tests read it. One atomic
/// increment per unknown-user verify, negligible next to the Argon2 run
/// itself.
static DUMMY_HASH_VERIFY_COUNT: std::sync::atomic::AtomicUsize =
    std::sync::atomic::AtomicUsize::new(0);

/// How many Argon2 verifies have run against the dummy hash (test hook).
#[must_use]
pub fn dummy_hash_verify_count() -> usize {
    DUMMY_HASH_VERIFY_COUNT.load(std::sync::atomic::Ordering::SeqCst)
}

/// Reset the dummy-hash verify counter (test hook; call before a
/// structural assertion so the count is exact).
pub fn reset_dummy_hash_verify_count() {
    DUMMY_HASH_VERIFY_COUNT.store(0, std::sync::atomic::Ordering::SeqCst);
}

/// Verify a PIN against the dummy hash (the unknown-user path in
/// `verify_pin`). Returns the verify result — the caller ignores it (an
/// unknown user always fails with `InvalidCredentials`). Exactly one
/// Argon2 verify per call, so the timing does not reveal whether the user
/// exists.
///
/// # Errors
/// Returns [`PinError::HashFailed`] if the dummy hash cannot be built,
/// [`PinError::InvalidHash`] if the dummy hash is corrupt (it is built by
/// us, so this cannot happen in practice), and
/// [`PinError::VerifyFailed`] if the verify itself fails.
pub(crate) fn verify_against_dummy_hash(pin: &str) -> Result<bool, PinError> {
    let hash = dummy_hash()?;
    DUMMY_HASH_VERIFY_COUNT.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    verify_pin_hash(pin, hash)
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]
mod tests {
    use super::*;

    #[test]
    fn hash_and_verify_roundtrip() {
        let hash = hash_pin("1234").unwrap();
        assert!(hash.starts_with("$argon2id$v=19$m=19456,t=2,p=1$"));
        assert!(verify_pin_hash("1234", &hash).unwrap());
        assert!(!verify_pin_hash("0000", &hash).unwrap());
        assert!(!verify_pin_hash("12345", &hash).unwrap());
    }

    #[test]
    fn invalid_stored_hash_is_an_error_not_a_mismatch() {
        assert!(matches!(
            verify_pin_hash("1234", "not-a-phc-string"),
            Err(PinError::InvalidHash)
        ));
    }

    /// The 15x debug speedup from `[profile.dev.package.argon2]`
    /// opt-level 3 is compiler optimization only: the Argon2 parameters
    /// are exactly the OWASP minimum (19 MiB memory, 2 iterations,
    /// 1 lane). Assert on the PRODUCED hash's parameters (not just the
    /// constants), so a parameter change is caught even if the constants
    /// are edited.
    #[test]
    fn argon2_parameters_are_the_owasp_minimum() {
        assert_eq!(ARGON2_M_COST_KIB, 19_456);
        assert_eq!(ARGON2_T_COST, 2);
        assert_eq!(ARGON2_P_COST, 1);
        let hash = hash_pin("1234").unwrap();
        let parsed = PasswordHash::new(&hash).unwrap();
        assert_eq!(parsed.algorithm.as_str(), "argon2id");
        assert_eq!(parsed.params.to_string(), "m=19456,t=2,p=1");
    }

    /// Measures the `Argon2id` verification time with the OWASP-minimum
    /// parameters and prints it (the value goes into ADR-032). The bound
    /// is profile-aware: debug codegen makes `Argon2id` ~30x slower
    /// (1.7 s on the dev VM) and is not the shipping artifact. Release
    /// (the till) must verify in under 1 s; debug gets a sanity bound
    /// only, so a loaded CI runner does not flake it.
    #[test]
    fn measure_verify_time() {
        let hash = hash_pin("1234").unwrap();
        // Warm-up (one run) so the measured runs are steady-state.
        assert!(verify_pin_hash("1234", &hash).unwrap());
        let runs = 5;
        let start = std::time::Instant::now();
        for _ in 0..runs {
            assert!(verify_pin_hash("1234", &hash).unwrap());
        }
        let mean_ms = start.elapsed().as_secs_f64() * 1000.0 / f64::from(runs);
        println!("argon2id verify mean over {runs} runs: {mean_ms:.1} ms");
        let bound_ms = if cfg!(debug_assertions) {
            5000.0
        } else {
            1000.0
        };
        assert!(
            mean_ms < bound_ms,
            "verify took {mean_ms:.1} ms, over the {bound_ms:.0} ms bound"
        );
    }
}
