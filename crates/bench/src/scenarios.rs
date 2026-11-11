//! Scenario implementations (spec 002, scenarios 1–10).
//!
//! Times are measured with `Instant` and recorded in a histogram in
//! microseconds; percentiles are reported in milliseconds. Every scenario
//! runs against the REAL app schema (001b-1 migrations, see
//! `real_schema.rs`) since spec 001b-2.

use std::path::Path;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use hdrhistogram::Histogram;
use rusqlite::{Connection, Transaction, params};
use serde::Deserialize;

use crate::BenchError;
use crate::report::{MachineRecord, ScenarioResult};
use crate::seed::DatasetSize;

/// Convert histogram microseconds to (p50, p95, p99) milliseconds.
#[allow(clippy::cast_precision_loss)]
fn percentiles(hist: &Histogram<u64>) -> (f64, f64, f64) {
    let to_ms = |us: u64| us as f64 / 1_000.0;
    (
        to_ms(hist.value_at_percentile(50.0)),
        to_ms(hist.value_at_percentile(95.0)),
        to_ms(hist.value_at_percentile(99.0)),
    )
}

/// Run `f` `iterations` times, timing each call, and return (p50, p95, p99)
/// in milliseconds.
///
/// # Errors
/// Returns [`BenchError::Histogram`] if a duration does not fit the
/// histogram, or the inner error if `f` fails.
fn measure<F>(iterations: u32, mut f: F) -> Result<(f64, f64, f64), BenchError>
where
    F: FnMut() -> Result<(), BenchError>,
{
    let mut hist = Histogram::<u64>::new_with_bounds(1, 60_000_000, 3)
        .map_err(BenchError::HistogramCreation)?;
    for _ in 0..iterations {
        let started = Instant::now();
        f()?;
        let micros = u64::try_from(started.elapsed().as_micros()).unwrap_or(u64::MAX);
        hist.record(micros).map_err(BenchError::HistogramRecord)?;
    }
    Ok(percentiles(&hist))
}

/// UI measurements produced by the debug route (Ctrl+Shift+B in the app).
/// Written by the `bench_ui_results` debug command; merged by the runner
/// when `--ui-results` is passed.
#[derive(Debug, Clone, Deserialize)]
pub struct UiResults {
    /// Process start → main window interactive, ms.
    pub startup_ms: f64,
    /// Frame time p95 while scrolling the 100k-row grid for 10 s, ms.
    pub scroll_p95_ms: f64,
    /// Long tasks (> 50 ms) seen during the scroll test.
    pub long_tasks: u64,
    /// Re-renders per cart row when 50 rows are added rapidly.
    pub cart_rerenders_per_row: f64,
    /// Route change to the debug view, first paint, ms.
    pub route_change_ms: f64,
}

/// Load UI results from a JSON file.
///
/// # Errors
/// Returns [`BenchError::Io`] or [`BenchError::Json`] on failure.
pub fn load_ui_results(path: &Path) -> Result<UiResults, BenchError> {
    let text = std::fs::read_to_string(path).map_err(BenchError::Io)?;
    serde_json::from_str(&text).map_err(BenchError::Json)
}

/// Try to drop the OS file cache so the "cold" barcode lookup is a
/// true cold read (disk, not the page cache). Best effort:
/// - Linux: write `3` to `/proc/sys/vm/drop_caches` (needs root).
///   Returns `true` only if the write succeeds.
/// - Windows: there is no equivalent a normal user process can do —
///   dropping the standby list needs an admin tool (`RAMMap`) or a
///   reboot. Returns `false`; the caller must say so honestly.
/// - Other platforms: `false`.
///
/// A `false` return is not an error — it means the cold number below
/// is a fresh-connection lookup on a warm cache, and it is labelled
/// that way instead of "cold".
#[must_use]
pub fn drop_os_file_cache() -> bool {
    #[cfg(target_os = "linux")]
    {
        std::fs::write("/proc/sys/vm/drop_caches", "3").is_ok()
    }
    #[cfg(not(target_os = "linux"))]
    {
        false
    }
}

