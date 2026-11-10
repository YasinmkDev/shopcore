//! Deterministic seeder for the throwaway bench database.
//!
//! Same seed always builds the same dataset (verified by
//! [`checksum`]: seed twice, compare). All money is integer minor units;
//! all quantities are milli-units — the same rules as the real app.

use rand::rngs::StdRng;
use rand::{RngExt, SeedableRng};
use rusqlite::{Connection, params};

use crate::BenchError;

/// Fixed seed: the same dataset on every run, on every machine.
pub const DEFAULT_SEED: u64 = 0x5EED_C0DE;

/// Row counts for a dataset.
#[derive(Debug, Clone, Copy)]
pub struct DatasetSize {
    /// Total SKUs; `active_skus` of them are flagged active (the hot set).
    pub skus: u64,
    /// Active SKUs (the in-memory hot index in scenario 2).
    pub active_skus: u64,
    /// Barcodes (about 1.2 per SKU).
    pub barcodes: u64,
    /// Customers.
    pub customers: u64,
    /// Sales; each sale carries 1–8 lines.
    pub sales: u64,
    /// Sale lines (derived from `sales`, kept as a target, not a promise).
    pub sale_lines: u64,
    /// Stock movements.
    pub movements: u64,
}

impl DatasetSize {
    /// Spec 002 full dataset: 500k SKUs (100k active), 600k barcodes,
    /// 100k customers, ~1.2M sales / 5M lines, 2M movements.
    #[must_use]
    pub const fn full() -> Self {
        Self {
            skus: 500_000,
            active_skus: 100_000,
            barcodes: 600_000,
            customers: 100_000,
            sales: 1_200_000,
            sale_lines: 5_000_000,
            movements: 2_000_000,
        }
    }

    /// CI smoke dataset: 10k SKUs (2k active). Fast enough for every push.
    #[must_use]
    pub const fn smoke() -> Self {
        Self {
            skus: 10_000,
            active_skus: 2_000,
            barcodes: 12_000,
            customers: 2_000,
            sales: 24_000,
            sale_lines: 100_000,
            movements: 40_000,
        }
    }

    /// Tiny dataset for unit tests.
    #[must_use]
    pub const fn tiny() -> Self {
        Self {
            skus: 100,
            active_skus: 20,
            barcodes: 120,
            customers: 20,
            sales: 240,
            sale_lines: 1_000,
            movements: 400,
        }
    }
}

/// What [`seed`] produced.
#[derive(Debug, Clone, Copy, Default)]
pub struct SeedStats {
    /// SKUs inserted.
    pub skus: u64,
    /// Barcodes inserted.
    pub barcodes: u64,
    /// Customers inserted.
    pub customers: u64,
    /// Sales inserted.
    pub sales: u64,
    /// Sale lines inserted.
    pub sale_lines: u64,
    /// Stock movements inserted.
    pub movements: u64,
    /// Seeding wall time, milliseconds.
    pub elapsed_ms: u64,
}

const TENANT: &str = "bench-tenant";
const BRANCH: &str = "bench-branch";
const TERMINAL: &str = "bench-terminal";
const SHIFT: &str = "bench-shift";
/// Milliseconds in a day.
const DAY_MS: i64 = 86_400_000;
/// Fixed "now" (epoch ms) so runs are comparable.
const NOW_MS: i64 = 1_790_000_000_000;

/// Product words combined deterministically into SKU names.
const WORDS: &[&str] = &[
    "Rice",
    "Milk",
    "Bread",
    "Sugar",
    "Tea",
    "Oil",
    "Flour",
    "Soap",
    "Salt",
    "Lentils",
    "Pasta",
    "Eggs",
    "Butter",
    "Cheese",
    "Juice",
    "Biscuits",
    "Shampoo",
    "Detergent",
    "Toothpaste",
    "Coffee",
];
const SIZES: &[&str] = &[
    "1kg", "5kg", "1L", "500g", "250g", "2L", "10kg", "200ml", "1pc", "12pc",
];

/// EAN-13 check digit for a 12-digit base (GS1 algorithm).
fn ean13_check_digit(base12: &str) -> char {
    let mut sum: u32 = 0;
    for (i, byte) in base12.bytes().enumerate() {
        let digit = u32::from(byte.saturating_sub(b'0'));
        // Positions are 1-based from the left: odd positions ×1, even ×3.
        if i % 2 == 0 {
            sum += digit;
        } else {
            sum += digit * 3;
        }
    }
    let check = (10 - (sum % 10)) % 10;
    char::from(b'0' + u8::try_from(check).unwrap_or(0))
}

/// A deterministic 13-digit EAN barcode for index `i`.
fn ean13(i: u64) -> String {
    let base = format!("890{:09}", i % 1_000_000_000);
    let check = ean13_check_digit(&base);
    format!("{base}{check}")
}

/// Deterministic SKU name for index `i`.
fn sku_name(rng: &mut StdRng, i: u64) -> String {
    let word = WORDS
        .get(usize::try_from(i % WORDS.len() as u64).unwrap_or(0))
        .copied()
        .unwrap_or("Item");
    let size = SIZES
        .get(rng.random_range(0..SIZES.len()))
        .copied()
        .unwrap_or("1pc");
    format!("{word} {size}")
}

