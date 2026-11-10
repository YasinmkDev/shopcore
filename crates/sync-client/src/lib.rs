//! `ShopCore` sync client (spec 005): till-side push/pull and snapshot
//! download over HTTP.
//!
//! This crate is deliberately SEPARATE from the desktop crate (amendment 2):
//! it is the only crate that depends on `reqwest` (and therefore
//! `aws-lc-rs`), so the shipped desktop binary contains no HTTP-client /
//! TLS code. The desktop never depends on this crate.

use std::io::Write as _;
use std::path::Path;

use reqwest::header::{HeaderMap, HeaderValue, RANGE};
use sha2::{Digest, Sha256};
use shopcore_protocol::{OutboxRow, PullResponse, SyncAck, SyncBatch};
use thiserror::Error;
use uuid::Uuid;

/// Errors the sync client can return.
#[derive(Debug, Error)]
pub enum ClientError {
    /// HTTP / transport error.
    #[error("http error: {0}")]
    Http(#[from] reqwest::Error),

    /// The server returned a non-success status.
    #[error("server error {status}: {body}")]
    Server {
        /// HTTP status code.
        status: u16,
        /// Response body.
        body: String,
    },

    /// The snapshot checksum did not match.
    #[error("snapshot checksum mismatch: expected {expected}, got {actual}")]
    ChecksumMismatch {
        /// Expected SHA-256 hex.
        expected: String,
        /// Actual SHA-256 hex.
        actual: String,
    },

    /// SQLite (till outbox) error.
    #[error("till db error: {0}")]
    TillDb(#[from] rusqlite::Error),

    /// I/O error (snapshot file).
    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    /// JSON error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

/// The till-side sync client.
#[derive(Debug, Clone)]
pub struct SyncClient {
    /// Server base URL, e.g. `http://127.0.0.1:8080`.
    base_url: String,
    /// reqwest client (rustls, no native-tls/OpenSSL).
    http: reqwest::Client,
}

impl SyncClient {
    /// Build a client for the given server base URL.
    ///
    /// # Errors
    /// Returns `ClientError::Http` if the reqwest client cannot be built.
    pub fn new(base_url: impl Into<String>) -> Result<SyncClient, ClientError> {
        let http = reqwest::Client::builder()
            .user_agent("shopcore-sync-client/0.1.0")
            .build()?;
        Ok(SyncClient {
            base_url: base_url.into(),
            http,
        })
    }

    /// Push one batch; returns the server's ack.
    ///
    /// # Errors
    /// Returns `ClientError::Server` on a non-success status, and
    /// `ClientError::Http` on a transport failure.
    pub async fn push(&self, batch: &SyncBatch) -> Result<SyncAck, ClientError> {
        let url = format!("{}/sync/push", self.base_url);
        let resp = self.http.post(url).json(batch).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server {
                status: status.as_u16(),
                body,
            });
        }
        Ok(resp.json::<SyncAck>().await?)
    }

    /// Pull one stream after a cursor.
    ///
    /// # Errors
    /// Returns `ClientError::Server` on a non-success status, and
    /// `ClientError::Http` on a transport failure.
    pub async fn pull(
        &self,
        tenant_id: Uuid,
        branch_id: Uuid,
        stream: &str,
        cursor: i64,
    ) -> Result<PullResponse, ClientError> {
        let url = format!(
            "{}/sync/pull?stream={stream}&cursor={cursor}&tenant_id={tenant_id}&branch_id={branch_id}",
            self.base_url
        );
        let resp = self.http.get(url).send().await?;
        let status = resp.status();
        if !status.is_success() {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server {
                status: status.as_u16(),
                body,
            });
        }
        Ok(resp.json::<PullResponse>().await?)
    }