/// Scenario 1 — barcode exact lookup, cold (fresh connection) and warm.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any lookup fails.
#[allow(clippy::too_many_lines)]
pub fn scenario_barcode_lookup(
    db_path: &Path,
    size: &DatasetSize,
) -> Result<Vec<ScenarioResult>, BenchError> {
    let mut out = Vec::new();
    // Pick 1,000 real barcodes to look up.
    let conn = open_db(db_path)?;
    let codes: Vec<String> = {
        let mut stmt = conn
            .prepare("SELECT code FROM barcode ORDER BY rowid LIMIT 1000")
            .map_err(BenchError::Sql)?;
        let rows = stmt
            .query_map([], |row| row.get::<_, String>(0))
            .map_err(BenchError::Sql)?;
        let mut codes = Vec::new();
        for row in rows {
            codes.push(row.map_err(BenchError::Sql)?);
        }
        codes
    };
    drop(conn);

    // Cold: drop the OS file cache first if possible, then a fresh
    // connection, first lookup only. If the cache could not be
    // dropped (no root on Linux, or Windows where a normal process
    // cannot do it), the number is honestly labelled "fresh
    // connection" — a warm-cache lookup — not "cold".
    let cache_dropped = drop_os_file_cache();
    let cold_conn = open_db(db_path)?;
    let code = codes.first().map_or("", String::as_str);
    let started = Instant::now();
    let _: Option<String> = cold_conn
        .query_row(
            "SELECT sku_id FROM barcode WHERE tenant_id = 'bench-tenant' AND code = ?1",
            params![code],
            |row| row.get(0),
        )
        .ok();
    let cold_ms = started.elapsed().as_secs_f64() * 1_000.0;
    drop(cold_conn);
    let (cold_name, cold_desc) = if cache_dropped {
        (
            "1 barcode-lookup (cold)",
            "first lookup on a fresh connection after dropping the OS file cache",
        )
    } else {
        (
            "1 barcode-lookup (fresh connection)",
            "first lookup on a fresh connection — OS page cache NOT dropped \
             (true cold read needs root on Linux, or a reboot / admin \
             cache-drop tool on Windows); this is a warm-cache lookup",
        )
    };
    out.push(ScenarioResult::single(
        cold_name,
        cold_desc,
        cold_ms,
        serde_json::json!({ "lookups": 1, "os_cache_dropped": cache_dropped }),
        Some(("< 50 ms (scan → line)", 50.0)),
    ));

    // Warm: 1,000 lookups on one connection, histogrammed.
    let conn = open_db(db_path)?;
    let mut index = 0usize;
    let (p50, p95, p99) = measure(1_000, || {
        let code = codes.get(index % codes.len()).map_or("", String::as_str);
        index += 1;
        let _: Option<String> = conn
            .query_row(
                "SELECT sku_id FROM barcode WHERE tenant_id = 'bench-tenant' AND code = ?1",
                params![code],
                |row| row.get(0),
            )
            .ok();
        Ok(())
    })?;
    out.push(ScenarioResult::from_histogram(
        "1 barcode-lookup (warm)",
        "single indexed lookup",
        p50,
        p95,
        p99,
        serde_json::json!({ "lookups": 1000, "dataset_skus": size.skus }),
        Some(("< 50 ms (scan → line)", 50.0)),
    ));
    Ok(out)
}

/// Scenario 2 — name search: FTS5 prefix + in-memory hot index.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any search fails.
pub fn scenario_name_search(
    db_path: &Path,
    size: &DatasetSize,
) -> Result<Vec<ScenarioResult>, BenchError> {
    let mut out = Vec::new();
    let conn = open_db(db_path)?;

    // FTS5 prefix search, 100 random prefixes.
    let prefixes = [
        "Ri", "Mi", "Bre", "Sug", "Te", "Oi", "Flo", "Soa", "Sa", "Le",
    ];
    let mut index = 0usize;
    let (p50, p95, p99) = measure(100, || {
        let prefix = prefixes
            .get(index % prefixes.len())
            .copied()
            .unwrap_or("Ri");
        index += 1;
        let mut stmt = conn
            .prepare("SELECT name FROM sku_fts WHERE sku_fts MATCH ?1 LIMIT 20")
            .map_err(BenchError::Sql)?;
        let query = format!("{prefix}*");
        let rows = stmt
            .query_map(params![query], |row| row.get::<_, String>(0))
            .map_err(BenchError::Sql)?;
        let mut count = 0;
        for row in rows {
            let _ = row.map_err(BenchError::Sql)?;
            count += 1;
        }
        let _ = count;
        Ok(())
    })?;
    out.push(ScenarioResult::from_histogram(
        "2 name-search (FTS5)",
        "prefix search, top 20",
        p50,
        p95,
        p99,
        serde_json::json!({ "dataset_skus": size.skus }),
        Some(("< 100 ms first results", 100.0)),
    ));

    // Hot index: sorted names of the active SKUs, built at startup.
    let started = Instant::now();
    let mut stmt = conn
        .prepare("SELECT name, id FROM sku WHERE active = 1 ORDER BY name")
        .map_err(BenchError::Sql)?;
    let rows = stmt
        .query_map([], |row| {
            Ok((row.get::<_, String>(0)?, row.get::<_, String>(1)?))
        })
        .map_err(BenchError::Sql)?;
    let mut index: Vec<(String, String)> = Vec::new();
    let mut bytes: usize = 0;
    for row in rows {
        let (name, id) = row.map_err(BenchError::Sql)?;
        bytes += name.len() + id.len() + 48; // rough per-entry overhead
        index.push((name, id));
    }
    let build_ms = started.elapsed().as_secs_f64() * 1_000.0;
    // One prefix lookup in the hot index (binary search).
    let started = Instant::now();
    let _ = index.partition_point(|(name, _)| name.as_str() < "Rice");
    let lookup_ms = started.elapsed().as_secs_f64() * 1_000.0;
    out.push(ScenarioResult::single(
        "2 hot-index build",
        "build sorted index of active SKUs",
        build_ms,
        serde_json::json!({
            "entries": index.len(),
            "estimated_bytes": bytes,
            "single_prefix_lookup_ms": lookup_ms,
        }),
        None,
    ));
    Ok(out)
}

