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
    /// Sales; each sale carries 1–8 lines.
    pub sales: u64,
    /// Sale lines (derived from `sales`, kept as a target, not a promise).
    pub sale_lines: u64,
    /// Stock movements.
    pub movements: u64,
}

impl DatasetSize {
    /// Spec 002 full dataset: 500k SKUs (100k active), 600k barcodes,
    /// ~1.2M sales / 5M lines, 2M movements. (No `customer` table in
    /// the real schema — seeded sales have `customer_id` NULL.)
    #[must_use]
    pub const fn full() -> Self {
        Self {
            skus: 500_000,
            active_skus: 100_000,
            barcodes: 600_000,
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

/// How many rows per transaction while seeding (also the resume
/// granularity: a killed run leaves whole batches committed, so a
/// resume never has to redo a partial batch).
const SEED_BATCH: u64 = 10_000;

fn existing_count(conn: &Connection, table: &str) -> Result<u64, BenchError> {
    let n: i64 = conn
        .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
            row.get(0)
        })
        .map_err(BenchError::Sql)?;
    Ok(u64::try_from(n).unwrap_or(0))
}

/// One generated sale: RNG draws only, no DB. Extracted so a resume can
/// replay the draws for already-seeded sales (keeping the RNG stream
/// aligned) without inserting them.
struct GeneratedLine {
    sku_i: u64,
    qty_milli: i64,
    unit_price_e4: i64,
    tax: i64,
    line_total: i64,
}

struct GeneratedSale {
    completed_at: i64,
    day: String,
    lines: Vec<GeneratedLine>,
    /// Net (line totals minus their tax) — the sale's `subtotal_minor`.
    subtotal_minor: i64,
    /// Sum of the lines' tax — the sale's `tax_minor`.
    tax_minor: i64,
    total_minor: i64,
}

fn generate_sale(rng: &mut StdRng, size: &DatasetSize) -> GeneratedSale {
    let day_offset = i64::from(rng.random_range(0..90));
    let completed_at = NOW_MS - day_offset * DAY_MS;
    let day = format!("day-{}", 90 - day_offset);
    let line_count = rng.random_range(1..=8);
    let mut lines = Vec::new();
    let mut total_minor: i64 = 0;
    let mut tax_minor: i64 = 0;
    for _ in 0..line_count {
        let sku_i = rng.random_range(0..size.skus);
        let qty_milli: i64 = i64::from(rng.random_range(1..=5) * 1000);
        let unit_price_e4: i64 = i64::from(rng.random_range(50..=50_000));
        let line_total = qty_milli * unit_price_e4 / 10_000;
        let tax = line_total * 18 / 118;
        total_minor += line_total;
        tax_minor += tax;
        lines.push(GeneratedLine {
            sku_i,
            qty_milli,
            unit_price_e4,
            tax,
            line_total,
        });
    }
    GeneratedSale {
        completed_at,
        day,
        lines,
        subtotal_minor: total_minor - tax_minor,
        tax_minor,
        total_minor,
    }
}

/// Seed `conn` with `size` rows from `seed_value`.
///
/// Resumable: if `conn` already holds a partially seeded dataset (same
/// seed value), the phases that are already complete are skipped and
/// the sales phase continues from the last committed batch. The RNG
/// stream is replayed for skipped rows, so a resumed seed produces
/// the identical dataset a fresh seed would (same checksum).
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

    // Existing counts: what a previous (interrupted) seed already wrote.
    // Row IDs are sequential (sku-0.., barcode-0.., sale-0..), and every
    // phase commits in whole batches, so the count is the resume point.
    let have_skus = existing_count(conn, "sku")?.min(size.skus);
    let have_barcodes = existing_count(conn, "barcode")?.min(size.barcodes);
    let have_sales = existing_count(conn, "sale")?.min(size.sales);
    let have_lines = existing_count(conn, "sale_line")?;
    let have_movements = existing_count(conn, "stock_movement")?;
    // Sales batches are single transactions, so the sale count must sit
    // on a batch boundary; a partial batch means the database was
    // modified outside the seeder. Fail loudly instead of silently
    // producing a wrong dataset (re-seed with --fresh).
    if have_sales % SEED_BATCH != 0 && have_sales != size.sales {
        return Err(BenchError::Usage(format!(
            "sale count {have_sales} is not a batch multiple of {SEED_BATCH} — \
             the database was modified outside the seeder; re-seed with --fresh"
        )));
    }
    let resumed = have_skus > 0 || have_barcodes > 0 || have_sales > 0;
    if resumed {
        println!(
            "resuming seed: {have_skus} skus, {have_barcodes} barcodes, \
             {have_sales} sales already present"
        );
    }

