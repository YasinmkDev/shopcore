//! `ShopCore` protocol crate: shared request/response/event types and the
//! sync wire format (see `context/03-architecture.md`).
//!
//! `TypeScript` types are generated from these Rust types with `ts-rs`
//! (ADR-013) by `just gen-types` into `packages/protocol-ts`. Never
//! hand-edit the generated file.
//!
//! Spec 000: the only type is `Pong`, proving the generation round-trip.

use serde::{Deserialize, Serialize};
use ts_rs::TS;

pub mod checkout;
pub mod sync;
pub mod time;

pub use checkout::{
    CartLineDto, CartTotalsDto, CartViewDto, HardwareStatusPayload, SaleCompletedPayload,
    SampleBarcodeDto, SessionDto, SkuHitDto, StatusDto,
};
pub use sync::{Cursor, OutboxRow, PriceListEntry, PullResponse, SyncAck, SyncBatch, Tombstone};
pub use time::{from_iso8601, to_iso8601};

/// Response to the `ping` command. Proves the UI → Rust round-trip.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, TS)]
pub struct Pong {
    /// Human-readable confirmation message.
    pub message: String,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pong_type_exports_message_field() {
        assert!(matches!(
            Pong::export_to_string(&ts_rs::Config::default()),
            Ok(s) if s.contains("message: string")
        ));
    }
}
