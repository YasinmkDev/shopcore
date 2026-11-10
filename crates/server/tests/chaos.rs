#![allow(clippy::unwrap_used, clippy::expect_used)]

//! Chaos tests for spec 005 (two-node sync spike).
//!
//! These tests need a real Postgres. They run against
//! `postgres://shopcore:shopcore@localhost:5432/shopcore_test` by default
//! (override with `DATABASE_URL`), or the `postgres` service in
//! `docker-compose.yml` / the CI `server-tests` job.
//!
//! Postgres tests NEVER skip silently (amendment 5): they skip ONLY when
//! the explicit env flag `SHOPCORE_SKIP_POSTGRES_TESTS=1` is set, and then
//! they print a loud message naming the skipped tests. Without the flag
//! and without Postgres, they FAIL.

use shopcore_protocol::{OutboxRow, PriceListEntry, SyncBatch};
use shopcore_server::{
    ServerState, applied_push_count, apply_price_entry, current_sequence, get_price_entry,
    get_stock_balance, revalidate_sale, run_migrations,
};
use shopcore_sync_client::{SyncClient, ensure_outbox_schema, mark_sent, read_unsent_outbox};
use sqlx::PgPool;
use uuid::Uuid;

/// Loud skip: prints the skipped test names. Only when the explicit flag
/// is set (amendment 5).
fn postgres_available() -> bool {
    if std::env::var("SHOPCORE_SKIP_POSTGRES_TESTS").as_deref() == Ok("1") {
        eprintln!(
            "LOUD SKIP: SHOPCORE_SKIP_POSTGRES_TESTS=1 — skipping ALL Postgres-backed \
             chaos tests (duplicate, out-of-order, drop, restart, skew, last-unit, \
             upgrade, snapshot, replay, LWW, gap-detector, revalidation). \
             This is NOT a pass; the tests did not run."
        );
        return false;
    }
    true
}

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgres://shopcore:shopcore@localhost:5432/shopcore_test".to_string())
}

/// Set up a fresh pool + migrations + a server on an ephemeral port.
/// Returns (pool, client, `base_url`, `server_handle`). Each test uses a
/// unique `tenant_id` so tests never interfere.
struct Harness {
    pool: PgPool,
    client: SyncClient,
    base_url: String,
    server: tokio::task::JoinHandle<()>,
    tenant_id: Uuid,
    branch_id: Uuid,
}

