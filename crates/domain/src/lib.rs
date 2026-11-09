//! `ShopCore` domain crate: pure business logic.
//!
//! This crate holds the money/quantity/rate newtypes, pricing, tax,
//! discounts, promotions, and stock rules from `context/02-domain-model.md`.
//!
//! Purity is an invariant (AGENTS.md): no I/O, no clock, no randomness, no
//! database, no network. Time, randomness, and IDs are injected via traits
//! defined here and implemented at the edges, so every rule is testable with
//! deterministic inputs. Spec 000 leaves this crate empty; the types and
//! rules land with the domain spec.
