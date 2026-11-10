//! `ShopCore` sync wire types (spec 005).
//!
//! The sync protocol (ADR-009, custom outbox/inbox): the till pushes batches
//! of outbox rows to the server; the server applies them idempotently and
//! returns an ack with its per-tenant monotonic sequence; the till pulls
//! per-stream changes by cursor. All types are versioned (`version` field)
//! so the wire format can evolve without breaking old tills.
//!
//! Note: these types do NOT derive `ts-rs::TS`. They are server/client
//! wire types, not UI types — the UI never consumes them, and `Uuid` /
//! `serde_json::Value` have no TS representation. The `Pong` type in
//! `lib.rs` is the one that proves the TS generation round-trip.

use serde::{Deserialize, Serialize};
use uuid::Uuid;

/// One row from the till's outbox, as pushed to the server.
///
/// The `id` is the outbox row's own `UUIDv7` — it IS the idempotency key
/// (spec 005, amendment 7). It is stable across retries by construction,
/// so a retry re-sends the same row with the same key and the server's
/// `UNIQUE (tenant_id, idempotency_key)` constraint dedupes it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OutboxRow {
    /// Outbox row `UUIDv7`; also the idempotency key.
    pub id: Uuid,
    /// Entity type, e.g. `sale`, `sale_line`, `stock_movement`.
    pub entity: String,
    /// Stream this row belongs to, e.g. `sale`, `sale_line`, `stock_movement`.
    pub stream: String,
    /// Row payload as JSON (the row's columns).
    pub payload: serde_json::Value,
    /// Till-local creation time, milliseconds since epoch.
    pub created_at_ms: i64,
}

/// A batch of outbox rows pushed to the server in one request.
///
/// Limits (spec 005): at most 200 rows and at most ~256 KB of JSON.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncBatch {
    /// Wire format version. Currently 1.
    pub version: u32,
    /// Tenant the rows belong to.
    pub tenant_id: Uuid,
    /// Branch the rows belong to.
    pub branch_id: Uuid,
    /// Rows in push order. The server applies them in this order.
    pub rows: Vec<OutboxRow>,
}

/// Acknowledgement for a pushed batch.
///
/// A replay of an already-applied batch returns the SAME ack (the existing
/// server sequence, no new row written) — see chaos test 9.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct SyncAck {
    /// Wire format version. Currently 1.
    pub version: u32,
    /// Idempotency keys the server accepted (applied or already applied).
    pub accepted: Vec<Uuid>,
    /// The server's per-tenant monotonic sequence after this batch.
    pub server_sequence: i64,
}

/// A per-stream pull cursor: "give me everything after this sequence".
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Cursor {
    /// Stream name, e.g. `price_list_entry`.
    pub stream: String,
    /// Last sequence the client has already applied (exclusive).
    pub sequence: i64,
}

/// One price-list entry on the pull stream.
///
/// Conflict rule: per-field last-write-wins by **server time**. The till
/// stores both the server time and its own local receipt time. Each
/// mutable field carries its own update timestamp so two edits to
/// different fields merge instead of clobbering each other (spec 005,
/// amendment 4): a newer `price_updated_at_ms` wins the price field,
/// a newer `effective_updated_at_ms` wins the effective-from field.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PriceListEntry {
    /// Entry `UUIDv7`.
    pub id: Uuid,
    /// Tenant the entry belongs to.
    pub tenant_id: Uuid,
    /// Branch the entry belongs to.
    pub branch_id: Uuid,
    /// SKU this price applies to.
    pub sku_id: Uuid,
    /// Price in minor units (`Money`, i64). Single currency per tenant
    /// in v1 (ADR-017).
    pub price_minor: i64,
    /// Server time the price was last updated, milliseconds since epoch.
    /// Per-field LWW timestamp for `price_minor`.
    pub price_updated_at_ms: i64,
    /// Currency code, e.g. `PKR`. Single currency per tenant in v1.
    pub currency: String,
    /// Server time this price takes effect from, milliseconds since epoch.
    pub effective_from_ms: i64,
    /// Server time the effective-from was last updated, milliseconds since
    /// epoch. Per-field LWW timestamp for `effective_from_ms`.
    pub effective_updated_at_ms: i64,
    /// Entry version (optimistic concurrency).
    pub version: i64,
    /// Deletion time, milliseconds since epoch; `None` means live.
    /// A set value is a tombstone.
    pub deleted_at_ms: Option<i64>,
}

/// A tombstone: a deleted row, so the pull side can delete its copy.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Tombstone {
    /// Id of the deleted row.
    pub id: Uuid,
    /// Deletion time, milliseconds since epoch.
    pub deleted_at_ms: i64,
}

/// Response to a pull request: rows after the cursor, plus the new cursor.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullResponse {
    /// Wire format version. Currently 1.
    pub version: u32,
    /// Stream this response is for.
    pub stream: String,
    /// Rows after the requested cursor, in sequence order.
    pub rows: Vec<serde_json::Value>,
    /// The cursor to use for the next pull (the last sequence returned).
    pub next_sequence: i64,
}

#[cfg(test)]
#[allow(clippy::unwrap_used, clippy::expect_used)]
mod tests {
    use super::*;

    #[test]
    fn batch_round_trips_through_json() {
        let row = OutboxRow {
            id: Uuid::now_v7(),
            entity: "sale".to_string(),
            stream: "sale".to_string(),
            payload: serde_json::json!({"total_minor": 1500}),
            created_at_ms: 1_790_000_000_000,
        };
        let batch = SyncBatch {
            version: 1,
            tenant_id: Uuid::now_v7(),
            branch_id: Uuid::now_v7(),
            rows: vec![row],
        };
        let json = serde_json::to_string(&batch).expect("serialize batch");
        let back: SyncBatch = serde_json::from_str(&json).expect("deserialize batch");
        assert_eq!(back, batch);
    }

    #[test]
    fn price_entry_round_trips_through_json() {
        let entry = PriceListEntry {
            id: Uuid::now_v7(),
            tenant_id: Uuid::now_v7(),
            branch_id: Uuid::now_v7(),
            sku_id: Uuid::now_v7(),
            price_minor: 2500,
            price_updated_at_ms: 1_790_000_000_000,
            currency: "PKR".to_string(),
            effective_from_ms: 1_790_000_000_000,
            effective_updated_at_ms: 1_790_000_000_000,
            version: 1,
            deleted_at_ms: None,
        };
        let json = serde_json::to_string(&entry).expect("serialize entry");
        let back: PriceListEntry = serde_json::from_str(&json).expect("deserialize entry");
        assert_eq!(back, entry);
    }
}
