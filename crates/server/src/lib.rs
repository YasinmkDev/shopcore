//! `ShopCore` sync server (spec 005): Axum + PostgreSQL.
//!
//! Endpoints:
//! - `POST /sync/push` — apply a batch of outbox rows (idempotent).
//! - `GET /sync/pull?stream&cursor` — rows after a per-stream cursor.
//! - `GET /snapshot/:tenant/:branch` — the zstd snapshot file, served with
//!   HTTP Range (tower-http `ServeFile`) so a download can resume.
//! - `GET /health` — liveness.
//!
//! Auth is a static dev token (`dev-only`) for the spike only; real auth
//! lands before Phase 5.

pub mod apply;
pub mod error;
pub mod snapshot;

use std::path::PathBuf;
use std::sync::Arc;

use axum::Router;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::Json;
use axum::routing::{get, post};
use serde::Deserialize;
use shopcore_protocol::{PullResponse, SyncAck, SyncBatch};
use sqlx::PgPool;
use tower_http::services::ServeFile;
use uuid::Uuid;

use crate::error::ServerError;

pub use apply::{
    applied_push_count, apply_batch, apply_price_entry, current_sequence, get_price_entry,
    get_stock_balance, revalidate_sale,
};
pub use snapshot::{SnapshotInfo, build_snapshot};

/// Shared server state.
#[derive(Clone)]
pub struct ServerState {
    /// Postgres pool.
    pub pool: PgPool,
    /// Directory snapshots are built into and served from.
    pub snapshot_dir: PathBuf,
}

/// Run the migration SQL in `crates/server/migrations`.
///
/// The `migrate` feature is deliberately NOT enabled on sqlx (it pulls in
/// `sqlx-sqlite`, whose `libsqlite3-sys` conflicts with rusqlite's — both
/// link the native `sqlite3` library). Instead the SQL is executed
/// directly with `sqlx::raw_sql`, which is enough for the spike's single
/// migration file.
/// Run the server migrations (spec 005). Idempotent: every statement is
/// `IF NOT EXISTS`.
///
/// # Errors
/// Returns `ServerError::Database` if the SQL fails.
///
/// Concurrency: nextest runs each chaos test in its own process, so an
/// in-process `OnceLock` cannot serialize migrations — and on a fresh
/// database the parallel `CREATE TABLE IF NOT EXISTS` calls race (two
/// processes both pass the existence check, then both try to insert the
/// table's type into `pg_type` and one dies with a unique violation).
/// So the whole migration runs inside one transaction that first takes a
/// transaction-scoped advisory lock; the second process blocks until the
/// first commits, then sees the tables already exist.
pub async fn run_migrations(pool: &PgPool) -> Result<(), ServerError> {
    let sql = include_str!("../migrations/001_sync_tables.sql");
    let mut tx = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(727271)")
        .execute(&mut *tx)
        .await?;
    sqlx::raw_sql(sql).execute(&mut *tx).await?;
    tx.commit().await?;
    Ok(())
}

/// Build the Axum router for the given state.
pub fn create_app(state: ServerState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/sync/push", post(push_handler))
        .route("/sync/pull", get(pull_handler))
        .route("/snapshot/{tenant}/{branch}", get(snapshot_handler))
        .with_state(Arc::new(state))
}

async fn health() -> &'static str {
    "ok"
}

