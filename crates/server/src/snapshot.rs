//! Snapshot build (spec 005): a zstd-compressed SQLite file of the branch's
//! catalog + stock balances, with a SHA-256 checksum and the cursor
//! (server sequence) it was taken at.
//!
//! Scope (spec 005, amendment 1 of the approval): the spike snapshot covers
//! catalog + stock balances. Customers and settings reuse the same
//! mechanism and are deferred. The catalog here is the set of SKUs the
//! server has seen (via pushed movements and price entries); the full
//! 500k-SKU catalog from spec 002 is the manual-run case, not the test.

use std::path::{Path, PathBuf};

use rusqlite::Connection;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row};
use uuid::Uuid;

use crate::error::ServerError;

/// What a built snapshot is: where it lives, its checksum, and the cursor.
#[derive(Debug, Clone)]
pub struct SnapshotInfo {
    /// Path to the `.zst` file (served by the snapshot endpoint).
    pub path: PathBuf,
    /// SHA-256 of the compressed file, lowercase hex.
    pub sha256_hex: String,
    /// Size of the compressed file in bytes.
    pub size_bytes: u64,
    /// Server sequence the snapshot was taken at (the pull cursor to
    /// continue from — the handoff point, no gap or overlap).
    pub cursor_sequence: i64,
}

/// Build a snapshot for one tenant+branch into `out_dir`.
///
/// The SQLite file has three tables: `sku` (id, code), `stock_balance`
/// (`branch_id`, `sku_id`, `qty_milli`), and `snapshot_cursor` (sequence).
/// It is then zstd-compressed; the returned `SnapshotInfo` carries the
/// SHA-256 of the compressed bytes and the cursor sequence.
///
/// # Errors
/// Returns `ServerError` for any Postgres, SQLite, or I/O failure.
pub async fn build_snapshot(
    pool: &PgPool,
    tenant_id: Uuid,
    branch_id: Uuid,
    out_dir: &Path,
) -> Result<SnapshotInfo, ServerError> {
    // The cursor: the server's current per-tenant sequence.
    let seq_row =
        sqlx::query("SELECT last_sequence FROM server_sequence WHERE tenant_id = $1::uuid")
            .bind(tenant_id.to_string())
            .fetch_optional(pool)
            .await?;
    let cursor_sequence: i64 = seq_row
        .and_then(|r| r.try_get("last_sequence").ok())
        .unwrap_or(0);

    // Balances for this branch.
    let balance_rows = sqlx::query(
        "SELECT sku_id::text AS sku_id, qty_milli FROM stock_balance
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid ORDER BY sku_id",
    )
    .bind(tenant_id.to_string())
    .bind(branch_id.to_string())
    .fetch_all(pool)
    .await?;
    // SKUs the server knows (from balances + price entries).
    let price_rows = sqlx::query(
        "SELECT DISTINCT sku_id::text AS sku_id FROM price_list_entry
         WHERE tenant_id = $1::uuid AND branch_id = $2::uuid",
    )
    .bind(tenant_id.to_string())
    .bind(branch_id.to_string())
    .fetch_all(pool)
    .await?;

    std::fs::create_dir_all(out_dir)?;
    let db_path = out_dir.join(format!("snapshot-{tenant_id}-{branch_id}.db"));
    let zst_path = out_dir.join(format!("snapshot-{tenant_id}-{branch_id}.db.zst"));
    let _ = std::fs::remove_file(&db_path);
    let _ = std::fs::remove_file(&zst_path);

    {
        let mut conn = Connection::open(&db_path)?;
        let tx = conn.transaction()?;
        tx.execute_batch(
            "CREATE TABLE sku (id TEXT PRIMARY KEY, code TEXT NOT NULL);
             CREATE TABLE stock_balance (branch_id TEXT NOT NULL, sku_id TEXT NOT NULL, qty_milli INTEGER NOT NULL, PRIMARY KEY (branch_id, sku_id));
             CREATE TABLE snapshot_cursor (sequence INTEGER NOT NULL);",
        )?;
        // SKUs from balances and price entries (deduped by the PK).
        let mut seen = std::collections::HashSet::new();
        for row in &balance_rows {
            let sku_id: Uuid = crate::parse_uuid(&row.try_get::<String, _>("sku_id")?)?;
            if seen.insert(sku_id) {
                tx.execute(
                    "INSERT OR IGNORE INTO sku (id, code) VALUES (?1, ?2)",
                    rusqlite::params![sku_id.to_string(), format!("SKU-{sku_id}")],
                )?;
            }
        }
        for row in &price_rows {
            let sku_id: Uuid = crate::parse_uuid(&row.try_get::<String, _>("sku_id")?)?;
            if seen.insert(sku_id) {
                tx.execute(
                    "INSERT OR IGNORE INTO sku (id, code) VALUES (?1, ?2)",
                    rusqlite::params![sku_id.to_string(), format!("SKU-{sku_id}")],
                )?;
            }
        }
        for row in &balance_rows {
            let sku_id: Uuid = crate::parse_uuid(&row.try_get::<String, _>("sku_id")?)?;
            let qty_milli: i64 = row.try_get("qty_milli")?;
            tx.execute(
                "INSERT INTO stock_balance (branch_id, sku_id, qty_milli) VALUES (?1, ?2, ?3)",
                rusqlite::params![branch_id.to_string(), sku_id.to_string(), qty_milli],
            )?;
        }
        tx.execute(
            "INSERT INTO snapshot_cursor (sequence) VALUES (?1)",
            rusqlite::params![cursor_sequence],
        )?;
        tx.commit()?;
    }

    // Compress with zstd (level 3, the default) and hash the compressed bytes.
    let raw = std::fs::read(&db_path)?;
    let compressed = zstd::stream::encode_all(std::io::Cursor::new(raw), 3)?;
    std::fs::write(&zst_path, &compressed)?;
    let _ = std::fs::remove_file(&db_path);
    let mut hasher = Sha256::new();
    hasher.update(&compressed);
    let sha256_hex = to_hex(&hasher.finalize());
    let size_bytes = compressed.len() as u64;

    Ok(SnapshotInfo {
        path: zst_path,
        sha256_hex,
        size_bytes,
        cursor_sequence,
    })
}

/// Lowercase hex encoding (avoids depending on the `hex` crate).
fn to_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(bytes.len() * 2);
    for &b in bytes {
        if let (Some(&hi), Some(&lo)) = (HEX.get((b >> 4) as usize), HEX.get((b & 0x0f) as usize)) {
            out.push(hi as char);
            out.push(lo as char);
        }
    }
    out
}