    // Tax group + rate (the real schema's `sale_line_tax` rows reference
    // a `tax_rate`; every seeded `SKU` is in the `STD` 18% group).
    // `INSERT OR IGNORE` so a resume does not fail on the UNIQUE key.
    conn.execute_batch(
        "INSERT OR IGNORE INTO tax_group (id, tenant_id, code, name, created_at, updated_at, version) \
         VALUES ('tax-std', 'bench-tenant', 'STD', 'Standard 18%', 0, 0, 1); \
         INSERT OR IGNORE INTO tax_rate (id, tenant_id, tax_group_id, rate_bp, created_at, updated_at, version) \
         VALUES ('taxrate-std-18', 'bench-tenant', 'tax-std', 1800, 0, 0, 1);",
    )
    .map_err(BenchError::Sql)?;

    // SKUs + their `item` rows, in batches. The real `sku` has a
    // NOT NULL `item_id` FK, so each `SKU` gets an `item` first.
    // `sku_name` consumes one RNG draw per SKU, so it is called for
    // every index — replaying the draw for skipped rows keeps the
    // stream aligned. The FTS5 table is NOT written here: the 001b-1
    // triggers maintain `sku_fts` (and the catalog-version setting)
    // on every `sku`/`barcode` insert.
    {
        let mut i = 0u64;
        while i < size.skus {
            let end = (i + SEED_BATCH).min(size.skus);
            let tx = conn.transaction().map_err(BenchError::Sql)?;
            {
                let mut item = tx
                    .prepare(
                        "INSERT INTO item (id, tenant_id, code, name, base_uom_id, \
                         tax_group_id, track_batch, active, created_at, updated_at, version) \
                         VALUES (?1, ?2, ?3, ?4, 'each', 'tax-std', 0, ?5, 0, 0, 1)",
                    )
                    .map_err(BenchError::Sql)?;
                let mut sku = tx
                    .prepare(
                        "INSERT INTO sku (id, tenant_id, item_id, code, name, base_uom_id, \
                         tax_group_id, track_batch, active, created_at, updated_at, version) \
                         VALUES (?1, ?2, ?3, ?4, ?5, 'each', 'tax-std', 0, ?6, 0, 0, 1)",
                    )
                    .map_err(BenchError::Sql)?;
                for j in i..end {
                    let name = sku_name(&mut rng, j);
                    if j < have_skus {
                        continue;
                    }
                    let code = format!("SKU-{j:06}");
                    let active = j < size.active_skus;
                    item.execute(params![format!("item-{j}"), TENANT, code, name, active])
                        .map_err(BenchError::Sql)?;
                    sku.execute(params![
                        format!("sku-{j}"),
                        TENANT,
                        format!("item-{j}"),
                        code,
                        name,
                        active
                    ])
                    .map_err(BenchError::Sql)?;
                }
            }
            tx.commit().map_err(BenchError::Sql)?;
            i = end;
        }
        stats.skus = size.skus;
    }

    // Barcodes (1.2 per SKU on average: every SKU gets one, every 5th a second).
    // Deterministic by index (no RNG), so skipping by count is exact.
    {
        let mut made: u64 = 0;
        while made < size.barcodes {
            let end = (made + SEED_BATCH).min(size.barcodes);
            let tx = conn.transaction().map_err(BenchError::Sql)?;
            {
                let mut ins = tx
                    .prepare(
                        "INSERT INTO barcode (id, tenant_id, sku_id, code, is_primary, \
                         pack_qty_milli, created_at, updated_at, version) \
                         VALUES (?1, ?2, ?3, ?4, ?5, 1000, 0, 0, 1)",
                    )
                    .map_err(BenchError::Sql)?;
                let mut i: u64 = made;
                while made < end {
                    let sku_i = i % size.skus;
                    let primary = made == sku_i;
                    if made >= have_barcodes {
                        ins.execute(params![
                            format!("barcode-{made}"),
                            TENANT,
                            format!("sku-{sku_i}"),
                            ean13(made),
                            primary
                        ])
                        .map_err(BenchError::Sql)?;
                    }
                    made += 1;
                    i += 1;
                }
            }
            tx.commit().map_err(BenchError::Sql)?;
        }
        stats.barcodes = size.barcodes;
    }