/// Scenario 3 — keyset page of 200 rows at several depths.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any page query fails.
pub fn scenario_keyset_pages(
    db_path: &Path,
    size: &DatasetSize,
) -> Result<Vec<ScenarioResult>, BenchError> {
    let conn = open_db(db_path)?;
    let total: i64 = conn
        .query_row("SELECT COUNT(*) FROM sku", [], |row| row.get(0))
        .map_err(BenchError::Sql)?;
    let depths: Vec<(String, i64)> = vec![
        ("depth 0".to_string(), 0),
        ("depth 10%".to_string(), total / 10),
        ("depth 50%".to_string(), total / 2),
    ];
    let _ = size;
    let mut out = Vec::new();
    for (label, depth) in depths {
        // Find the key at this depth, then page from it.
        let key: String = conn
            .query_row(
                "SELECT id FROM sku ORDER BY id LIMIT 1 OFFSET ?1",
                params![depth],
                |row| row.get(0),
            )
            .map_err(BenchError::Sql)?;
        let (p50, p95, p99) = measure(20, || {
            let mut stmt = conn
                .prepare("SELECT id, code, name FROM sku WHERE id > ?1 ORDER BY id LIMIT 200")
                .map_err(BenchError::Sql)?;
            let rows = stmt
                .query_map(params![key], |row| {
                    Ok((
                        row.get::<_, String>(0)?,
                        row.get::<_, String>(1)?,
                        row.get::<_, String>(2)?,
                    ))
                })
                .map_err(BenchError::Sql)?;
            let mut count = 0;
            for row in rows {
                let _ = row.map_err(BenchError::Sql)?;
                count += 1;
            }
            let _ = count;
            Ok(())
        })?;
        out.push(ScenarioResult::from_histogram(
            &format!("3 keyset page ({label})"),
            "200 rows by keyset",
            p50,
            p95,
            p99,
            serde_json::json!({ "depth_rows": depth }),
            None,
        ));
    }
    Ok(out)
}

/// Run one 6-line sale commit — the full write path from `04` on the
/// REAL schema (001b-2): sale, lines, per-component tax rows
/// (`sale_line_tax`), payment, stock movements + balances, aggregates
/// (`daily_sales_summary`, `daily_item_sales`, `daily_cash_summary`),
/// audit, outbox (sale + one stock row per `SKU`), `receipt_counter`,
/// and the queued `print_job`. `run_id` makes every bench row unique
/// per run, so re-running the scenarios on the same database never
/// hits a UNIQUE constraint, and the bench aggregate day never touches
/// the seeded aggregates.
///
/// The sale is the real domain's Exclusive-tax shape: 6 lines of
/// 1 unit at 1.00 (gross 1000, tax 180 at 18%, total 1180); sale
/// subtotal 6000, tax 1080, total 7080, tendered in cash.
fn commit_one_sale(conn: &mut Connection, sale_no: u64, run_id: u64) -> Result<(), BenchError> {
    let tx = conn.transaction().map_err(BenchError::Sql)?;
    let total_minor: i64 = 7_080;
    let sale_id = format!("sale-bench-{run_id}-{sale_no}");
    tx.execute(
        "INSERT INTO sale (id, branch_id, terminal_id, shift_id, receipt_no, status, \
         customer_id, draft_id, subtotal_minor, discount_minor, tax_minor, total_minor, \
         completed_at, tenant_id, created_at, updated_at, version) \
         VALUES (?1, 'bench-branch', 'bench-terminal', 'bench-shift', ?2, 'completed', \
         NULL, NULL, 6000, 0, 1080, ?3, 1790000000000, 'bench-tenant', 0, 0, 1)",
        params![sale_id, format!("RB-{run_id}-{sale_no:08}"), total_minor],
    )
    .map_err(BenchError::Sql)?;
    for line in 0..6 {
        commit_sale_line(&tx, &sale_id, sale_no, run_id, line)?;
    }
    commit_sale_tail(&tx, &sale_id, sale_no, run_id, total_minor)?;
    tx.commit().map_err(BenchError::Sql)?;
    Ok(())
}

