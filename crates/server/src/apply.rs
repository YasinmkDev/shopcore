//! Batch apply (spec 005): idempotent, transactional, per-tenant ordered.
//!
//! Rules:
//! - One Postgres transaction per batch. The tenant's sequence row is locked
//!   (`SELECT ... FOR UPDATE`), serializing batches per tenant.
//! - Idempotency is enforced by the DB (amendment 5): `applied_push` has
//!   PRIMARY KEY (`tenant_id`, `idempotency_key`). A replay inserts nothing and
//!   applies no effects; the ack returns the existing sequence.
//! - Stock balances are derived in the same transaction as the movements
//!   (amendment 8): insert the movement, then upsert the balance by the
//!   same `qty_milli`. The balance is never written outside this transaction.
//! - Per-field LWW for price entries (amendment 4): each mutable field
//!   carries its own update timestamp; a field is only overwritten by a
//!   newer timestamp for that field.

use shopcore_domain::Money;
use shopcore_protocol::{PriceListEntry, SyncAck, SyncBatch};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::ServerError;

/// Maximum rows per batch (spec 005).
pub const MAX_BATCH_ROWS: usize = 200;

/// Apply a pushed batch. Returns the ack: accepted ids + the server's
/// per-tenant sequence after the batch.
///
/// # Errors
/// Returns `ServerError::InvalidBatch` for a wrong version or too many
/// rows, and `ServerError::Database` for any Postgres failure.
pub async fn apply_batch(pool: &PgPool, batch: &SyncBatch) -> Result<SyncAck, ServerError> {
    if batch.version != 1 {
        return Err(ServerError::InvalidBatch(format!(
            "unsupported batch version: {}",
            batch.version
        )));
    }
    if batch.rows.len() > MAX_BATCH_ROWS {
        return Err(ServerError::InvalidBatch(format!(
            "batch has {} rows, max is {MAX_BATCH_ROWS}",
            batch.rows.len()
        )));
    }

    let mut tx = pool.begin().await?;
    // Lock the tenant's sequence row for the duration of the batch.
    // Creates the row on first use.
    sqlx::query(
        "INSERT INTO server_sequence (tenant_id, last_sequence) VALUES ($1::uuid, 0)
         ON CONFLICT (tenant_id) DO NOTHING",
    )
    .bind(batch.tenant_id.to_string())
    .execute(&mut *tx)
    .await?;
    let row = sqlx::query(
        "SELECT last_sequence FROM server_sequence WHERE tenant_id = $1::uuid::uuid FOR UPDATE",
    )
    .bind(batch.tenant_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let mut last_sequence: i64 = row.try_get("last_sequence")?;

    let mut accepted: Vec<Uuid> = Vec::with_capacity(batch.rows.len());
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));

    for outbox_row in &batch.rows {
        // The idempotency key IS the outbox row's own `UUIDv7` (amendment 7).
        // ON CONFLICT DO NOTHING + RETURNING tells us whether this row is
        // new (inserted) or a replay (already present).
        let inserted = sqlx::query(
            "INSERT INTO applied_push
             (tenant_id, idempotency_key, entity, stream, payload, server_sequence, applied_at_ms)
             VALUES ($1::uuid, $2::uuid, $3, $4, $5::jsonb, $6, $7)
             ON CONFLICT (tenant_id, idempotency_key) DO NOTHING
             RETURNING server_sequence",
        )
        .bind(batch.tenant_id.to_string())
        .bind(outbox_row.id.to_string())
        .bind(&outbox_row.entity)
        .bind(&outbox_row.stream)
        // Bind the payload as JSON text; Postgres casts text → JSONB.
        // (sqlx's `json` feature is not enabled — it pulls sqlx-sqlite,
        // which conflicts with rusqlite's libsqlite3-sys; see Cargo.toml.)
        .bind(outbox_row.payload.to_string())
        .bind(last_sequence + 1)
        .bind(now_ms)
        .fetch_optional(&mut *tx)
        .await?;

        if inserted.is_none() {
            // Replay: already applied. No new row, no new sequence, no
            // effects. Still accepted (the ack lists it).
            accepted.push(outbox_row.id);
            continue;
        }

        // New row: consume the next sequence number and apply its effects.
        last_sequence += 1;
        apply_row_effects(&mut tx, batch, outbox_row, last_sequence).await?;
        accepted.push(outbox_row.id);
    }

    sqlx::query("UPDATE server_sequence SET last_sequence = $1 WHERE tenant_id = $2::uuid")
        .bind(last_sequence)
        .bind(batch.tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;

    Ok(SyncAck {
        version: 1,
        accepted,
        server_sequence: last_sequence,
    })
}