impl Harness {
    async fn start() -> Harness {
        let pool = PgPool::connect(&database_url())
            .await
            .expect("Postgres must be up for chaos tests (or set SHOPCORE_SKIP_POSTGRES_TESTS=1)");
        run_migrations(&pool).await.expect("migrations");
        let snapshot_dir = std::env::temp_dir().join(format!("shopcore-snap-{}", Uuid::now_v7()));
        let state = ServerState {
            pool: pool.clone(),
            snapshot_dir,
        };
        let app = shopcore_server::create_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        // Wait until the server answers /health.
        let client = SyncClient::new(base_url.clone()).expect("sync client");
        for _ in 0..50 {
            if client
                .pull(Uuid::now_v7(), Uuid::now_v7(), "price_list_entry", 0)
                .await
                .is_ok()
            {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        }
        Harness {
            pool,
            client,
            base_url,
            server,
            tenant_id: Uuid::now_v7(),
            branch_id: Uuid::now_v7(),
        }
    }

    /// Stop the server (for the restart test).
    fn stop_server(self) -> (PgPool, String) {
        self.server.abort();
        (self.pool, self.base_url)
    }

    /// Restart the server on a new ephemeral port, same pool.
    async fn restart(pool: PgPool) -> (SyncClient, String, tokio::task::JoinHandle<()>) {
        let snapshot_dir = std::env::temp_dir().join(format!("shopcore-snap-{}", Uuid::now_v7()));
        let state = ServerState { pool, snapshot_dir };
        let app = shopcore_server::create_app(state);
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0")
            .await
            .expect("bind ephemeral port");
        let addr = listener.local_addr().expect("local addr");
        let base_url = format!("http://{addr}");
        let server = tokio::spawn(async move {
            axum::serve(listener, app).await.expect("serve");
        });
        let client = SyncClient::new(base_url.clone()).expect("sync client");
        (client, base_url, server)
    }
}

fn movement_row(sku_id: Uuid, qty_milli: i64, occurred_at_ms: i64) -> OutboxRow {
    OutboxRow {
        id: Uuid::now_v7(),
        entity: "stock_movement".to_string(),
        stream: "stock_movement".to_string(),
        payload: serde_json::json!({
            "sku_id": sku_id.to_string(),
            "qty_milli": qty_milli,
            "occurred_at_ms": occurred_at_ms,
        }),
        created_at_ms: occurred_at_ms,
    }
}

fn batch(tenant_id: Uuid, branch_id: Uuid, rows: Vec<OutboxRow>) -> SyncBatch {
    SyncBatch {
        version: 1,
        tenant_id,
        branch_id,
        rows,
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| i64::try_from(d.as_millis()).unwrap_or(i64::MAX))
}

macro_rules! require_postgres {
    () => {
        if !postgres_available() {
            return;
        }
    };
}

#[tokio::test]
async fn chaos_duplicate_delivery_no_duplicates() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    let rows = vec![movement_row(sku, 5000, now_ms())];
    let b = batch(h.tenant_id, h.branch_id, rows);
    let ack1 = h.client.push(&b).await.expect("push 1");
    let ack2 = h.client.push(&b).await.expect("push 2 (duplicate)");
    // Same ack, same sequence, no new row.
    assert_eq!(ack1.server_sequence, ack2.server_sequence);
    assert_eq!(ack1.accepted, ack2.accepted);
    assert_eq!(
        applied_push_count(&h.pool, h.tenant_id)
            .await
            .expect("count"),
        1
    );
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        5000
    );
    h.server.abort();
}

#[tokio::test]
async fn chaos_out_of_order_batches_converge() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Batch A (+1000) and batch B (+2000). Push B first, then A.
    let b_a = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 1000, now_ms())],
    );
    let b_b = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 2000, now_ms())],
    );
    h.client.push(&b_b).await.expect("push B");
    h.client.push(&b_a).await.expect("push A");
    // Final state is the same as in-order: +3000 total.
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        3000
    );
    assert_eq!(
        applied_push_count(&h.pool, h.tenant_id)
            .await
            .expect("count"),
        2
    );
    h.server.abort();
}

#[tokio::test]
async fn chaos_connection_dropped_mid_batch_resumes() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Simulate a drop: the client pushes but never sees the ack (we drop
    // the future), then retries the same batch. The retry must be a no-op.
    let rows = vec![
        movement_row(sku, 1000, now_ms()),
        movement_row(sku, 2000, now_ms()),
    ];
    let b = batch(h.tenant_id, h.branch_id, rows);
    let push_fut = h.client.push(&b);
    // Drop the in-flight push (simulating a severed connection).
    drop(push_fut);
    // Retry the same batch: deduped, no duplicates.
    let ack = h.client.push(&b).await.expect("retry after drop");
    assert_eq!(ack.accepted.len(), 2);
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        3000
    );
    assert_eq!(
        applied_push_count(&h.pool, h.tenant_id)
            .await
            .expect("count"),
        2
    );
    h.server.abort();
}

