#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::indexing_slicing
)]

//! Migration tests (spec 001b-1): migrations apply cleanly on a fresh DB,
//! in order, and are idempotent on re-run. Only the tables in req. 1
//! exist — `cash_movement`, `uom_conversion`, `brand`, `category` are
//! dropped and must never appear.

use shopcore_db::{Db, migrations::run_migrations};

const EXPECTED_TABLES: &[&str] = &[
    "branch",
    "terminal",
    "item",
    "sku",
    "barcode",
    "uom",
    "tax_group",
    "tax_rate",
    "price_list",
    "price_list_entry",
    "sale",
    "sale_line",
    "sale_payment",
    "sale_line_tax",
    "stock_movement",
    "stock_balance",
    "shift",
    "user",
    "role",
    "settings",
    "receipt_counter",
    "print_job",
    "audit_log",
    "outbox",
    "daily_sales_summary",
    "daily_item_sales",
    "daily_cash_summary",
    "draft_sale",
    "draft_line",
    "schema_migrations",
];

const DROPPED_TABLES: &[&str] = &["cash_movement", "uom_conversion", "brand", "category"];

fn table_names(db: &Db) -> Vec<String> {
    db.read(|conn| {
        let mut stmt = conn.prepare(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%' AND name NOT LIKE 'sku_fts_%' ORDER BY name",
        )?;
        let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
        let mut names = Vec::new();
        for row in rows {
            names.push(row?);
        }
        Ok(names)
    })
    .unwrap()
}

#[test]
fn migrations_create_only_the_spec_tables() {
    let db = Db::open_in_memory().unwrap();
    let names = table_names(&db);
    for table in EXPECTED_TABLES {
        assert!(
            names.iter().any(|n| n == table),
            "expected table {table} missing; got {names:?}"
        );
    }
    for table in DROPPED_TABLES {
        assert!(
            !names.iter().any(|n| n == table),
            "dropped table {table} must not exist"
        );
    }
    // FTS5 product search table exists.
    assert!(names.iter().any(|n| n == "sku_fts"));
}

#[test]
fn migrations_are_idempotent_on_rerun() {
    let db = Db::open_in_memory().unwrap();
    let before = table_names(&db);
    // Re-run the runner on the same database (through the actor, like the
    // app would on a restart): no error, no schema change.
    db.write(run_migrations).unwrap();
    db.write(run_migrations).unwrap();
    let after = table_names(&db);
    assert_eq!(before, after);
    // Exactly one version row, for migration 0001.
    let versions: Vec<i64> = db
        .read(|conn| {
            let mut stmt =
                conn.prepare("SELECT version FROM schema_migrations ORDER BY version")?;
            let rows = stmt.query_map([], |row| row.get::<_, i64>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    assert_eq!(versions, vec![1]);
}

#[test]
fn required_indexes_exist() {
    let db = Db::open_in_memory().unwrap();
    let indexes: Vec<String> = db
        .read(|conn| {
            let mut stmt = conn.prepare("SELECT name FROM sqlite_master WHERE type = 'index' AND name LIKE 'idx_%' ORDER BY name")?;
            let rows = stmt.query_map([], |row| row.get::<_, String>(0))?;
            let mut out = Vec::new();
            for row in rows {
                out.push(row?);
            }
            Ok(out)
        })
        .unwrap();
    for index in [
        "idx_barcode_tenant_code",
        "idx_sku_tenant_code",
        "idx_sale_branch_completed",
        "idx_sale_line_sale",
        "idx_sale_line_sku_sale",
        "idx_stock_movement_branch_sku_time",
        "idx_outbox_unsent",
        "idx_audit_terminal_at",
        "idx_sale_line_tax_line",
        "idx_draft_line_draft",
    ] {
        assert!(indexes.iter().any(|n| n == index), "index {index} missing");
    }
}
