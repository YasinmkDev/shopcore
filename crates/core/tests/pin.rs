//! PIN tests (spec 001b-1 req. 7–8): Argon2id verify, lockout, unknown-user
//! dummy work, manager approval, off-UI-thread verification.

#![allow(clippy::unwrap_used, clippy::expect_used, clippy::panic)]

mod common;

use std::time::{Duration, Instant};

use shopcore_core::permissions::has_permission;
use shopcore_core::pin::PinError;
use shopcore_core::users::{
    approve_by_manager, create_role, create_user, verify_pin, verify_pin_async,
};

use common::{NOW, TENANT, fresh_db, make_user};

#[test]
fn correct_pin_verifies_wrong_pin_rejects() {
    let db = fresh_db();
    make_user(&db, "cashier", "1234", &["sale.create"]);
    let user = verify_pin(&db, TENANT, "cashier", "1234", NOW).unwrap();
    assert_eq!(user.username, "cashier");
    assert!(matches!(
        verify_pin(&db, TENANT, "cashier", "9999", NOW),
        Err(PinError::InvalidCredentials)
    ));
}

#[test]
fn too_short_pin_is_rejected_at_creation() {
    let db = fresh_db();
    make_user(&db, "cashier", "1234", &["sale.create"]);
    assert!(matches!(
        create_user(&db, TENANT, "short", "12", None, NOW),
        Err(shopcore_core::CoreError::Pin(PinError::PinTooShort {
            min: 4
        }))
    ));
}

#[test]
fn lockout_after_five_failures_for_five_minutes_then_unlocks() {
    let db = fresh_db();
    make_user(&db, "cashier", "1234", &["sale.create"]);
    // Failures 1–4: plain invalid credentials, no event.
    for _ in 0..4 {
        assert!(matches!(
            verify_pin(&db, TENANT, "cashier", "9999", NOW),
            Err(PinError::InvalidCredentials)
        ));
    }
    // Failure 5: locked out, and the error carries the audit event.
    let err = verify_pin(&db, TENANT, "cashier", "9999", NOW).unwrap_err();
    let event = match err {
        PinError::LockedOut { until_ms, event } => {
            assert_eq!(until_ms, NOW + 5 * 60 * 1000);
            event.expect("lockout must carry the audit event")
        }
        other => panic!("expected LockedOut, got {other:?}"),
    };
    assert_eq!(event.username, "cashier");
    assert_eq!(event.tenant_id, TENANT);
    // Still locked one minute later — even the CORRECT pin is refused,
    // and a re-check emits no second event.
    assert!(matches!(
        verify_pin(&db, TENANT, "cashier", "1234", NOW + 60_000),
        Err(PinError::LockedOut {
            until_ms: _,
            event: None,
        })
    ));
    // After the lockout window, the correct PIN works again.
    let user = verify_pin(&db, TENANT, "cashier", "1234", NOW + 5 * 60 * 1000 + 1).unwrap();
    assert_eq!(user.username, "cashier");
}

#[test]
fn unknown_username_costs_a_dummy_hash_and_fails_like_a_wrong_pin() {
    let db = fresh_db();
    make_user(&db, "cashier", "1234", &["sale.create"]);
    // Both paths run one Argon2id verification. The unknown path must
    // not be suspiciously faster than a known-wrong PIN (which would
    // reveal whether the user exists). Coarse bound: >= 50%.
    let time_it = |f: &dyn Fn()| {
        let start = Instant::now();
        f();
        start.elapsed()
    };
    let known = time_it(&|| {
        let _ = verify_pin(&db, TENANT, "cashier", "9999", NOW);
    });
    let unknown = time_it(&|| {
        let _ = verify_pin(&db, TENANT, "nobody", "9999", NOW);
    });
    assert!(matches!(
        verify_pin(&db, TENANT, "nobody", "9999", NOW),
        Err(PinError::InvalidCredentials)
    ));
    assert!(
        unknown.as_millis() * 2 >= known.as_millis(),
        "unknown-user verify ({unknown:?}) was less than half the known-wrong-PIN verify ({known:?})"
    );
}

#[test]
fn manager_approval_requires_select_manager_plus_pin() {
    let db = fresh_db();
    let cashier_role = create_role(&db, TENANT, "Cashier", &["sale.create"], 500, NOW).unwrap();
    let manager_role = create_role(
        &db,
        TENANT,
        "Manager",
        &["sale.create", "sale.discount.bill"],
        2000,
        NOW,
    )
    .unwrap();
    create_user(&db, TENANT, "cashier", "1234", Some(&cashier_role.id), NOW).unwrap();
    create_user(&db, TENANT, "manager", "4321", Some(&manager_role.id), NOW).unwrap();
    // No named approver at all: rejected (PIN-only scanning is not an API).
    assert!(matches!(
        approve_by_manager(&db, TENANT, "", "4321", NOW),
        Err(PinError::ManagerNotSelected)
    ));
    // Named approver, wrong PIN: rejected.
    assert!(matches!(
        approve_by_manager(&db, TENANT, "manager", "0000", NOW),
        Err(PinError::InvalidCredentials)
    ));
    // Named manager + correct PIN: approved. The permission check is the
    // caller's job (001b-2's discount use case) — the roles prove the
    // seam: the manager has `sale.discount.bill`, the cashier does not.
    let approver = approve_by_manager(&db, TENANT, "manager", "4321", NOW).unwrap();
    assert_eq!(approver.username, "manager");
    assert!(has_permission(&manager_role, "sale.discount.bill"));
    assert!(!has_permission(&cashier_role, "sale.discount.bill"));
}

#[tokio::test(flavor = "multi_thread", worker_threads = 1)]
async fn verify_pin_async_runs_off_the_runtime_thread() {
    let db = fresh_db();
    make_user(&db, "cashier", "1234", &["sale.create"]);
    // The verify runs on the blocking pool, not the runtime worker. While
    // it is in flight (~1.7 s in debug), a 50 ms timer task on the single
    // worker must still fire on time — it could not if the verify blocked
    // the worker thread.
    let verify = tokio::spawn(verify_pin_async(
        db,
        TENANT.to_string(),
        "cashier".to_string(),
        "1234".to_string(),
        NOW,
    ));
    let timer_start = Instant::now();
    tokio::time::sleep(Duration::from_millis(50)).await;
    let timer_elapsed = timer_start.elapsed();
    let user = verify.await.unwrap().unwrap();
    assert_eq!(user.username, "cashier");
    assert!(
        timer_elapsed < Duration::from_millis(500),
        "timer task took {timer_elapsed:?} while the verify was in flight — the verify blocked the runtime thread"
    );
}