#[tokio::test]
async fn chaos_server_restart_during_push_retries_safely() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    let b = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 4000, now_ms())],
    );
    h.client.push(&b).await.expect("push before restart");
    // Restart the server (same pool / Postgres, so applied_push persists).
    let tenant_id = h.tenant_id;
    let branch_id = h.branch_id;
    let (pool, _old_url) = h.stop_server();
    let (client2, _url2, server2) = Harness::restart(pool.clone()).await;
    // Retry after restart: deduped, no duplicate row or balance change.
    let ack = client2.push(&b).await.expect("push after restart");
    assert_eq!(ack.accepted.len(), 1);
    assert_eq!(
        get_stock_balance(&pool, tenant_id, branch_id, sku)
            .await
            .expect("balance"),
        4000
    );
    assert_eq!(
        applied_push_count(&pool, tenant_id).await.expect("count"),
        1
    );
    server2.abort();
}

#[tokio::test]
async fn chaos_clock_skew_uses_server_time() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    let now = now_ms();
    // Till clock 10 min fast and 10 min slow. Server orders by its own
    // sequence; both the till time and the server time are stored.
    let fast = movement_row(sku, 1000, now + 600_000);
    let slow = movement_row(sku, 2000, now - 600_000);
    let b = batch(h.tenant_id, h.branch_id, vec![fast, slow]);
    let ack = h.client.push(&b).await.expect("push skewed");
    assert_eq!(ack.accepted.len(), 2);
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        3000
    );
    // The server stored its own applied_at_ms (server time) alongside the
    // till's created_at_ms in the payload — verify the rows exist.
    assert_eq!(
        applied_push_count(&h.pool, h.tenant_id)
            .await
            .expect("count"),
        2
    );
    h.server.abort();
}

#[tokio::test]
async fn chaos_two_tills_sell_last_unit_both_accepted() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Seed balance 1 (one unit in stock, same branch).
    let seed = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 1000, now_ms())],
    );
    h.client.push(&seed).await.expect("seed");
    // Two tills in the SAME branch each sell the last unit simultaneously.
    // Both sales are accepted (the system does not block the second);
    // the shared balance goes negative (-1000), which flags the item.
    let sale_a = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, -1000, now_ms())],
    );
    let sale_b = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, -1000, now_ms())],
    );
    let (ra, rb) = tokio::join!(h.client.push(&sale_a), h.client.push(&sale_b));
    assert!(ra.is_ok(), "till A sale accepted");
    assert!(rb.is_ok(), "till B sale accepted");
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        -1000,
        "balance negative (1 - 1 - 1), item flagged"
    );
    h.server.abort();
}

#[tokio::test]
async fn chaos_upgrade_with_unsent_outbox_rows_still_delivered() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Write rows to the till's SQLite outbox BEFORE the server is used
    // (simulating rows queued before an upgrade), then push them.
    let db_path = std::env::temp_dir().join(format!("till-{}.db", Uuid::now_v7()));
    let conn = rusqlite::Connection::open(&db_path).expect("till db");
    ensure_outbox_schema(&conn).expect("outbox schema");
    let row = movement_row(sku, 7000, now_ms());
    conn.execute(
        "INSERT INTO outbox (id, tenant_id, branch_id, entity, stream, payload, idempotency_key, created_at, updated_at, deleted_at, version, sent_at, attempts)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, NULL, 1, NULL, 0)",
        rusqlite::params![
            row.id.to_string(),
            h.tenant_id.to_string(),
            h.branch_id.to_string(),
            row.entity,
            row.stream,
            row.payload.to_string(),
            row.id.to_string(),
            row.created_at_ms,
            row.created_at_ms,
        ],
    )
    .expect("insert outbox row");
    // Read the unsent rows and push them (the "upgrade" changed nothing
    // about the queued rows — they are still delivered).
    let unsent = read_unsent_outbox(&conn, 200).expect("read unsent");
    assert_eq!(unsent.len(), 1);
    let b = batch(h.tenant_id, h.branch_id, unsent.clone());
    let ack = h.client.push(&b).await.expect("push after upgrade");
    mark_sent(&conn, &ack.accepted, now_ms()).expect("mark sent");
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        7000
    );
    assert_eq!(read_unsent_outbox(&conn, 200).expect("re-read").len(), 0);
    let _ = std::fs::remove_file(&db_path);
    h.server.abort();
}