/// One line of the bench sale: `sale_line`, its `sale_line_tax` row,
/// the stock movement + balance, the item aggregate, and the stock
/// outbox row.
fn commit_sale_line(
    tx: &Transaction<'_>,
    sale_id: &str,
    sale_no: u64,
    run_id: u64,
    line: u64,
) -> Result<(), BenchError> {
    let sku = format!("sku-{line}");
    tx.execute(
        "INSERT INTO sale_line (id, sale_id, sku_id, qty_milli, unit_price_e4, \
         discount_minor, tax_minor, total_minor, tenant_id, created_at, updated_at, version) \
         VALUES (?1, ?2, ?3, 1000, 10000, 0, 180, 1180, 'bench-tenant', 0, 0, 1)",
        params![
            format!("line-bench-{run_id}-{sale_no}-{line}"),
            sale_id,
            sku
        ],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO sale_line_tax (id, tenant_id, sale_line_id, tax_rate_id, tax_minor, \
         rate_bp, created_at, updated_at, version) \
         VALUES (?1, 'bench-tenant', ?2, 'taxrate-std-18', 180, 1800, 0, 0, 1)",
        params![
            format!("slt-bench-{run_id}-{sale_no}-{line}"),
            format!("line-bench-{run_id}-{sale_no}-{line}")
        ],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO stock_movement (id, branch_id, sku_id, batch_id, type, qty_milli, \
         unit_cost_e4, ref_type, ref_id, occurred_at, tenant_id, created_at, updated_at, version) \
         VALUES (?1, 'bench-branch', ?2, NULL, 'sale', -1000, 10000, 'sale', ?3, \
         1790000000000, 'bench-tenant', 0, 0, 1)",
        params![format!("mov-bench-{run_id}-{sale_no}-{line}"), sku, sale_id],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO stock_balance (id, tenant_id, branch_id, sku_id, qty_milli, avg_cost_e4, \
         created_at, updated_at, version) VALUES (?1, 'bench-tenant', 'bench-branch', ?2, \
         -1000, 10000, 0, 0, 1) \
         ON CONFLICT(tenant_id, branch_id, sku_id) DO UPDATE SET qty_milli = qty_milli - 1000",
        params![format!("bal-bench-{run_id}-{sale_no}-{line}"), sku],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO daily_item_sales (id, tenant_id, branch_id, sku_id, date, qty_milli, \
         total_minor, created_at, updated_at, version) \
         VALUES (?1, 'bench-tenant', 'bench-branch', ?2, ?3, 1000, 1180, 0, 0, 1) \
         ON CONFLICT(tenant_id, branch_id, sku_id, date) DO UPDATE SET \
         qty_milli = qty_milli + 1000, total_minor = total_minor + 1180",
        params![
            format!("dis-bench-{run_id}-{sale_no}-{line}"),
            sku,
            format!("bench-day-{run_id}")
        ],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO outbox (id, tenant_id, entity, entity_id, payload, created_at, \
         updated_at, sent_at, attempts, version) \
         VALUES (?1, 'bench-tenant', 'stock', ?2, '{}', 1790000000000, 1790000000000, \
         NULL, 0, 1)",
        params![format!("outbox-stock-bench-{run_id}-{sale_no}-{line}"), sku],
    )
    .map_err(BenchError::Sql)?;
    Ok(())
}

