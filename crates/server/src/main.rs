//! `shopcore-server` binary (spec 005 sync spike).
//!
//! Usage: `DATABASE_URL=postgres://... SHOPCORE_SNAPSHOT_DIR=/tmp/snaps
//! shopcore-server [bind-addr]` (default bind: `127.0.0.1:8080`).
//! Runs migrations on start, then serves the sync API.

use std::path::PathBuf;

use shopcore_server::{ServerState, create_app, run_migrations};
use sqlx::PgPool;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let database_url = std::env::var("DATABASE_URL").map_err(
        |_| "DATABASE_URL is not set (e.g. postgres://shopcore:shopcore@localhost:5432/shopcore)",
    )?;
    let snapshot_dir = std::env::var("SHOPCORE_SNAPSHOT_DIR")
        .unwrap_or_else(|_| "/tmp/shopcore-snapshots".to_string());
    let bind = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "127.0.0.1:8080".to_string());

    let pool = PgPool::connect(&database_url).await?;
    run_migrations(&pool).await?;
    let state = ServerState {
        pool,
        snapshot_dir: PathBuf::from(snapshot_dir),
    };
    let app = create_app(state);
    let listener = tokio::net::TcpListener::bind(&bind).await?;
    eprintln!("shopcore-server listening on {bind}");
    axum::serve(listener, app).await?;
    Ok(())
}