    // Sales + lines + tax rows + payments + movements + aggregates, in batches.
    // 90 days of history; `completed_at` is epoch ms.
    // Line/movement IDs continue from the existing counts, so a resume
    // never reuses an ID. The RNG is replayed for already-seeded sales
    // (no inserts) so the stream stays aligned.
    let mut sale_lines: u64 = have_lines;
    let mut movements: u64 = have_movements;
    // Resume point for sales: the last committed batch boundary — or
    // the whole size when the dataset is already complete (a size
    // smaller than one batch would otherwise round down to 0 and
    // re-insert everything).
    let done_sales = if have_sales == size.sales {
        size.sales
    } else {
        (have_sales / SEED_BATCH) * SEED_BATCH
    };
    for _ in 0..done_sales {
        // These rows already exist; only the RNG draws matter here.
        // (Counters stay at the existing counts — new rows continue
        // from there.)
        let _ = generate_sale(&mut rng, size);
    }
    let mut done = done_sales;
    while done < size.sales {
        let end = (done + SEED_BATCH).min(size.sales);
        let tx = conn.transaction().map_err(BenchError::Sql)?;
        {
            let mut sale = tx
                .prepare(
                    "INSERT INTO sale (id, branch_id, terminal_id, shift_id, receipt_no, \
                     status, customer_id, draft_id, subtotal_minor, discount_minor, \
                     tax_minor, total_minor, completed_at, tenant_id, \
                     created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, ?4, ?5, 'completed', NULL, NULL, ?6, 0, \
                     ?7, ?8, ?9, ?10, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            let mut line = tx
                .prepare(
                    "INSERT INTO sale_line (id, sale_id, sku_id, qty_milli, unit_price_e4, \
                     discount_minor, tax_minor, total_minor, tenant_id, created_at, \
                     updated_at, version) VALUES (?1, ?2, ?3, ?4, ?5, 0, ?6, ?7, ?8, 0, 0, 1)",
                )
                .map_err(BenchError::Sql)?;
            // One `sale_line_tax` row per line (the real sale writes
            // one per tax component, in group order — here a single
            // 18% component per line).
            let mut line_tax = tx
                .prepare(
                    "INSERT INTO sale_line_tax (id, tenant_id, sale_line_id, tax_rate_id, \
                     tax_minor, rate_bp, created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, 'taxrate-std-18', ?4, 1800, 0, 0, 1)",
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
                    "INSERT INTO daily_sales_summary (id, tenant_id, branch_id, date, \
                     sale_count, subtotal_minor, discount_minor, tax_minor, total_minor, \
                     created_at, updated_at, version) \
                     VALUES (?1, ?2, ?3, ?4, 1, ?5, 0, ?6, ?7, 0, 0, 1) \
                     ON CONFLICT(tenant_id, branch_id, date) DO UPDATE SET \
                     subtotal_minor = subtotal_minor + excluded.subtotal_minor, \
                     tax_minor = tax_minor + excluded.tax_minor, \
                     total_minor = total_minor + excluded.total_minor, \
                     sale_count = sale_count + 1",
                )
                .map_err(BenchError::Sql)?;
            for s in done..end {
                let g = generate_sale(&mut rng, size);
                // The sale FIRST: `sale_line.sale_id` references
                // `sale(id)` (a real FK — the throwaway schema had
                // none, so lines could precede the sale there).
                sale.execute(params![
                    format!("sale-{s}"),
                    BRANCH,
                    TERMINAL,
                    SHIFT,
                    format!("R-{s:08}"),
                    g.subtotal_minor,
                    g.tax_minor,
                    g.total_minor,
                    g.completed_at,
                    TENANT
                ])
                .map_err(BenchError::Sql)?;
                for l in &g.lines {
                    line.execute(params![
                        format!("line-{sale_lines}"),
                        format!("sale-{s}"),
                        format!("sku-{}", l.sku_i),
                        l.qty_milli,
                        l.unit_price_e4,
                        l.tax,
                        l.line_total,
                        TENANT
                    ])
                    .map_err(BenchError::Sql)?;
                    line_tax
                        .execute(params![
                            format!("slt-{sale_lines}"),
                            TENANT,
                            format!("line-{sale_lines}"),
                            l.tax
                        ])
                        .map_err(BenchError::Sql)?;
                    // Sale-movement IDs share the line counter, not the
                    // total movement counter: receipts (generated after
                    // the sales) occupy the high IDs, so a resume that
                    // continued from the total count would collide with
                    // them. Sale movements are always mov-0..mov-(lines-1).
                    mov.execute(params![
                        format!("mov-{sale_lines}"),
                        BRANCH,
                        format!("sku-{}", l.sku_i),
                        -l.qty_milli,
                        l.unit_price_e4,
                        format!("sale-{s}"),
                        g.completed_at,
                        TENANT
                    ])
                    .map_err(BenchError::Sql)?;
                    sale_lines += 1;
                    movements += 1;
                }
                pay.execute(params![
                    format!("pay-{s}"),
                    format!("sale-{s}"),
                    g.total_minor,
                    TENANT
                ])
                .map_err(BenchError::Sql)?;
                agg.execute(params![
                    format!("dss-{}", g.day),
                    TENANT,
                    BRANCH,
                    g.day,
                    g.subtotal_minor,
                    g.tax_minor,
                    g.total_minor
                ])
                .map_err(BenchError::Sql)?;
            }
            done = end;
        }
        tx.commit().map_err(BenchError::Sql)?;
    }
    stats.sales = size.sales;
    stats.sale_lines = sale_lines;
    stats.movements = movements;

    // Remaining movements (receipts) to reach the target count. Each
    // receipt consumes one RNG draw (sku); replay the draws for
    // receipts already present (movements beyond the sale lines).
    let receipts_done = have_movements.saturating_sub(have_lines);
    for _ in 0..receipts_done {
        let _ = rng.random_range(0..size.skus);
    }
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
    // INSERT OR REPLACE makes this idempotent: a resume just recomputes it.
    // The real `stock_balance` has an `id` column and its PK is
    // (tenant_id, branch_id, sku_id) — the id is derived from the SKU.
    conn.execute_batch(
        "INSERT OR REPLACE INTO stock_balance \
         (id, tenant_id, branch_id, sku_id, qty_milli, avg_cost_e4, created_at, updated_at, version) \
         SELECT 'bal-' || sku_id, tenant_id, branch_id, sku_id, SUM(qty_milli), 100, 0, 0, 1 \
         FROM stock_movement GROUP BY tenant_id, branch_id, sku_id;",
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
    // No `customer` table in the real schema (seeded sales have
    // `customer_id` NULL).
    for table in ["sku", "barcode", "sale", "sale_line", "stock_movement"] {
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

/// Result of [`verify`]: the dataset checksum plus the per-SKU balance
/// checks. All counts must be zero for the dataset to be trusted.
#[derive(Debug)]
pub struct VerifyReport {
    /// The [`checksum`] string (counts + sums).
    pub checksum: String,
    /// Balance rows whose `qty_milli` differs from the sum of their
    /// stock movements (must be 0).
    pub balance_mismatches: i64,
    /// Distinct (branch, sku) pairs in `stock_movement` with no
    /// `stock_balance` row (must be 0).
    pub movements_without_balance: i64,
    /// Whether `sale_sum == line_sum` (must be true).
    pub sale_line_sums_match: bool,
}

/// Verify a seeded (or bench-run) database: the checksum, that every
/// sale total equals the sum of its lines, and — the strong check —
/// that every per-SKU `stock_balance.qty_milli` equals the sum of that
/// SKU's `stock_movement` rows, and that every moved SKU has a balance
/// row. Used by the `verify` subcommand and by the recovery proof: the
/// full dataset recovered after the 2026-11-10 VM restart must pass
/// this before its numbers are trusted.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any check query fails.
pub fn verify(conn: &Connection) -> Result<VerifyReport, BenchError> {
    let checksum = checksum(conn)?;
    let (sale_sum, line_sum): (i64, i64) = conn
        .query_row(
            "SELECT (SELECT COALESCE(SUM(total_minor), 0) FROM sale), \
             (SELECT COALESCE(SUM(total_minor), 0) FROM sale_line)",
            [],
            |row| Ok((row.get(0)?, row.get(1)?)),
        )
        .map_err(BenchError::Sql)?;
    // Per-SKU check. Aggregate the movements ONCE into a temp table
    // (one sequential scan), then join it to the balances twice. The
    // first version did a correlated lookup per balance row (500k
    // random index probes into 5.4M movements) and took 10+ minutes
    // on this disk; the aggregate-first version is one scan.
    // `NOT INDEXED` is load-bearing: without it the planner uses
    // `idx_movement_branch_sku_time` for index-ordered grouping, but
    // `qty_milli` is not in that index, so it does one random table
    // lookup per movement (5.4M lookups, ~26 GB re-read, 20+ min).
    // Forcing a sequential scan + temp b-tree sort is one pass
    // (same trap as the 2026-11-10 seed recovery aggregation).
    conn.execute_batch(
        "CREATE TEMP TABLE movement_sums AS \
         SELECT branch_id, sku_id, SUM(qty_milli) AS s \
         FROM stock_movement NOT INDEXED GROUP BY branch_id, sku_id;",
    )
    .map_err(BenchError::Sql)?;
    let balance_mismatches: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM stock_balance b \
             LEFT JOIN movement_sums g \
               ON g.branch_id = b.branch_id AND g.sku_id = b.sku_id \
             WHERE b.qty_milli != COALESCE(g.s, 0)",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    let movements_without_balance: i64 = conn
        .query_row(
            "SELECT COUNT(*) FROM movement_sums g \
             LEFT JOIN stock_balance b \
               ON b.branch_id = g.branch_id AND b.sku_id = g.sku_id \
             WHERE b.sku_id IS NULL",
            [],
            |row| row.get(0),
        )
        .map_err(BenchError::Sql)?;
    Ok(VerifyReport {
        checksum,
        balance_mismatches,
        movements_without_balance,
        sale_line_sums_match: sale_sum == line_sum,
    })
}