/// The sale-level rows: payment, daily aggregates, the audit row, the
/// sale outbox row, the receipt counter, and the queued `print_job`.
fn commit_sale_tail(
    tx: &Transaction<'_>,
    sale_id: &str,
    sale_no: u64,
    run_id: u64,
    total_minor: i64,
) -> Result<(), BenchError> {
    tx.execute(
        "INSERT INTO sale_payment (id, sale_id, method, amount_minor, reference, tenant_id, \
         created_at, updated_at, version) VALUES (?1, ?2, 'cash', ?3, NULL, 'bench-tenant', 0, 0, 1)",
        params![
            format!("pay-bench-{run_id}-{sale_no}"),
            sale_id,
            total_minor
        ],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO daily_sales_summary (id, tenant_id, branch_id, date, sale_count, \
         subtotal_minor, discount_minor, tax_minor, total_minor, created_at, updated_at, version) \
         VALUES (?1, 'bench-tenant', 'bench-branch', ?2, 1, 6000, 0, 1080, ?3, 0, 0, 1) \
         ON CONFLICT(tenant_id, branch_id, date) DO UPDATE SET \
         subtotal_minor = subtotal_minor + 6000, tax_minor = tax_minor + 1080, \
         total_minor = total_minor + excluded.total_minor, sale_count = sale_count + 1",
        params![
            format!("dss-bench-{run_id}"),
            format!("bench-day-{run_id}"),
            total_minor
        ],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO daily_cash_summary (id, tenant_id, branch_id, terminal_id, date, \
         opening_minor, cash_sales_minor, cash_in_minor, cash_out_minor, expected_minor, \
         counted_minor, created_at, updated_at, version) \
         VALUES (?1, 'bench-tenant', 'bench-branch', 'bench-terminal', ?2, 0, ?3, 0, 0, \
         ?3, NULL, 0, 0, 1) \
         ON CONFLICT(tenant_id, branch_id, date) DO UPDATE SET \
         cash_sales_minor = cash_sales_minor + excluded.cash_sales_minor, \
         expected_minor = expected_minor + excluded.expected_minor",
        params![
            format!("dcs-bench-{run_id}"),
            format!("bench-day-{run_id}"),
            total_minor
        ],
    )
    .map_err(BenchError::Sql)?;
    // Synthetic load rows (terminal_id NULL, NULL hashes): the chain
    // invariant applies to rows the app writes via the audit writer,
    // not the bench.
    tx.execute(
        "INSERT INTO audit_log (id, tenant_id, terminal_id, user_id, action, entity, entity_id, \
         before_json, after_json, reason, at, prev_hash, row_hash, created_at, updated_at, version) \
         VALUES (?1, 'bench-tenant', NULL, 'bench-user', 'sale.completed', \
         'sale', ?2, NULL, NULL, 'bench', 1790000000000, NULL, NULL, 0, 0, 1)",
        params![format!("audit-bench-{run_id}-{sale_no}"), sale_id],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO outbox (id, tenant_id, entity, entity_id, payload, created_at, \
         updated_at, sent_at, attempts, version) \
         VALUES (?1, 'bench-tenant', 'sale', ?2, '{}', 1790000000000, 1790000000000, NULL, 0, 1)",
        params![format!("outbox-bench-{run_id}-{sale_no}"), sale_id],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO receipt_counter (id, tenant_id, terminal_id, date, last_seq, created_at, \
         updated_at, version) VALUES (?1, 'bench-tenant', 'bench-terminal', '20261111', 1, 0, 0, 1) \
         ON CONFLICT(tenant_id, terminal_id, date) DO UPDATE SET last_seq = last_seq + 1",
        params![format!("rc-bench-{run_id}")],
    )
    .map_err(BenchError::Sql)?;
    tx.execute(
        "INSERT INTO print_job (id, tenant_id, terminal_id, sale_id, payload_json, status, \
         attempts, created_at, updated_at, version) \
         VALUES (?1, 'bench-tenant', 'bench-terminal', ?2, '{}', 'queued', 0, 0, 0, 1)",
        params![format!("print-bench-{run_id}-{sale_no}"), sale_id],
    )
    .map_err(BenchError::Sql)?;
    Ok(())
}

/// Scenario 4 — sale commit transaction (6 lines, full write path).
/// On the REAL schema since 001b-2 (the throwaway mirrored schema was
/// deleted with the switch).
///
/// # Errors
/// Returns [`BenchError::Sql`] if any commit fails.
pub fn scenario_sale_commit(db_path: &Path, run_id: u64) -> Result<ScenarioResult, BenchError> {
    let mut conn = open_db(db_path)?;
    let mut sale_no: u64 = 0;
    let (p50, p95, p99) = measure(100, || {
        sale_no += 1;
        commit_one_sale(&mut conn, sale_no, run_id)
    })?;
    Ok(ScenarioResult::from_histogram(
        "4 sale-commit (real schema)",
        "6-line sale, one transaction, synchronous=FULL",
        p50,
        p95,
        p99,
        serde_json::json!({ "commits": 100 }),
        Some(("< 200 ms (tender → commit)", 200.0)),
    ))
}

