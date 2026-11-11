//! `ShopCore` domain crate: pure business logic.
//!
//! This crate holds the money/quantity/rate newtypes, pricing, tax,
//! discounts, promotions, and stock rules from `context/02-domain-model.md`.
//!
//! Purity is an invariant (AGENTS.md): no I/O, no clock, no randomness, no
//! database, no network. Time, randomness, and IDs are injected via traits
//! defined here and implemented at the edges, so every rule is testable with
//! deterministic inputs.
//!
//! Spec 005 lands the first real type: [`Money`] (checked i64 minor units).
//! Spec 001a lands `Qty`, `Rate`, `UnitPrice`/`UnitCost`, line pricing,
//! tax, discounts, cash rounding, change, and receipt numbers. Promotions
//! are a later spec.

pub mod cash;
pub mod error;
pub mod money;
pub mod price;
pub mod pricing;
pub mod qty;
pub mod rate;
pub mod receipt;
pub(crate) mod rounding;

pub use cash::{CashRounding, CashRoundingResult, compute_change};
pub use error::DomainError;
pub use money::Money;
pub use price::{UnitCost, UnitPrice};
pub use pricing::{
    BillDiscount, CartResult, LineDiscount, LineInput, LineResult, TaxBreakdown, TaxSpec,
    allocate_bill_discount, bill_discount_total, line_discount_amount, line_gross, price_cart,
    tax_for_line,
};
pub use qty::Qty;
pub use rate::Rate;
pub use receipt::next_receipt_number;