/// Seed `conn` with `size` rows from `seed_value`.
///
/// Seeding runs with `synchronous=NORMAL` for speed; the scenarios run
/// with `synchronous=FULL` (the app's setting), so measurements are honest.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any insert fails.
#[allow(clippy::too_many_lines)]
pub fn seed(
    conn: &mut Connection,
    size: &DatasetSize,
    seed_value: u64,
) -> Result<SeedStats, BenchError> {
    let started = std::time::Instant::now();
    let mut rng = StdRng::seed_from_u64(seed_value);
    // Speed: seeding is bulk load, not a measured scenario.
    conn.execute_batch("PRAGMA synchronous=NORMAL;")
        .map_err(BenchError::Sql)?;
    let mut stats = SeedStats::default();

    // SKUs + FTS rows.
    {
        let tx = conn.transaction().map_err(BenchError::Sql)?;
        {
            let mut sku = tx
                .prepare(
                    "INSERT INTO sku (id, tenant_id, code, name, base_uom_id, \
                     tax_group_id, track_batch, active, created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, ?4, 'each', 'std', 0, ?5, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut fts = tx
                .prepare("INSERT INTO sku_fts (name, code) VALUES (?1, ?2)")
                .map_err(BenchError::Sql)?;
            for i in 0..size.skus {
                let name = sku_name(&mut rng, i);
                let code = format!("SKU-{i:06}");
                let active = i < size.active_skus;
                sku.execute(params![format!("sku-{i}"), TENANT, code, name, active])
                    .map_err(BenchError::Sql)?;
                fts.execute(params![name, code]).map_err(BenchError::Sql)?;
            }
        }
        tx.commit().map_err(BenchError::Sql)?;
        stats.skus = size.skus;
    }

    // Barcodes (1.2 per SKU on average: every SKU gets one, every 5th a second).
    {
        let tx = conn.transaction().map_err(BenchError::Sql)?;
        {
            let mut ins = tx
                .prepare(
                    "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, \
                     pack_qty_milli, created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 1000, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut made: u64 = 0;
            let mut i: u64 = 0;
            while made < size.barcodes {
                let sku_i = i % size.skus;
                let primary = made == sku_i;
                ins.execute(params![
                    format!("barcode-{made}"),
                    TENANT,
                    format!("sku-{sku_i}"),
                    ean13(made),
                    primary
                ])
                .map_err(BenchError::Sql)?;
                made += 1;
                i += 1;
            }
        }
        tx.commit().map_err(BenchError::Sql)?;
        stats.barcodes = size.barcodes;
    }

    // Customers.
    {
        let tx = conn.transaction().map_err(BenchError::Sql)?;
        {
            let mut ins = tx
                .prepare(
                    "INSERT INTO customer (id, tenant_id, name, created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            for i in 0..size.customers {
                ins.execute(params![
                    format!("cust-{i}"),
                    TENANT,
                    format!("Customer {i}")
                ])
                .map_err(BenchError::Sql)?;
            }
        }
        tx.commit().map_err(BenchError::Sql)?;
        stats.customers = size.customers;
    }

    // Sales + lines + payments + movements + aggregates, in batches.
    // 90 days of history; `completed_at` is epoch ms.
    let mut sale_lines: u64 = 0;
    let mut movements: u64 = 0;
    let batch_sales: u64 = 10_000;
    let mut done_sales: u64 = 0;
    while done_sales < size.sales {
        let tx = conn.transaction().map_err(BenchError::Sql)?;
        {
            let mut sale = tx
                .prepare(
                    "INSERT INTO sale (id, branch_id, terminal_id, shift_id, receipt_no, \
                     status, customer_id, total_minor, completed_at, tenant_id, \
                     created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 'completed', ?6, ?7, ?8, ?9, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut line = tx
                .prepare(
                    "INSERT INTO sale_line (id, sale_id, sku_id, qty_milli, unit_price_e4, \
                     discount_minor, tax_minor, total_minor, tenant_id, created_at, \
                     updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut pay = tx
                .prepare(
                    "INSERT INTO sale_payment (id, sale_id, method, amount_minor, reference, \
                     tenant_id, created_at, updated_at, version) \
                     VALUES (?1, ?2, 'cash', ?3, NULL, ?4, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut mov = tx
                .prepare(
                    "INSERT INTO stock_movement (id, branch_id, sku_id, batch_id, type, \
                     qty_milli, unit_cost_e4, ref_type, ref_id, occurred_at, tenant_id, \
                     created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, NULL, 'sale', ?4, ?5, 'sale', ?6, ?7, ?8, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut agg = tx
                .prepare(
                    "INSERT INTO daily_sales_summary (day, branch_id, total_minor, sale_count, tenant_id) \
                     VALUES (?1, ?2, ?3, 1, ?4) \
                     ON CONFLICT(day, branch_id) DO UPDATE SET \
                     total_minor = total_minor + excluded.total_minor, \
                     sale_count = sale_count + 1",
                )
                .map_err(BenchError::Sql)?;
            let end = (done_sales + batch_sales).min(size.sales);
            for s in done_sales..end {
                let day_offset = i64::from(rng.random_range(0..90));
                let completed_at = NOW_MS - day_offset * DAY_MS;
                let day = format!("day-{}", 90 - day_offset);
                let customer = format!("cust-{}", rng.random_range(0..size.customers));
                let line_count = rng.random_range(1..=8);
                let mut total_minor: i64 = 0;
                for _ in 0..line_count {
                    let sku_i = rng.random_range(0..size.skus);
                    let qty_milli: i64 = i64::from(rng.random_range(1..=5) * 1000);
                    let unit_price_e4: i64 = i64::from(rng.random_range(50..=50_000));
                    let line_total = qty_milli * unit_price_e4 / 10_000;
                    let tax = line_total * 18 / 118;
                    line.execute(params![
                        format!("line-{sale_lines}"),
                        format!("sale-{s}"),
                        format!("sku-{sku_i}"),
                        qty_milli,
                        unit_price_e4,
                        tax,
                        line_total,
                        TENANT
                    ])
                    .map_err(BenchError::Sql)?;
                    mov.execute(params![
                        format!("mov-{movements}"),
                        BRANCH,
                        format!("sku-{sku_i}"),
                        -qty_milli,
                        unit_price_e4,
                        format!("sale-{s}"),
                        completed_at,
                        TENANT
                    ])
                    .map_err(BenchError::Sql)?;
                    total_minor += line_total;
                    sale_lines += 1;
                    movements += 1;
                }
                sale.execute(params![
                    format!("sale-{s}"),
                    BRANCH,
                    TERMINAL,
                    SHIFT,
                    format!("R-{s:08}"),
                    customer,
                    total_minor,
                    completed_at,
                    TENANT
                ])
                .map_err(BenchError::Sql)?;
                pay.execute(params![
                    format!("pay-{s}"),
                    format!("sale-{s}"),
                    total_minor,
                    TENANT
                ])
                .map_err(BenchError::Sql)?;
                agg.execute(params![day, BRANCH, total_minor, TENANT])
                    .map_err(BenchError::Sql)?;
            }
            done_sales = end;
        }
        tx.commit().map_err(BenchError::Sql)?;
    }
    stats.sales = size.sales;
    stats.sale_lines = sale_lines;
    stats.movements = movements;

    // Remaining movements (receipts) to reach the target count.
    if movements < size.movements {
        let tx = conn.transaction().map_err(BenchError::Sql)?;
        {
            let mut mov = tx
                .prepare(
                    "INSERT INTO stock_movement (id, branch_id, sku_id, batch_id, type, \
                     qty_milli, unit_cost_e4, ref_type, ref_id, occurred_at, tenant_id, \
                     created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, NULL, 'receipt', ?4, ?5, 'goods_receipt', ?6, ?7, ?8, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            while movements < size.movements {
                let sku_i = rng.random_range(0..size.skus);
                mov.execute(params![
                    format!("mov-{movements}"),
                    BRANCH,
                    format!("sku-{sku_i}"),
                    1000,
                    100,
                    format!("grn-{movements}"),
                    NOW_MS,
                    TENANT
                ])
                .map_err(BenchError::Sql)?;
                movements += 1;
            }
        }
        tx.commit().map_err(BenchError::Sql)?;
        stats.movements = movements;
    }

    // Stock balances from the movements ledger (one row per SKU touched).
    conn.execute_batch(
        "INSERT OR REPLACE INTO stock_balance \
         (branch_id, sku_id, qty_milli, avg_cost_e4, tenant_id, created_at, updated_at, version) \
         SELECT branch_id, sku_id, SUM(qty_milli), 100, tenant_id, 0, 0, 1 \
         FROM stock_movement GROUP BY branch_id, sku_id;",
    )
    .map_err(BenchError::Sql)?;

    // Restore the app's durability setting for the scenarios.
    conn.execute_batch("PRAGMA synchronous=FULL;")
        .map_err(BenchError::Sql)?;
    stats.elapsed_ms = u64::try_from(started.elapsed().as_millis()).unwrap_or(0);
    Ok(stats)
}

/// Deterministic checksum of a seeded database: per-table counts plus sums
/// of a numeric column. Seeding twice with the same seed must produce the
/// same string.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any count query fails.
pub fn checksum(conn: &Connection) -> Result<String, BenchError> {
    let mut parts: Vec<String> = Vec::new();
    for table in [
        "sku",
        "barcode",
        "customer",
        "sale",
        "sale_line",
        "stock_movement",
    ] {
        let count: i64 = conn
            .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                row.get(0)
            })
            .map_err(BenchError::Sql)?;
        parts.push(format!("{table}={count}"));
    }
    let sale_sum: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0) FROM sale",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    let line_sum: i64 = conn
        .query_row(
            "SELECT COALESCE(SUM(total_minor), 0) FROM sale_line",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    parts.push(format!("sale_sum={sale_sum}"));
    parts.push(format!("line_sum={line_sum}"));
    Ok(parts.join(","))
}