/// Scenario 5 — sale commit while a background writer inserts batches of
/// 200 rows (simulated sync) and a read-heavy report runs.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any commit fails.
pub fn scenario_sale_commit_under_load(
    db_path: &Path,
    run_id: u64,
) -> Result<ScenarioResult, BenchError> {
    // Open the main connection BEFORE spawning the load threads: the
    // writer's first batch used to race this open, and the journal-mode
    // change plus the first commit then contended for the write lock.
    let mut conn = open_db(db_path)?;
    let stop = Arc::new(AtomicBool::new(false));
    let writer_stop = Arc::clone(&stop);
    let writer_path = db_path.to_path_buf();
    let writer = std::thread::spawn(move || {
        let mut n: u64 = 0;
        while !writer_stop.load(Ordering::Relaxed) {
            if let Ok(mut conn) = open_db(&writer_path) {
                // One transaction per 200-row batch, not 200 autocommit
                // INSERTs. Each autocommit INSERT fsyncs (synchronous=FULL),
                // so 200 of them back-to-back held/re-acquired the write
                // lock for seconds on a slow disk and starved the main
                // thread's commit past its 5 s busy_timeout — the
                // "database is locked" flake. One batch = one fsync, then
                // the lock is released for the sleep, giving the main
                // thread a clean window every batch.
                if let Ok(tx) = conn.transaction() {
                    let mut i = 0;
                    while i < 200 {
                        let _ = tx.execute(
                            "INSERT INTO bench_scratch (payload) VALUES (?1)",
                            params![format!("load-{n}")],
                        );
                        i += 1;
                        n += 1;
                    }
                    let _ = tx.commit();
                }
            }
            std::thread::sleep(Duration::from_millis(10));
        }
    });
    let reader_stop = Arc::clone(&stop);
    let reader_path = db_path.to_path_buf();
    let reader = std::thread::spawn(move || {
        while !reader_stop.load(Ordering::Relaxed) {
            if let Ok(conn) = open_db(&reader_path) {
                let _: Result<i64, _> = conn.query_row(
                    "SELECT COALESCE(SUM(total_minor), 0) FROM daily_sales_summary",
                    [],
                    |row| row.get(0),
                );
            }
            std::thread::sleep(Duration::from_millis(50));
        }
    });

    let mut sale_no: u64 = 1_000_000;
    let result = measure(50, || {
        sale_no += 1;
        commit_one_sale(&mut conn, sale_no, run_id)
    });
    stop.store(true, Ordering::Relaxed);
    let _ = writer.join();
    let _ = reader.join();
    let (p50, p95, p99) = result?;
    Ok(ScenarioResult::from_histogram(
        "5 sale-commit under load",
        "6-line sale while sync writer + report run",
        p50,
        p95,
        p99,
        serde_json::json!({ "commits": 50 }),
        Some(("< 200 ms (tender → commit)", 200.0)),
    ))
}

/// Scenario 6 — 90-day report from aggregates vs from raw `sale_line`.
///
/// # Errors
/// Returns [`BenchError::Sql`] if either report query fails.
pub fn scenario_reports(db_path: &Path) -> Result<Vec<ScenarioResult>, BenchError> {
    let conn = open_db(db_path)?;
    let mut out = Vec::new();
    let started = Instant::now();
    let agg_total: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0) FROM daily_sales_summary",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    let agg_ms = started.elapsed().as_secs_f64() * 1_000.0;
    out.push(ScenarioResult::single(
        "6 report (aggregates)",
        "90-day sales from daily_sales_summary",
        agg_ms,
        serde_json::json!({ "total_minor": agg_total }),
        Some(("< 1 s", 1_000.0)),
    ));
    let started = Instant::now();
    let raw_total: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0) FROM sale_line",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    let raw_ms = started.elapsed().as_secs_f64() * 1_000.0;
    out.push(ScenarioResult::single(
        "6 report (raw sale_line)",
        "same range scanned from raw lines (why aggregates exist)",
        raw_ms,
        serde_json::json!({ "total_minor": raw_total }),
        Some(("< 3 s (server budget, informational)", 3_000.0)),
    ));
    Ok(out)
}

/// Scenario 7 — startup, from UI results (the debug route measures it).
/// Not measured by the runner itself.
#[must_use]
pub fn scenario_startup(ui: Option<&UiResults>) -> ScenarioResult {
    match ui {
        Some(results) => ScenarioResult::single(
            "7 startup",
            "process start → main window interactive",
            results.startup_ms,
            serde_json::json!({ "source": "debug route" }),
            Some(("< 4 s on weakest PC", 4_000.0)),
        ),
        None => ScenarioResult::not_measured(
            "7 startup",
            "process start → main window interactive",
            "run the debug route (Ctrl+Shift+B) and pass --ui-results",
        ),
    }
}