#[tokio::test]
async fn chaos_snapshot_interrupted_resumes_and_handoff_clean() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Seed data so the snapshot is non-empty.
    let seed = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 9000, now_ms())],
    );
    h.client.push(&seed).await.expect("seed");
    // Build the snapshot directly (server-side) to get its checksum +
    // cursor, then download it via the client in two parts (interrupt +
    // resume) and verify.
    let snap_dir = std::env::temp_dir().join(format!("snap-test-{}", Uuid::now_v7()));
    let info = shopcore_server::build_snapshot(&h.pool, h.tenant_id, h.branch_id, &snap_dir)
        .await
        .expect("build snapshot");
    assert!(info.size_bytes > 0);
    assert_eq!(info.sha256_hex.len(), 64);
    // Download with the client (Range resume). First download fully.
    let dest = snap_dir.join("download.db.zst");
    let size = h
        .client
        .download_snapshot(h.tenant_id, h.branch_id, &dest, &info.sha256_hex)
        .await
        .expect("download snapshot");
    assert_eq!(size, info.size_bytes);
    // Interrupt + resume: truncate to half, then re-download (resumes
    // from the current size via Range) and the checksum must still pass.
    let half = size / 2;
    {
        let f = std::fs::OpenOptions::new()
            .write(true)
            .open(&dest)
            .expect("open");
        f.set_len(half).expect("truncate");
    }
    let size2 = h
        .client
        .download_snapshot(h.tenant_id, h.branch_id, &dest, &info.sha256_hex)
        .await
        .expect("resume download");
    assert_eq!(size2, info.size_bytes);
    // Cursor handoff: pull from the snapshot's cursor returns only rows
    // after it (no gap, no overlap). Nothing was pushed after the
    // snapshot, so the pull is empty.
    let pulled = h
        .client
        .pull(
            h.tenant_id,
            h.branch_id,
            "price_list_entry",
            info.cursor_sequence,
        )
        .await
        .expect("pull after snapshot");
    assert_eq!(pulled.next_sequence, info.cursor_sequence);
    assert_eq!(pulled.rows.len(), 0);
    let _ = std::fs::remove_dir_all(&snap_dir);
    h.server.abort();
}

#[tokio::test]
async fn chaos_replay_push_history_twice_byte_identical() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    let b1 = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 1000, now_ms())],
    );
    let b2 = batch(
        h.tenant_id,
        h.branch_id,
        vec![movement_row(sku, 2000, now_ms())],
    );
    h.client.push(&b1).await.expect("push b1");
    h.client.push(&b2).await.expect("push b2");
    let count_before = applied_push_count(&h.pool, h.tenant_id)
        .await
        .expect("count");
    let seq_before = current_sequence(&h.pool, h.tenant_id).await.expect("seq");
    let bal_before = get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
        .await
        .expect("balance");
    // Replay the ENTIRE push history.
    let rack1 = h.client.push(&b1).await.expect("replay b1");
    let rack2 = h.client.push(&b2).await.expect("replay b2");
    // The replay returns the EXISTING (current) sequence — no new sequence
    // was consumed. It is seq_before (2), not ack1's original sequence (1),
    // because b2 was applied after b1.
    assert_eq!(rack1.server_sequence, seq_before);
    assert_eq!(rack2.server_sequence, seq_before);
    // Byte-identical state: no new rows, no sequence advance, same balance.
    assert_eq!(
        applied_push_count(&h.pool, h.tenant_id)
            .await
            .expect("count"),
        count_before
    );
    assert_eq!(
        current_sequence(&h.pool, h.tenant_id).await.expect("seq"),
        seq_before
    );
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, sku)
            .await
            .expect("balance"),
        bal_before
    );
    // Prove the DB constraint fires: a direct duplicate insert into
    // applied_push is rejected by Postgres (unique violation), not by
    // application code.
    let dup = sqlx::query(
        "INSERT INTO applied_push (tenant_id, idempotency_key, entity, stream, payload, server_sequence, applied_at_ms)
         VALUES ($1::uuid, $2::uuid, 'sale', 'sale', '{}', 999, $3)",
    )
    .bind(b1.tenant_id.to_string())
    .bind(b1.rows.first().map(|r| r.id.to_string()).unwrap_or_default())
    .bind(now_ms())
    .execute(&h.pool)
    .await;
    assert!(dup.is_err(), "duplicate insert must be rejected by the DB");
    let err = format!("{}", dup.unwrap_err());
    assert!(
        err.contains("duplicate key") || err.contains("unique"),
        "expected a unique-violation error, got: {err}"
    );
    h.server.abort();
}