    /// Download a snapshot with Range resume, verifying the SHA-256.
    ///
    /// If `dest` already exists, the download resumes from its current
    /// size (the server must support Range, which tower-http `ServeFile`
    /// does). `expected_sha256_hex` is the checksum the server published
    /// for the snapshot. Returns the final size in bytes.
    ///
    /// # Errors
    /// Returns `ClientError::ChecksumMismatch` when the downloaded bytes
    /// do not hash to the expected value, `ClientError::Server` on a
    /// non-success status, and `ClientError::Http` on a transport failure.
    pub async fn download_snapshot(
        &self,
        tenant_id: Uuid,
        branch_id: Uuid,
        dest: &Path,
        expected_sha256_hex: &str,
    ) -> Result<u64, ClientError> {
        let url = format!("{}/snapshot/{tenant_id}/{branch_id}", self.base_url);
        let existing = std::fs::metadata(dest).map_or(0, |m| m.len());
        let mut headers = HeaderMap::new();
        if existing > 0 {
            headers.insert(
                RANGE,
                HeaderValue::from_str(&format!("bytes={existing}-"))
                    .unwrap_or_else(|_| HeaderValue::from_static("bytes=0-")),
            );
        }
        let resp = self.http.get(&url).headers(headers).send().await?;
        let status = resp.status();
        if !(status.is_success() || status.as_u16() == 206) {
            let body = resp.text().await.unwrap_or_default();
            return Err(ClientError::Server {
                status: status.as_u16(),
                body,
            });
        }
        let bytes = resp.bytes().await?;
        // Append (resume) or create.
        let mut file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dest)?;
        file.write_all(&bytes)?;
        drop(file);
        // Verify the whole file against the published checksum.
        let all = std::fs::read(dest)?;
        let mut hasher = Sha256::new();
        hasher.update(&all);
        let actual = to_hex(&hasher.finalize());
        if actual != expected_sha256_hex {
            return Err(ClientError::ChecksumMismatch {
                expected: expected_sha256_hex.to_string(),
                actual,
            });
        }
        Ok(all.len() as u64)
    }
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

/// Create the till-side outbox table if it does not exist (spec 005,
/// fix 1: the full `context/04` column set).
///
/// # Errors
/// Returns `ClientError::TillDb` for any SQLite failure.
pub fn ensure_outbox_schema(conn: &rusqlite::Connection) -> Result<(), ClientError> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS outbox (
            id TEXT PRIMARY KEY,
            tenant_id TEXT NOT NULL,
            branch_id TEXT NOT NULL,
            entity TEXT NOT NULL,
            stream TEXT NOT NULL,
            payload TEXT NOT NULL,
            idempotency_key TEXT NOT NULL,
            created_at INTEGER NOT NULL,
            updated_at INTEGER NOT NULL,
            deleted_at INTEGER,
            version INTEGER NOT NULL DEFAULT 1,
            sent_at INTEGER,
            attempts INTEGER NOT NULL DEFAULT 0
        );
        CREATE INDEX IF NOT EXISTS idx_outbox_unsent ON outbox(sent_at) WHERE sent_at IS NULL;",
    )?;
    Ok(())
}

/// Read up to `limit` unsent outbox rows, oldest first.
///
/// # Errors
/// Returns `ClientError::TillDb` for any SQLite failure.
pub fn read_unsent_outbox(
    conn: &rusqlite::Connection,
    limit: usize,
) -> Result<Vec<OutboxRow>, ClientError> {
    let mut stmt = conn.prepare(
        "SELECT id, entity, stream, payload, created_at FROM outbox
         WHERE sent_at IS NULL ORDER BY created_at ASC LIMIT ?1",
    )?;
    let limit_i64 = i64::try_from(limit).unwrap_or(i64::MAX);
    let rows = stmt.query_map(rusqlite::params![limit_i64], |row| {
        let id_str: String = row.get(0)?;
        let entity: String = row.get(1)?;
        let stream: String = row.get(2)?;
        let payload_str: String = row.get(3)?;
        let created_at_ms: i64 = row.get(4)?;
        Ok((id_str, entity, stream, payload_str, created_at_ms))
    })?;
    let mut out = Vec::new();
    for r in rows {
        let (id_str, entity, stream, payload_str, created_at_ms) = r?;
        let id = Uuid::parse_str(&id_str).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(0, rusqlite::types::Type::Text, Box::new(e))
        })?;
        let payload: serde_json::Value = serde_json::from_str(&payload_str)?;
        out.push(OutboxRow {
            id,
            entity,
            stream,
            payload,
            created_at_ms,
        });
    }
    Ok(out)
}

/// Mark outbox rows as sent (after a successful ack).
///
/// # Errors
/// Returns `ClientError::TillDb` for any SQLite failure.
pub fn mark_sent(
    conn: &rusqlite::Connection,
    ids: &[Uuid],
    sent_at_ms: i64,
) -> Result<(), ClientError> {
    let mut stmt =
        conn.prepare("UPDATE outbox SET sent_at = ?1, attempts = attempts + 1 WHERE id = ?2")?;
    for id in ids {
        stmt.execute(rusqlite::params![sent_at_ms, id.to_string()])?;
    }
    Ok(())
}