/// Scenario 8 — UI in `WebView2`, from UI results (the debug route measures it).
#[must_use]
pub fn scenario_ui(ui: Option<&UiResults>) -> Vec<ScenarioResult> {
    match ui {
        Some(results) => vec![
            ScenarioResult::single(
                "8 UI scroll (100k grid)",
                "frame p95 over 10 s scroll",
                results.scroll_p95_ms,
                serde_json::json!({ "long_tasks_over_50ms": results.long_tasks }),
                Some(("60 fps = 16.7 ms frame", 16.7)),
            ),
            ScenarioResult::single(
                "8 UI cart",
                "re-renders per row adding 50 rows",
                results.cart_rerenders_per_row,
                serde_json::json!({ "unit": "renders/row" }),
                None,
            ),
            ScenarioResult::single(
                "8 UI route change",
                "first paint of the debug view",
                results.route_change_ms,
                serde_json::json!({}),
                Some(("< 150 ms to first paint", 150.0)),
            ),
        ],
        None => vec![ScenarioResult::not_measured(
            "8 UI (WebView2)",
            "scroll / cart / route change",
            "run the debug route (Ctrl+Shift+B) and pass --ui-results",
        )],
    }
}

/// Read peak RSS (`VmHWM`) and current RSS (`VmRSS`) in MiB, Linux only.
fn linux_memory_mb() -> (f64, f64) {
    let mut peak = 0.0;
    let mut current = 0.0;
    if let Ok(status) = std::fs::read_to_string("/proc/self/status") {
        for line in status.lines() {
            if let Some(value) = line.strip_prefix("VmHWM:") {
                let digits: String = value.chars().filter(char::is_ascii_digit).collect();
                if let Ok(kb) = digits.parse::<f64>() {
                    peak = kb / 1024.0;
                }
            }
            if let Some(value) = line.strip_prefix("VmRSS:") {
                let digits: String = value.chars().filter(char::is_ascii_digit).collect();
                if let Ok(kb) = digits.parse::<f64>() {
                    current = kb / 1024.0;
                }
            }
        }
    }
    (peak, current)
}

/// CPU idle % over a 1 s sample, Linux only.
fn linux_cpu_idle_pct() -> f64 {
    let read_cpu = || -> Option<(u64, u64)> {
        let stat = std::fs::read_to_string("/proc/stat").ok()?;
        let line = stat.lines().next()?;
        let mut parts = line.split_whitespace();
        if parts.next()? != "cpu" {
            return None;
        }
        let mut total: u64 = 0;
        let mut idle: u64 = 0;
        for (i, part) in parts.enumerate() {
            let value: u64 = part.parse().ok()?;
            total += value;
            if i == 3 {
                idle = value;
            }
        }
        Some((idle, total))
    };
    let Some((idle0, total0)) = read_cpu() else {
        return 0.0;
    };
    std::thread::sleep(Duration::from_secs(1));
    let Some((idle1, total1)) = read_cpu() else {
        return 0.0;
    };
    let idle_diff = idle1.saturating_sub(idle0);
    let total_diff = total1.saturating_sub(total0);
    if total_diff == 0 {
        0.0
    } else {
        #[allow(clippy::cast_precision_loss)]
        let pct = idle_diff as f64 / total_diff as f64 * 100.0;
        pct
    }
}

/// Scenario 9 — memory: runner peak/current RSS + DB file size + CPU idle.
/// The 8 h soak is a later release gate, not this run. Cannot fail: every
/// measurement degrades to a zero with an explanatory `extra` field.
#[must_use]
pub fn scenario_memory(db_path: &Path) -> ScenarioResult {
    let (peak_mb, current_mb) = linux_memory_mb();
    let db_bytes = std::fs::metadata(db_path).map_or(0, |m| m.len());
    let cpu_idle = linux_cpu_idle_pct();
    ScenarioResult::single(
        "9 memory",
        "runner peak RSS",
        peak_mb,
        serde_json::json!({
            "peak_rss_mb": peak_mb,
            "current_rss_mb": current_mb,
            "db_bytes": db_bytes,
            "cpu_idle_pct": cpu_idle,
            "unit": "MiB RSS (this process only; 8 h soak is a later gate)",
        }),
        Some(("< 400 MB idle, all processes (proxy: this process)", 400.0)),
    )
}

/// One installer artifact: display name, size in bytes, and its ADR-016
/// budget as (label, threshold in MiB), when one applies.
type ArtifactSize<'a> = (String, u64, Option<(&'a str, f64)>);

