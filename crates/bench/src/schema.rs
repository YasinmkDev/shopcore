//! Throwaway bench schema, mirroring `context/04-data-and-sync.md` tables.
//!
//! This schema is NEVER the app schema. When the real migrations land
//! (spec 001), the bench switches to the real schema and this file is
//! deleted. Until then scenario 4 measures the mirrored schema and the
//! results are labelled as such (spec 002 review, 2026-11-10).

use crate::BenchError;

/// DDL for the throwaway bench database. PRAGMAs match the app config in
/// `04-data-and-sync.md` (WAL, `synchronous=FULL`, foreign keys on).
pub const SCHEMA_SQL: &str = "
PRAGMA journal_mode=WAL;
PRAGMA synchronous=FULL;
PRAGMA foreign_keys=ON;
PRAGMA busy_timeout=5000;
PRAGMA temp_store=MEMORY;

CREATE TABLE IF NOT EXISTS sku (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  code TEXT NOT NULL,
  name TEXT NOT NULL,
  base_uom_id TEXT,
  tax_group_id TEXT,
  track_batch INTEGER NOT NULL DEFAULT 0,
  active INTEGER NOT NULL DEFAULT 1,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_sku_tenant_code ON sku(tenant_id, code);
CREATE INDEX IF NOT EXISTS idx_sku_active ON sku(active);

CREATE TABLE IF NOT EXISTS barcode (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  sku_id TEXT NOT NULL,
  code TEXT NOT NULL,
  is_primary INTEGER NOT NULL DEFAULT 0,
  pack_qty_milli INTEGER NOT NULL DEFAULT 1000,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1,
  UNIQUE(tenant_id, code)
);
CREATE INDEX IF NOT EXISTS idx_barcode_tenant_code ON barcode(tenant_id, code);

CREATE TABLE IF NOT EXISTS customer (
  id TEXT PRIMARY KEY,
  tenant_id TEXT NOT NULL,
  name TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS sale (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL,
  terminal_id TEXT NOT NULL,
  shift_id TEXT NOT NULL,
  receipt_no TEXT NOT NULL UNIQUE,
  status TEXT NOT NULL,
  customer_id TEXT,
  total_minor INTEGER NOT NULL,
  completed_at INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_sale_branch_completed ON sale(branch_id, completed_at);

CREATE TABLE IF NOT EXISTS sale_line (
  id TEXT PRIMARY KEY,
  sale_id TEXT NOT NULL,
  sku_id TEXT NOT NULL,
  qty_milli INTEGER NOT NULL,
  unit_price_e4 INTEGER NOT NULL,
  discount_minor INTEGER NOT NULL DEFAULT 0,
  tax_minor INTEGER NOT NULL DEFAULT 0,
  total_minor INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_sale_line_sale ON sale_line(sale_id);
CREATE INDEX IF NOT EXISTS idx_sale_line_sku_sale ON sale_line(sku_id, sale_id);

CREATE TABLE IF NOT EXISTS sale_payment (
  id TEXT PRIMARY KEY,
  sale_id TEXT NOT NULL,
  method TEXT NOT NULL,
  amount_minor INTEGER NOT NULL,
  reference TEXT,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1
);

CREATE TABLE IF NOT EXISTS stock_movement (
  id TEXT PRIMARY KEY,
  branch_id TEXT NOT NULL,
  sku_id TEXT NOT NULL,
  batch_id TEXT,
  type TEXT NOT NULL,
  qty_milli INTEGER NOT NULL,
  unit_cost_e4 INTEGER NOT NULL,
  ref_type TEXT,
  ref_id TEXT,
  occurred_at INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS idx_movement_branch_sku_time
  ON stock_movement(branch_id, sku_id, occurred_at);

CREATE TABLE IF NOT EXISTS stock_balance (
  branch_id TEXT NOT NULL,
  sku_id TEXT NOT NULL,
  qty_milli INTEGER NOT NULL,
  avg_cost_e4 INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  deleted_at INTEGER,
  version INTEGER NOT NULL DEFAULT 1,
  PRIMARY KEY (branch_id, sku_id)
);

CREATE TABLE IF NOT EXISTS daily_sales_summary (
  day TEXT NOT NULL,
  branch_id TEXT NOT NULL,
  total_minor INTEGER NOT NULL,
  sale_count INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  PRIMARY KEY (day, branch_id)
);

CREATE TABLE IF NOT EXISTS daily_item_sales (
  day TEXT NOT NULL,
  sku_id TEXT NOT NULL,
  qty_milli INTEGER NOT NULL,
  total_minor INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  PRIMARY KEY (day, sku_id)
);

CREATE TABLE IF NOT EXISTS daily_cash_summary (
  day TEXT NOT NULL,
  branch_id TEXT NOT NULL,
  total_minor INTEGER NOT NULL,
  tenant_id TEXT NOT NULL,
  PRIMARY KEY (day, branch_id)
);

CREATE TABLE IF NOT EXISTS audit_log (
  id TEXT PRIMARY KEY,
  user_id TEXT NOT NULL,
  action TEXT NOT NULL,
  entity TEXT NOT NULL,
  entity_id TEXT NOT NULL,
  before_json TEXT,
  after_json TEXT,
  reason TEXT,
  at INTEGER NOT NULL,
  tenant_id TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS outbox (
  id TEXT PRIMARY KEY,
  entity TEXT NOT NULL,
  payload TEXT NOT NULL,
  created_at INTEGER NOT NULL,
  sent_at INTEGER,
  attempts INTEGER NOT NULL DEFAULT 0,
  tenant_id TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_outbox_unsent
  ON outbox(sent_at) WHERE sent_at IS NULL;

CREATE TABLE IF NOT EXISTS sync_state (
  stream TEXT PRIMARY KEY,
  cursor TEXT NOT NULL,
  tenant_id TEXT NOT NULL
);

CREATE VIRTUAL TABLE IF NOT EXISTS sku_fts USING fts5(name, code);

CREATE TABLE IF NOT EXISTS bench_scratch (
  id INTEGER PRIMARY KEY,
  payload TEXT NOT NULL
);
";

/// Create the throwaway schema on `conn`.
///
/// # Errors
/// Returns [`BenchError::Sql`] if any DDL statement fails.
pub fn create_schema(conn: &rusqlite::Connection) -> Result<(), BenchError> {
    conn.execute_batch(SCHEMA_SQL).map_err(BenchError::Sql)
}