async fn push_handler(
    State(state): State<Arc<ServerState>>,
    Json(batch): Json<SyncBatch>,
) -> Result<Json<SyncAck>, (StatusCode, String)> {
    apply_batch(&state.pool, &batch)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

#[derive(Debug, Deserialize)]
pub struct PullQuery {
    /// Stream name, e.g. `price_list_entry`.
    pub stream: String,
    /// Last sequence the client has already applied (exclusive).
    pub cursor: i64,
    /// Tenant the rows belong to.
    pub tenant_id: Uuid,
    /// Branch the rows belong to.
    pub branch_id: Uuid,
}

async fn pull_handler(
    State(state): State<Arc<ServerState>>,
    Query(q): Query<PullQuery>,
) -> Result<Json<PullResponse>, (StatusCode, String)> {
    pull_stream(&state.pool, &q)
        .await
        .map(Json)
        .map_err(|e| (StatusCode::BAD_REQUEST, e.to_string()))
}

/// Pull rows for one stream after a cursor, in sequence order.
///
/// Currently serves the `price_list_entry` stream (the spike's pull
/// stream). Other streams return an empty page with the same cursor.
///
/// # Errors
/// Returns `ServerError::UnknownStream` for an unrecognized stream name,
/// and `ServerError::Database` for any Postgres failure.
pub async fn pull_stream(pool: &PgPool, q: &PullQuery) -> Result<PullResponse, ServerError> {
    match q.stream.as_str() {
        "price_list_entry" => {
            let rows = sqlx::query(
                "SELECT id::text AS id, tenant_id::text AS tenant_id,
                        branch_id::text AS branch_id, sku_id::text AS sku_id,
                        price_minor, price_updated_at_ms,
                        currency, effective_from_ms, effective_updated_at_ms, version,
                        deleted_at_ms, server_sequence
                 FROM price_list_entry
                 WHERE tenant_id = $1::uuid AND branch_id = $2::uuid AND server_sequence > $3
                 ORDER BY server_sequence ASC LIMIT 200",
            )
            .bind(q.tenant_id.to_string())
            .bind(q.branch_id.to_string())
            .bind(q.cursor)
            .fetch_all(pool)
            .await?;
            let mut next_sequence = q.cursor;
            let mut out: Vec<serde_json::Value> = Vec::with_capacity(rows.len());
            for row in rows {
                use sqlx::Row as _;
                let seq: i64 = row.try_get("server_sequence")?;
                if seq > next_sequence {
                    next_sequence = seq;
                }
                let entry = shopcore_protocol::PriceListEntry {
                    id: parse_uuid(&row.try_get::<String, _>("id")?)?,
                    tenant_id: parse_uuid(&row.try_get::<String, _>("tenant_id")?)?,
                    branch_id: parse_uuid(&row.try_get::<String, _>("branch_id")?)?,
                    sku_id: parse_uuid(&row.try_get::<String, _>("sku_id")?)?,
                    price_minor: row.try_get("price_minor")?,
                    price_updated_at_ms: row.try_get("price_updated_at_ms")?,
                    currency: row.try_get("currency")?,
                    effective_from_ms: row.try_get("effective_from_ms")?,
                    effective_updated_at_ms: row.try_get("effective_updated_at_ms")?,
                    version: row.try_get("version")?,
                    deleted_at_ms: row.try_get("deleted_at_ms")?,
                };
                out.push(serde_json::to_value(entry).map_err(|e| {
                    ServerError::InvalidBatch(format!("serialize price entry: {e}"))
                })?);
            }
            Ok(PullResponse {
                version: 1,
                stream: q.stream.clone(),
                rows: out,
                next_sequence,
            })
        }
        other => Err(ServerError::UnknownStream(other.to_string())),
    }
}

async fn snapshot_handler(
    State(state): State<Arc<ServerState>>,
    Path((tenant, branch)): Path<(Uuid, Uuid)>,
    req: axum::extract::Request,
) -> Result<axum::response::Response, (StatusCode, String)> {
    // Build (or rebuild) the snapshot on demand, then delegate the
    // response to tower-http `ServeFile` so HTTP Range is handled by
    // tower-http, not by hand (amendment 3). The request (with its Range
    // header) is passed through untouched.
    use tower::ServiceExt as _;
    let info = build_snapshot(&state.pool, tenant, branch, &state.snapshot_dir)
        .await
        .map_err(|e| (StatusCode::NOT_FOUND, e.to_string()))?;
    let serve_file = ServeFile::new(info.path);
    serve_file
        .oneshot(req)
        .await
        .map(|resp| resp.map(axum::body::Body::new))
        .map_err(|e| (StatusCode::INTERNAL_SERVER_ERROR, e.to_string()))
}

/// Parse a UUID read back from Postgres as text.
///
/// sqlx's `uuid` feature is deliberately NOT enabled: it pulls in
/// `sqlx-sqlite`, whose `libsqlite3-sys` conflicts with rusqlite's (both
/// link the native `sqlite3` library). UUIDs are therefore bound as text
/// (Postgres casts text → uuid) and read back via `::text` casts.
/// (See the `sqlx` note in `Cargo.toml`.)
///
/// # Errors
/// Returns `ServerError::InvalidBatch` when the text is not a valid UUID.
pub(crate) fn parse_uuid(s: &str) -> Result<Uuid, ServerError> {
    Uuid::parse_str(s).map_err(|e| ServerError::InvalidBatch(format!("bad uuid: {e}")))
}

/// The dev token for the spike (placeholder, never a real secret).
pub const DEV_TOKEN: &str = "dev-only";