/// Scenario 10 — installer sizes from a directory of Windows artifacts.
/// Each artifact is checked against its own ADR-016 budget.
#[must_use]
pub fn scenario_installers(artifacts_dir: Option<&Path>) -> ScenarioResult {
    let Some(dir) = artifacts_dir else {
        return ScenarioResult::not_measured(
            "10 installer sizes",
            "exe / NSIS / MSI",
            "pass --artifacts-dir with the Windows build outputs",
        );
    };
    let mut sizes: Vec<ArtifactSize<'_>> = Vec::new();
    let exe = dir.join("shopcore-desktop.exe");
    let nsis = dir.join("ShopCore_0.1.0_x64-setup.exe");
    let msi = dir.join("ShopCore_0.1.0_x64_en-US.msi");
    if let Ok(meta) = std::fs::metadata(&exe) {
        sizes.push(("exe".to_string(), meta.len(), None));
    }
    if let Ok(meta) = std::fs::metadata(&nsis) {
        sizes.push((
            "NSIS setup".to_string(),
            meta.len(),
            Some(("< 60 MB online bootstrapper excl. WebView2", 60.0)),
        ));
    }
    if let Ok(meta) = std::fs::metadata(&msi) {
        sizes.push((
            "MSI".to_string(),
            meta.len(),
            Some(("~150 MB offline installer", 150.0)),
        ));
    }
    if sizes.is_empty() {
        return ScenarioResult::not_measured(
            "10 installer sizes",
            "exe / NSIS / MSI",
            "no Windows artifacts found in --artifacts-dir",
        );
    }
    let mut details = serde_json::Map::new();
    let mut worst_pass: Option<bool> = Some(true);
    let mut worst_mb: f64 = 0.0;
    for (name, bytes, budget) in &sizes {
        #[allow(clippy::cast_precision_loss)]
        let mb = *bytes as f64 / 1_048_576.0;
        details.insert(
            (*name).clone(),
            serde_json::json!({ "bytes": bytes, "mb": mb }),
        );
        if mb > worst_mb {
            worst_mb = mb;
        }
        if let Some((_, threshold)) = budget
            && mb > *threshold
        {
            worst_pass = Some(false);
        }
    }
    ScenarioResult {
        name: "10 installer sizes".to_string(),
        metric: "largest artifact".to_string(),
        unit: "MiB".to_string(),
        p50_ms: worst_mb,
        p95_ms: worst_mb,
        p99_ms: worst_mb,
        extra: serde_json::Value::Object(details),
        budget: Some("NSIS < 60 MB / MSI ~150 MB (ADR-016)".to_string()),
        pass: worst_pass,
        not_measured: false,
    }
}

/// Warm the OS page cache before measuring: one barcode lookup, one FTS5
/// search, one keyset page, one sale commit, both report queries. Without
/// this, first-touch operations measure cold-cache disk reads and the
/// numbers swing 2–3x run to run on a shared machine.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any warm-up query fails.
pub fn warmup(db_path: &Path, run_id: u64) -> Result<(), BenchError> {
    let mut conn = open_db(db_path)?;
    let _: Option<String> = conn
        .query_row(
            "SELECT sku_id FROM barcode WHERE tenant_id = 'bench-tenant' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let _: Option<String> = conn
        .query_row(
            "SELECT name FROM sku_fts WHERE sku_fts MATCH 'Ri*' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let _: Option<String> = conn
        .query_row(
            "SELECT id FROM sku WHERE id > 'sku-0' ORDER BY id LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();
    let _: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0) FROM daily_sales_summary",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    let _: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0) FROM sale_line",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    commit_one_sale(&mut conn, 0, run_id)?;
    Ok(())
}

/// Open the bench DB with the app's durability settings. Public so the
/// `verify` subcommand (the binary) can open a database the same way
/// the scenarios do.
///
/// `busy_timeout` is set FIRST: every later statement waits instead of
/// failing instantly on contention. The journal mode is VERIFIED, not
/// changed: `PRAGMA journal_mode=WAL` does not honor `busy_timeout` —
/// it fails instantly with `SQLITE_BUSY` if another connection holds a
/// lock — so changing it here was the "database is locked" flake.
/// Every bench DB is created in WAL mode by the seeder (`schema.rs`),
/// so a non-WAL database means the seeder was not run; fail loudly
/// instead of silently running in rollback-journal mode.
///
/// # Errors
/// Returns [`BenchError::Sql`] if the database cannot be opened or a
/// pragma fails, [`BenchError::Usage`] if the database is not in WAL mode.
pub fn open_db(path: &Path) -> Result<Connection, BenchError> {
    let conn = Connection::open(path).map_err(BenchError::Sql)?;
    conn.execute_batch(
        "PRAGMA busy_timeout=5000; PRAGMA synchronous=FULL; PRAGMA foreign_keys=ON;",
    )
    .map_err(BenchError::Sql)?;
    let mode: String = conn
        .query_row("PRAGMA journal_mode", [], |row| row.get(0))
        .map_err(BenchError::Sql)?;
    if mode != "wal" {
        return Err(BenchError::Usage(format!(
            "database {} is not in WAL mode (journal_mode={mode}); seed it first",
            path.display()
        )));
    }
    Ok(conn)
}

/// The machine record for this run.
#[must_use]
pub fn machine_record() -> MachineRecord {
    MachineRecord::detect()
}