/// Apply the effects of one newly-inserted outbox row, inside the batch
/// transaction. Dispatches on the row's entity.
async fn apply_row_effects(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    batch: &SyncBatch,
    row: &shopcore_protocol::OutboxRow,
    sequence: i64,
) -> Result<(), ServerError> {
    // Only `stock_movement` has server-side effects in the spike.
    // `sale`, `sale_line`, and any unknown entity are stored in
    // applied_push (already done); their totals are checked by the
    // revalidation stub, not here. (A future entity with its own
    // effects gets its own `if` branch here.)
    if row.entity.as_str() != "stock_movement" {
        return Ok(());
    }
    // Payload: { sku_id, qty_milli, occurred_at_ms }.
    let sku_id: Uuid = serde_json::from_value(
        row.payload
            .get("sku_id")
            .cloned()
            .unwrap_or(serde_json::Value::Null),
    )
    .map_err(|e| ServerError::InvalidBatch(format!("bad sku_id in movement: {e}")))?;
    let qty_milli: i64 = row
        .payload
        .get("qty_milli")
        .and_then(serde_json::Value::as_i64)
        .ok_or_else(|| ServerError::InvalidBatch("movement missing qty_milli".to_string()))?;
    let occurred_at_ms: i64 = row
        .payload
        .get("occurred_at_ms")
        .and_then(serde_json::Value::as_i64)
        .unwrap_or(row.created_at_ms);

    // Insert the movement; only update the balance if it is new.
    // (The applied_push dedupe already prevents a replay, but the
    // RETURNING check makes the balance update safe even if the
    // movement id is ever seen twice.)
    let inserted = sqlx::query(
        "INSERT INTO stock_movement
         (id, tenant_id, branch_id, sku_id, qty_milli, occurred_at_ms, server_sequence)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6, $7)
         ON CONFLICT (id) DO NOTHING RETURNING id",
    )
    .bind(row.id.to_string())
    .bind(batch.tenant_id.to_string())
    .bind(batch.branch_id.to_string())
    .bind(sku_id.to_string())
    .bind(qty_milli)
    .bind(occurred_at_ms)
    .bind(sequence)
    .fetch_optional(&mut **tx)
    .await?;
    if inserted.is_some() {
        // Same transaction, same rule as the till (amendment 8):
        // balance = sum of qty_milli over movements per branch+sku.
        sqlx::query(
            "INSERT INTO stock_balance (tenant_id, branch_id, sku_id, qty_milli)
             VALUES ($1::uuid, $2::uuid, $3::uuid, $4)
             ON CONFLICT (tenant_id, branch_id, sku_id)
             DO UPDATE SET qty_milli = stock_balance.qty_milli + EXCLUDED.qty_milli",
        )
        .bind(batch.tenant_id.to_string())
        .bind(batch.branch_id.to_string())
        .bind(sku_id.to_string())
        .bind(qty_milli)
        .execute(&mut **tx)
        .await?;
    }
    Ok(())
}