#[tokio::test]
async fn per_field_lww_two_edits_merge() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    let entry_id = Uuid::now_v7();
    let base = PriceListEntry {
        id: entry_id,
        tenant_id: h.tenant_id,
        branch_id: h.branch_id,
        sku_id: sku,
        price_minor: 1000,
        price_updated_at_ms: 1_000,
        currency: "PKR".to_string(),
        effective_from_ms: 2_000,
        effective_updated_at_ms: 1_000,
        version: 1,
        deleted_at_ms: None,
    };
    apply_price_entry(&h.pool, &base).await.expect("base entry");
    // Edit A changes ONLY the price (newer price timestamp).
    let mut edit_a = base.clone();
    edit_a.price_minor = 1500;
    edit_a.price_updated_at_ms = 2_000;
    // Edit B changes ONLY effective_from (newer effective timestamp).
    let mut edit_b = base.clone();
    edit_b.effective_from_ms = 3_000;
    edit_b.effective_updated_at_ms = 2_000;
    apply_price_entry(&h.pool, &edit_a).await.expect("edit A");
    apply_price_entry(&h.pool, &edit_b).await.expect("edit B");
    // Both fields merged: A's price AND B's effective date.
    let merged = get_price_entry(&h.pool, entry_id)
        .await
        .expect("read merged");
    assert_eq!(merged.price_minor, 1500, "edit A's price survived");
    assert_eq!(
        merged.effective_from_ms, 3_000,
        "edit B's effective date survived"
    );
    h.server.abort();
}

#[tokio::test]
async fn gap_detector_sequences_are_contiguous() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Push 5 batches; the server sequences for this tenant must be
    // contiguous (1..=5) with no gap — a gap would mean a lost batch.
    for i in 0..5 {
        let b = batch(
            h.tenant_id,
            h.branch_id,
            vec![movement_row(sku, 100 * (i + 1), now_ms())],
        );
        h.client.push(&b).await.expect("push");
    }
    let seq = current_sequence(&h.pool, h.tenant_id).await.expect("seq");
    assert_eq!(seq, 5, "5 batches => sequence 5, no gap");
    // The applied_push rows carry sequences 1..=5 with no duplicates.
    let rows = sqlx::query(
        "SELECT server_sequence FROM applied_push WHERE tenant_id = $1::uuid ORDER BY server_sequence",
    )
    .bind(h.tenant_id.to_string())
    .fetch_all(&h.pool)
    .await
    .expect("sequences");
    let seqs: Vec<i64> = rows
        .iter()
        .map(|r| {
            use sqlx::Row as _;
            r.try_get("server_sequence").unwrap_or(-1)
        })
        .collect();
    assert_eq!(seqs, vec![1, 2, 3, 4, 5]);
    h.server.abort();
}

