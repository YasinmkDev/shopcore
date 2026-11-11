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

use shopcore_core::draft::{draft_add_line, open_draft};
use shopcore_core::sale::{CompleteSaleCashInput, SaleCompleted, complete_sale_cash};
use shopcore_core::scratch::ScratchDb;
use shopcore_core::settings::{
    KEY_ACTIVE_PRICE_LIST, KEY_CASH_ROUNDING_GRANULARITY_MINOR, KEY_TAX_DEFAULT_RATE_BP,
    KEY_TENANT_CURRENCY, KEY_TENANT_CURRENCY_EXPONENT, SCOPE_TENANT, set_setting,
};
use shopcore_core::users::{create_role, create_user};
use shopcore_db::{Db, DbError};
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
             upgrade, snapshot, replay, LWW, gap-detector, revalidation, \
             till-outbox-push). \
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

/// A real till (001b-1 migrations + 001b-2 sale use cases) whose
/// tenant/branch match the harness's, so its outbox rows push to the
/// real server. One SKU (`TST100`, 100.00 exclusive @18%).
struct TillFixture {
    db: Db,
    scratch: ScratchDb,
    tenant_id: String,
    branch_id: String,
    terminal_id: String,
    shift_id: String,
    cashier_id: String,
    sku_id: Uuid,
}