/// Apply (upsert) a price-list entry with per-field last-write-wins
/// (amendment 4). Each mutable field is only overwritten by a newer
/// timestamp for THAT field, so two edits to different fields merge.
///
/// # Errors
/// Returns `ServerError::Database` for any Postgres failure.
pub async fn apply_price_entry(pool: &PgPool, entry: &PriceListEntry) -> Result<(), ServerError> {
    let now_ms = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX));
    let mut tx = pool.begin().await?;
    // Lock the tenant sequence row and take the next sequence number.
    sqlx::query(
        "INSERT INTO server_sequence (tenant_id, last_sequence) VALUES ($1::uuid, 0)
         ON CONFLICT (tenant_id) DO NOTHING",
    )
    .bind(entry.tenant_id.to_string())
    .execute(&mut *tx)
    .await?;
    let row = sqlx::query(
        "SELECT last_sequence FROM server_sequence WHERE tenant_id = $1::uuid::uuid FOR UPDATE",
    )
    .bind(entry.tenant_id.to_string())
    .fetch_one(&mut *tx)
    .await?;
    let last_sequence: i64 = row.try_get("last_sequence")?;
    let sequence = last_sequence + 1;

    sqlx::query(
        "INSERT INTO price_list_entry
         (id, tenant_id, branch_id, sku_id, price_minor, price_updated_at_ms,
          currency, effective_from_ms, effective_updated_at_ms, version,
          deleted_at_ms, server_sequence, updated_at_ms)
         VALUES ($1::uuid, $2::uuid, $3::uuid, $4::uuid, $5, $6, $7, $8, $9, $10, $11, $12, $13)
         ON CONFLICT (id) DO UPDATE SET
           price_minor = CASE
             WHEN EXCLUDED.price_updated_at_ms > price_list_entry.price_updated_at_ms
             THEN EXCLUDED.price_minor ELSE price_list_entry.price_minor END,
           price_updated_at_ms = GREATEST(
             price_list_entry.price_updated_at_ms, EXCLUDED.price_updated_at_ms),
           effective_from_ms = CASE
             WHEN EXCLUDED.effective_updated_at_ms > price_list_entry.effective_updated_at_ms
             THEN EXCLUDED.effective_from_ms ELSE price_list_entry.effective_from_ms END,
           effective_updated_at_ms = GREATEST(
             price_list_entry.effective_updated_at_ms, EXCLUDED.effective_updated_at_ms),
           version = price_list_entry.version + 1,
           server_sequence = EXCLUDED.server_sequence,
           updated_at_ms = EXCLUDED.updated_at_ms",
    )
    .bind(entry.id.to_string())
    .bind(entry.tenant_id.to_string())
    .bind(entry.branch_id.to_string())
    .bind(entry.sku_id.to_string())
    .bind(entry.price_minor)
    .bind(entry.price_updated_at_ms)
    .bind(&entry.currency)
    .bind(entry.effective_from_ms)
    .bind(entry.effective_updated_at_ms)
    .bind(entry.version)
    .bind(entry.deleted_at_ms)
    .bind(sequence)
    .bind(now_ms)
    .execute(&mut *tx)
    .await?;
    sqlx::query("UPDATE server_sequence SET last_sequence = $1 WHERE tenant_id = $2::uuid")
        .bind(sequence)
        .bind(entry.tenant_id.to_string())
        .execute(&mut *tx)
        .await?;
    tx.commit().await?;
    Ok(())
}

