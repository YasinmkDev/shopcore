//! `ShopCore` core: application services / use cases, the `DB` actor, the sync
//! engine, and permissions.
//!
//! The UI talks to the core only through Tauri commands (embedded mode) or
//! HTTP (hub mode). It never touches the `DB`, filesystem, or hardware
//! directly. Cross-module calls go through use cases, not shared table
//! access (see `context/03-architecture.md`).
//!
//! Spec 000: this crate is a seam only — it exposes `ping()`. Spec 001a
//! lands the domain re-exports. Spec 001b-1 lands the foundation: the
//! `DB` actor (in `shopcore-db`), search/lookup, users/roles/PIN,
//! permissions, settings, and the dev seed.

pub mod audit;
pub mod cart;
pub mod core;
pub mod draft;
pub mod error;
pub mod permissions;
pub mod pin;
pub mod sale;
pub mod scratch;
pub mod search;
#[cfg(feature = "dev-seed")]
pub mod seed;
pub mod settings;
pub mod shift;
pub mod tax;
pub mod users;

pub use audit::{AuditEvent, verify_audit_chain};
pub use cart::{PricedCart, PricedCartLine, price_draft_cart};
pub use core::Core;
pub use draft::{DraftCart, DraftLine};
pub use error::CoreError;
pub use sale::{CompleteSaleCashInput, SaleCompleted, SaleError};
pub use scratch::ScratchDb;
pub use shopcore_protocol::Pong;

/// Proves the UI → core round-trip. Not business logic.
#[must_use]
pub fn ping() -> Pong {
    Pong {
        message: "pong".to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ping_returns_pong() {
        assert_eq!(ping().message, "pong");
    }
}