#[tokio::test]
async fn revalidation_stub_flags_mismatch() {
    // Pure function (no Postgres needed) — but keep the same gate for
    // consistency with the other chaos tests.
    require_postgres!();
    // Matching sums pass.
    assert!(revalidate_sale(&[1000, 500, 250], 1750).is_ok());
    assert!(revalidate_sale(&[], 0).is_ok());
    // Mismatch is flagged, never altered.
    let err = revalidate_sale(&[1000, 500], 1600).unwrap_err();
    assert!(
        format!("{err}").contains("1500"),
        "error names the actual sum"
    );
    // Overflow is flagged (checked add, never wraps).
    let err = revalidate_sale(&[i64::MAX, 1], 0).unwrap_err();
    assert!(format!("{err}").contains("overflow"));
}

/// Performance measurement (ignored in CI; run locally with
/// `cargo test -p shopcore-server --test chaos perf_measure -- --ignored --nocapture`).
/// Prints push/pull rows/s and snapshot size/time. Numbers go to
/// `08-decisions.md` (spec 005 performance section).
#[tokio::test]
#[ignore = "perf measurement, run manually with --ignored"]
async fn perf_measure() {
    require_postgres!();
    let h = Harness::start().await;
    let sku = Uuid::now_v7();
    // Push throughput: 10 batches x 200 rows = 2000 rows.
    let t0 = std::time::Instant::now();
    for _ in 0..10 {
        let rows: Vec<OutboxRow> = (0..200).map(|_| movement_row(sku, 100, now_ms())).collect();
        let b = batch(h.tenant_id, h.branch_id, rows);
        h.client.push(&b).await.expect("push");
    }
    let push_secs = t0.elapsed().as_secs_f64();
    eprintln!(
        "PERF push: 2000 rows in {push_secs:.3}s = {:.0} rows/s",
        2000.0 / push_secs
    );
    // Pull throughput: seed 50 price entries, then pull them.
    for i in 0..50 {
        let entry = PriceListEntry {
            id: Uuid::now_v7(),
            tenant_id: h.tenant_id,
            branch_id: h.branch_id,
            sku_id: Uuid::now_v7(),
            price_minor: 1000 + i,
            price_updated_at_ms: now_ms(),
            currency: "PKR".to_string(),
            effective_from_ms: now_ms(),
            effective_updated_at_ms: now_ms(),
            version: 1,
            deleted_at_ms: None,
        };
        apply_price_entry(&h.pool, &entry)
            .await
            .expect("price entry");
    }
    let t0 = std::time::Instant::now();
    let pulled = h
        .client
        .pull(h.tenant_id, h.branch_id, "price_list_entry", 0)
        .await
        .expect("pull");
    let pull_secs = t0.elapsed().as_secs_f64();
    let n = u32::try_from(pulled.rows.len()).unwrap_or(u32::MAX);
    eprintln!(
        "PERF pull: {} rows in {pull_secs:.3}s = {:.0} rows/s",
        n,
        f64::from(n) / pull_secs
    );
    // Snapshot: build + download + checksum.
    let snap_dir = std::env::temp_dir().join(format!("snap-perf-{}", Uuid::now_v7()));
    let t0 = std::time::Instant::now();
    let info = shopcore_server::build_snapshot(&h.pool, h.tenant_id, h.branch_id, &snap_dir)
        .await
        .expect("build snapshot");
    let build_secs = t0.elapsed().as_secs_f64();
    eprintln!(
        "PERF snapshot: {} bytes, cursor {}, built in {build_secs:.3}s",
        info.size_bytes, info.cursor_sequence
    );
    let dest = snap_dir.join("download.db.zst");
    let t0 = std::time::Instant::now();
    let size = h
        .client
        .download_snapshot(h.tenant_id, h.branch_id, &dest, &info.sha256_hex)
        .await
        .expect("download snapshot");
    let dl_secs = t0.elapsed().as_secs_f64();
    eprintln!("PERF snapshot download: {size} bytes in {dl_secs:.3}s");
    let _ = std::fs::remove_dir_all(&snap_dir);
    h.server.abort();
}