/// Read one price-list entry back (for the LWW merge test).
///
/// # Errors
/// Returns `ServerError::Database` for any Postgres failure, and
/// `ServerError::InvalidBatch` when the entry does not exist.
pub async fn get_price_entry(pool: &PgPool, id: Uuid) -> Result<PriceListEntry, ServerError> {
    let row = sqlx::query(
        "SELECT id::text AS id, tenant_id::text AS tenant_id,
                branch_id::text AS branch_id, sku_id::text AS sku_id,
                price_minor, price_updated_at_ms,
                currency, effective_from_ms, effective_updated_at_ms, version,
                deleted_at_ms
         FROM price_list_entry WHERE id = $1::uuid",
    )
    .bind(id.to_string())
    .fetch_optional(pool)
    .await?
    .ok_or_else(|| ServerError::InvalidBatch(format!("price entry not found: {id}")))?;
    Ok(PriceListEntry {
        id: crate::parse_uuid(&row.try_get::<String, _>("id")?)?,
        tenant_id: crate::parse_uuid(&row.try_get::<String, _>("tenant_id")?)?,
        branch_id: crate::parse_uuid(&row.try_get::<String, _>("branch_id")?)?,
        sku_id: crate::parse_uuid(&row.try_get::<String, _>("sku_id")?)?,
        price_minor: row.try_get("price_minor")?,
        price_updated_at_ms: row.try_get("price_updated_at_ms")?,
        currency: row.try_get("currency")?,
        effective_from_ms: row.try_get("effective_from_ms")?,
        effective_updated_at_ms: row.try_get("effective_updated_at_ms")?,
        version: row.try_get("version")?,
        deleted_at_ms: row.try_get("deleted_at_ms")?,
    })
}

/// Read one stock balance back (for the balance-derivation test).
///
/// # Errors
/// Returns `ServerError::Database` for any Postgres failure.
pub async fn get_stock_balance(
    pool: &PgPool,
    tenant_id: Uuid,
    branch_id: Uuid,
    sku_id: Uuid,
) -> Result<i64, ServerError> {
    let row = sqlx::query(
        "SELECT qty_milli FROM stock_balance
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND sku_id = $3::uuid",
    )
    .bind(tenant_id.to_string())
    .bind(branch_id.to_string())
    .bind(sku_id.to_string())
    .fetch_optional(pool)
    .await?;
    Ok(row.and_then(|r| r.try_get("qty_milli").ok()).unwrap_or(0))
}

/// The revalidation stub (spec 005): recompute `sum(line totals) == sale
/// total` using the real `crates/domain` `Money` with checked ops.
/// Mismatches are flagged (returned), never altered.
///
/// # Errors
/// Returns `ServerError::RevalidationMismatch` when the lines do not sum
/// to the sale total, and `ServerError::RevalidationOverflow` if the sum
/// overflows i64.
pub fn revalidate_sale(
    line_totals_minor: &[i64],
    sale_total_minor: i64,
) -> Result<(), ServerError> {
    let mut sum = Money::ZERO;
    for &line in line_totals_minor {
        sum = sum
            .checked_add(Money::from_minor(line))
            .ok_or(ServerError::RevalidationOverflow)?;
    }
    if sum.minor() == sale_total_minor {
        Ok(())
    } else {
        Err(ServerError::RevalidationMismatch {
            expected: sale_total_minor,
            actual: sum.minor(),
        })
    }
}

/// Current per-tenant sequence (for the gap-detector test).
///
/// # Errors
/// Returns `ServerError::Database` for any Postgres failure.
pub async fn current_sequence(pool: &PgPool, tenant_id: Uuid) -> Result<i64, ServerError> {
    let row = sqlx::query("SELECT last_sequence FROM server_sequence WHERE tenant_id = $1::uuid")
        .bind(tenant_id.to_string())
        .fetch_optional(pool)
        .await?;
    Ok(row
        .and_then(|r| r.try_get("last_sequence").ok())
        .unwrap_or(0))
}

/// Count of rows in `applied_push` for a tenant (for the replay test).
///
/// # Errors
/// Returns `ServerError::Database` for any Postgres failure.
pub async fn applied_push_count(pool: &PgPool, tenant_id: Uuid) -> Result<i64, ServerError> {
    let row = sqlx::query("SELECT COUNT(*) AS n FROM applied_push WHERE tenant_id = $1::uuid")
        .bind(tenant_id.to_string())
        .fetch_one(pool)
        .await?;
    Ok(row.try_get("n")?)
}