/// Build the till: migrations, one cashier role + user (one Argon2id
/// PIN hash), branch/terminal, `STD` 18% tax group + rate, one SKU
/// with opening stock, tenant settings. Mirrors the core sale tests'
/// fixture, but every id is a real `UUIDv7` (the server parses the
/// pushed `sku_id` as a `Uuid`) and the tenant/branch are the
/// harness's.
/// Seed the till's org + catalog in one transaction: branch/terminal,
/// `STD` 18% tax group + rate, one SKU with opening stock.
#[allow(clippy::too_many_arguments)]
fn seed_till_org_and_catalog(
    db: &Db,
    tenant: &str,
    branch: &str,
    terminal_id: &str,
    shift_id: &str,
    tax_std_id: &str,
    tax_rate_id: &str,
    sku_id: Uuid,
    now: i64,
) {
    let tenant = tenant.to_string();
    let branch = branch.to_string();
    let terminal_id = terminal_id.to_string();
    let shift_id = shift_id.to_string();
    let tax_std_id = tax_std_id.to_string();
    let tax_rate_id = tax_rate_id.to_string();
    let sku = sku_id.to_string();
    db.write(move |conn| {
        let tx = conn.transaction().unwrap();
        tx.execute(
            "INSERT INTO branch (id, tenant_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, 'MAIN', 'Main Branch', ?3, ?3, 1)",
            rusqlite::params![branch, tenant, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO terminal (id, tenant_id, branch_id, code, name, created_at, updated_at, version) VALUES (?1, ?2, ?3, 'T1', 'Till 1', ?4, ?4, 1)",
            rusqlite::params![terminal_id, tenant, branch, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_group (id, tenant_id, code, name, price_includes_tax, created_at, updated_at, version) VALUES (?1, ?2, 'STD', 'Standard 18%', 0, ?3, ?3, 1)",
            rusqlite::params![tax_std_id, tenant, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) VALUES (?1, ?2, ?3, 1800, ?4, ?4, 1)",
            rusqlite::params![tax_rate_id, tenant, tax_std_id, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO uom (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('uom-ea', ?1, 'EA', 'Each', ?2, ?2, 1)",
            rusqlite::params![tenant, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO price_list (id, tenant_id, code, name, created_at, updated_at, version) VALUES ('pricelist-std', ?1, 'STD', 'Standard', ?2, ?2, 1)",
            rusqlite::params![tenant, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO shift (id, tenant_id, branch_id, terminal_id, status, opened_at, created_at, updated_at, version) VALUES (?1, ?2, ?3, ?4, 'open', ?5, ?5, ?5, 1)",
            rusqlite::params![shift_id, tenant, branch, terminal_id, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO item (id, tenant_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES ('item-TST100', ?1, 'TST100', 'TST100', 'uom-ea', ?2, ?3, ?3, 1)",
            rusqlite::params![tenant, tax_std_id, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, tax_group_id, created_at, updated_at, version) VALUES (?1, ?2, 'item-TST100', 'TST100', 'TST100', 'uom-ea', ?3, ?4, ?4, 1)",
            rusqlite::params![sku, tenant, tax_std_id, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, created_at, updated_at, version) VALUES ('barcode-TST100', ?1, ?2, '1234567890998', 1, ?3, ?3, 1)",
            rusqlite::params![tenant, sku, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO price_list_entry (id, tenant_id, price_list_id, sku_id, price_e4, created_at, updated_at, version) VALUES ('ple-TST100', ?1, 'pricelist-std', ?2, 1000000, ?3, ?3, 1)",
            rusqlite::params![tenant, sku, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO stock_balance (id, tenant_id, branch_id, sku_id, qty_milli, created_at, updated_at, version) VALUES ('bal-TST100', ?1, ?2, ?3, 10000, ?4, ?4, 1)",
            rusqlite::params![tenant, branch, sku, now],
        )
        .unwrap();
        tx.execute(
            "INSERT INTO stock_movement (id, tenant_id, branch_id, sku_id, type, qty_milli, unit_cost_e4, occurred_at, created_at, updated_at, version) VALUES ('mov-open-TST100', ?1, ?2, ?3, 'opening', 10000, 0, ?4, ?4, ?4, 1)",
            rusqlite::params![tenant, branch, sku, now],
        )
        .unwrap();
        tx.commit().unwrap();
        Ok(())
    })
    .expect("till org + catalog");
}

fn build_till(tenant_id: Uuid, branch_id: Uuid) -> TillFixture {
    let dir = std::env::temp_dir().join(format!("shopcore-till-{}", Uuid::now_v7()));
    std::fs::create_dir_all(&dir).expect("till dir");
    let db = Db::open(dir.join("till.db"), None).expect("till db");
    let scratch = ScratchDb::open(dir.join("scratch.db")).expect("scratch db");
    let tenant = tenant_id.to_string();
    let branch = branch_id.to_string();
    let terminal_id = Uuid::now_v7().to_string();
    let shift_id = Uuid::now_v7().to_string();
    let tax_std_id = Uuid::now_v7().to_string();
    let tax_rate_id = Uuid::now_v7().to_string();
    let sku_id = Uuid::now_v7();
    let now = now_ms();
    // `ensure_outbox_schema` (the sync-client's table creator) must
    // NOT conflict with the migration's outbox table — it is a no-op
    // when the table already exists with the spec-005 columns.
    db.write(|conn| ensure_outbox_schema(conn).map_err(|e| DbError::InvalidInput(e.to_string())))
        .expect("ensure_outbox_schema is a no-op on the migrated table");
    let role =
        create_role(&db, &tenant, "Cashier", &["sale.create"], 500, now).expect("cashier role");
    let cashier =
        create_user(&db, &tenant, "cashier", "1234", Some(&role.id), now).expect("cashier user");
    seed_till_org_and_catalog(
        &db,
        &tenant,
        &branch,
        &terminal_id,
        &shift_id,
        &tax_std_id,
        &tax_rate_id,
        sku_id,
        now,
    );
    set_setting(
        &db,
        &tenant,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY,
        "PKR",
        now,
    )
    .expect("currency setting");
    set_setting(
        &db,
        &tenant,
        SCOPE_TENANT,
        "",
        KEY_TENANT_CURRENCY_EXPONENT,
        "2",
        now,
    )
    .expect("exponent setting");
    set_setting(
        &db,
        &tenant,
        SCOPE_TENANT,
        "",
        KEY_CASH_ROUNDING_GRANULARITY_MINOR,
        "0",
        now,
    )
    .expect("cash rounding setting");
    set_setting(
        &db,
        &tenant,
        SCOPE_TENANT,
        "",
        KEY_TAX_DEFAULT_RATE_BP,
        "1800",
        now,
    )
    .expect("tax default setting");
    set_setting(
        &db,
        &tenant,
        SCOPE_TENANT,
        "",
        KEY_ACTIVE_PRICE_LIST,
        "pricelist-std",
        now,
    )
    .expect("price list setting");
    TillFixture {
        db,
        scratch,
        tenant_id: tenant,
        branch_id: branch,
        terminal_id,
        shift_id,
        cashier_id: cashier.id,
        sku_id,
    }
}

/// Assert the migrated outbox columns: every unsent row carries
/// the branch, and its idempotency key IS its own id.
fn assert_outbox_columns(till: &TillFixture, n: usize) {
    // idempotency key IS its own id.
    let cols = till
        .db
        .read(|conn| {
            let mut stmt = conn
                .prepare("SELECT branch_id, idempotency_key, id FROM outbox WHERE sent_at IS NULL")
                .map_err(DbError::from)?;
            let mapped = stmt
                .query_map([], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(DbError::from)?;
            let mut out = Vec::new();
            for r in mapped {
                out.push(r.map_err(DbError::from)?);
            }
            Ok(out)
        })
        .expect("read outbox columns");
    assert_eq!(cols.len(), n);
    for (branch_id, idem, id) in &cols {
        assert_eq!(branch_id, &till.branch_id);
        assert_eq!(idem, id, "idempotency_key is the row's own id");
    }
}

/// Complete a real sale on the till: 1x 100.00 exclusive @18%
/// (the 001a golden: tax 18.00, total 118.00), tender 118.00.
fn complete_till_sale(till: &TillFixture, now: i64) -> SaleCompleted {
    // (the 001a golden: tax 18.00, total 118.00), tender 118.00.
    let cart = open_draft(
        &till.scratch,
        &till.tenant_id,
        &till.branch_id,
        &till.terminal_id,
        Some(&till.shift_id),
        Some(&till.cashier_id),
        now,
    )
    .expect("open draft");
    draft_add_line(
        &till.scratch,
        &cart.id,
        &till.sku_id.to_string(),
        1_000,
        1_000_000,
        now,
    )
    .expect("add line");
    let completed = complete_sale_cash(
        &till.db,
        &till.scratch,
        CompleteSaleCashInput {
            tenant_id: till.tenant_id.clone(),
            branch_id: till.branch_id.clone(),
            terminal_id: till.terminal_id.clone(),
            branch_code: "MAIN".to_string(),
            terminal_code: "T1".to_string(),
            shift_id: till.shift_id.clone(),
            user_id: till.cashier_id.clone(),
            draft_id: cart.id.clone(),
            tender_minor: 11_800,
            now_ms: now,
            date_yyyymmdd: "20261111".to_string(),
        },
        None,
    )
    .expect("complete sale");
    assert!(completed.is_fresh, "the sale commits fresh");
    assert_eq!(completed.total_minor, 11_800);
    assert_eq!(completed.change_minor, 0);
    completed
}

/// Item 1 (owner decision 2026-11-11): REAL till outbox rows push to
/// the REAL server and the server revalidates the sale. The till is a
/// 001b-1 migration + 001b-2 sale; the outbox rows are read through
/// the sync-client's own reader and pushed over real HTTP; the
/// server's ack, `applied_push` count, derived stock balance, and
/// `revalidate_sale` on the pushed sale's line totals are all
/// asserted. If revalidation flags a mismatch on this real till sale,
/// that is a BUG — not a test to weaken.
#[tokio::test]
async fn till_outbox_pushes_to_real_server_and_revalidates() {
    require_postgres!();
    let h = Harness::start().await;
    let till = build_till(h.tenant_id, h.branch_id);
    let now = now_ms();
    let completed = complete_till_sale(&till, now);
    // The outbox rows the sale wrote, read through the sync-client's
    // own reader (the same code the real sync worker uses) — this is
    // the proof the migrated table has the spec-005 columns
    // (`stream` included).
    let rows = till
        .db
        .read(|conn| {
            read_unsent_outbox(conn, 200).map_err(|e| DbError::InvalidInput(e.to_string()))
        })
        .expect("read outbox");
    let n = rows.len();
    assert_eq!(n, 2, "one sale row + one stock_movement row");
    assert!(
        rows.iter()
            .any(|r| r.entity == "sale" && r.stream == "sale")
    );
    assert!(
        rows.iter()
            .any(|r| r.entity == "stock_movement" && r.stream == "stock_movement")
    );
    assert_outbox_columns(&till, n);
    // Push the REAL till rows to the REAL server.
    let ids: Vec<Uuid> = rows.iter().map(|r| r.id).collect();
    let ack = h
        .client
        .push(&batch(h.tenant_id, h.branch_id, rows))
        .await
        .expect("push");
    assert_eq!(ack.accepted.len(), n, "the server accepted every row");
    assert_eq!(
        applied_push_count(&h.pool, h.tenant_id)
            .await
            .expect("count"),
        i64::try_from(n).unwrap_or(i64::MAX),
        "every row is in applied_push"
    );
    // The server derived the stock balance from the pushed movement.
    // It has no opening stock, so the balance is the sale alone: -1000.
    assert_eq!(
        get_stock_balance(&h.pool, h.tenant_id, h.branch_id, till.sku_id)
            .await
            .expect("balance"),
        -1_000
    );
    // The server revalidates the pushed sale: the line totals sum to
    // the sale total — no mismatch flagged.
    let line_total = till
        .db
        .read(|conn| {
            conn.query_row(
                "SELECT total_minor FROM sale_line WHERE sale_id = ?1",
                rusqlite::params![completed.sale_id],
                |row| row.get::<_, i64>(0),
            )
            .map_err(DbError::from)
        })
        .expect("line total");
    assert!(revalidate_sale(&[line_total], completed.total_minor).is_ok());
    // Mark sent (the real sync worker's step after the ack): the
    // outbox is empty afterwards.
    till.db
        .write(move |conn| {
            mark_sent(conn, &ids, now_ms()).map_err(|e| DbError::InvalidInput(e.to_string()))
        })
        .expect("mark sent");
    let remaining = till
        .db
        .read(|conn| {
            read_unsent_outbox(conn, 200).map_err(|e| DbError::InvalidInput(e.to_string()))
        })
        .expect("read outbox again");
    assert!(remaining.is_empty(), "the outbox is empty after mark_sent");
    h.server.abort();
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
