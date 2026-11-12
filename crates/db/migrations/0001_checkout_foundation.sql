-- Migration 0001: checkout foundation (spec 001b-1).
-- Creates ONLY the tables the 001 checkout flow uses. DROPPED (never
-- created): cash_movement, uom_conversion, brand, category.
-- Conventions (04): id UUIDv7 TEXT, tenant_id everywhere, epoch-ms
-- timestamps, money *_minor (i64), prices *_e4, quantities *_milli,
-- rates *_bp. Soft delete only (deleted_at).

CREATE TABLE IF NOT EXISTS branch (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, code TEXT NOT NULL, name TEXT NOT NULL, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS terminal (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL REFERENCES branch(id), code TEXT NOT NULL, name TEXT NOT NULL, UNIQUE(tenant_id, branch_id, code));
CREATE TABLE IF NOT EXISTS item (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, code TEXT NOT NULL, name TEXT NOT NULL, base_uom_id TEXT, tax_group_id TEXT, track_batch INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS sku (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, item_id TEXT NOT NULL REFERENCES item(id), code TEXT NOT NULL, name TEXT NOT NULL, base_uom_id TEXT, tax_group_id TEXT, track_batch INTEGER NOT NULL DEFAULT 0, active INTEGER NOT NULL DEFAULT 1, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS barcode (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, sku_id TEXT NOT NULL REFERENCES sku(id), code TEXT NOT NULL, is_primary INTEGER NOT NULL DEFAULT 0, pack_qty_milli INTEGER NOT NULL DEFAULT 1000, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS uom (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, code TEXT NOT NULL, name TEXT NOT NULL, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS tax_group (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, code TEXT NOT NULL, name TEXT NOT NULL, price_includes_tax INTEGER NOT NULL DEFAULT 0, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS tax_rate (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, tax_group_id TEXT NOT NULL REFERENCES tax_group(id), rate_bp INTEGER NOT NULL, valid_from INTEGER, valid_to INTEGER);
CREATE TABLE IF NOT EXISTS price_list (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, code TEXT NOT NULL, name TEXT NOT NULL, UNIQUE(tenant_id, code));
CREATE TABLE IF NOT EXISTS price_list_entry (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, price_list_id TEXT NOT NULL REFERENCES price_list(id), sku_id TEXT NOT NULL REFERENCES sku(id), price_e4 INTEGER NOT NULL, valid_from INTEGER, valid_to INTEGER, UNIQUE(tenant_id, price_list_id, sku_id, valid_from));
CREATE TABLE IF NOT EXISTS sale (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, terminal_id TEXT NOT NULL, shift_id TEXT, receipt_no TEXT UNIQUE, status TEXT NOT NULL, customer_id TEXT, draft_id TEXT UNIQUE, subtotal_minor INTEGER NOT NULL DEFAULT 0, discount_minor INTEGER NOT NULL DEFAULT 0, tax_minor INTEGER NOT NULL DEFAULT 0, total_minor INTEGER NOT NULL DEFAULT 0, completed_at INTEGER);
CREATE TABLE IF NOT EXISTS sale_line (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, sale_id TEXT NOT NULL REFERENCES sale(id), sku_id TEXT NOT NULL, qty_milli INTEGER NOT NULL, unit_price_e4 INTEGER NOT NULL, discount_minor INTEGER NOT NULL DEFAULT 0, tax_minor INTEGER NOT NULL DEFAULT 0, total_minor INTEGER NOT NULL, promo_json TEXT);
CREATE TABLE IF NOT EXISTS sale_payment (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, sale_id TEXT NOT NULL REFERENCES sale(id), method TEXT NOT NULL, amount_minor INTEGER NOT NULL, reference TEXT);
CREATE TABLE IF NOT EXISTS sale_line_tax (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, sale_line_id TEXT NOT NULL REFERENCES sale_line(id), tax_rate_id TEXT NOT NULL REFERENCES tax_rate(id), tax_minor INTEGER NOT NULL, rate_bp INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS stock_movement (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, sku_id TEXT NOT NULL, batch_id TEXT, type TEXT NOT NULL, qty_milli INTEGER NOT NULL, unit_cost_e4 INTEGER NOT NULL DEFAULT 0, ref_type TEXT, ref_id TEXT, occurred_at INTEGER NOT NULL);
CREATE TABLE IF NOT EXISTS stock_balance (id TEXT NOT NULL, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, sku_id TEXT NOT NULL, qty_milli INTEGER NOT NULL DEFAULT 0, avg_cost_e4 INTEGER NOT NULL DEFAULT 0, PRIMARY KEY (tenant_id, branch_id, sku_id));
CREATE TABLE IF NOT EXISTS shift (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, terminal_id TEXT NOT NULL, opened_by_user_id TEXT, closed_by_user_id TEXT, status TEXT NOT NULL, opened_at INTEGER NOT NULL, closed_at INTEGER);
CREATE TABLE IF NOT EXISTS user (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, username TEXT NOT NULL, pin_hash TEXT NOT NULL, role_id TEXT REFERENCES role(id), failed_attempts INTEGER NOT NULL DEFAULT 0, locked_until INTEGER, UNIQUE(tenant_id, username));
CREATE TABLE IF NOT EXISTS role (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, name TEXT NOT NULL, permissions_json TEXT NOT NULL DEFAULT '[]', max_line_discount_bps INTEGER NOT NULL DEFAULT 0, UNIQUE(tenant_id, name));
CREATE TABLE IF NOT EXISTS settings (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, scope TEXT NOT NULL, scope_id TEXT NOT NULL DEFAULT '', key TEXT NOT NULL, value TEXT NOT NULL, UNIQUE(tenant_id, scope, scope_id, key));
CREATE TABLE IF NOT EXISTS receipt_counter (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, terminal_id TEXT NOT NULL, date TEXT NOT NULL, last_seq INTEGER NOT NULL DEFAULT 0, UNIQUE(tenant_id, terminal_id, date));
CREATE TABLE IF NOT EXISTS print_job (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, terminal_id TEXT, sale_id TEXT, payload_json TEXT NOT NULL DEFAULT '{}', status TEXT NOT NULL DEFAULT 'queued', attempts INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS audit_log (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, terminal_id TEXT, user_id TEXT, action TEXT NOT NULL, entity TEXT, entity_id TEXT, before_json TEXT, after_json TEXT, reason TEXT, at INTEGER NOT NULL, prev_hash TEXT, row_hash TEXT);
CREATE TABLE IF NOT EXISTS outbox (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, branch_id TEXT NOT NULL, entity TEXT NOT NULL, stream TEXT NOT NULL, payload TEXT NOT NULL, idempotency_key TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, sent_at INTEGER, attempts INTEGER NOT NULL DEFAULT 0);
CREATE TABLE IF NOT EXISTS daily_sales_summary (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, date TEXT NOT NULL, sale_count INTEGER NOT NULL DEFAULT 0, subtotal_minor INTEGER NOT NULL DEFAULT 0, discount_minor INTEGER NOT NULL DEFAULT 0, tax_minor INTEGER NOT NULL DEFAULT 0, total_minor INTEGER NOT NULL DEFAULT 0, UNIQUE(tenant_id, branch_id, date));
CREATE TABLE IF NOT EXISTS daily_item_sales (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, sku_id TEXT NOT NULL, date TEXT NOT NULL, qty_milli INTEGER NOT NULL DEFAULT 0, total_minor INTEGER NOT NULL DEFAULT 0, UNIQUE(tenant_id, branch_id, sku_id, date));
CREATE TABLE IF NOT EXISTS daily_cash_summary (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, terminal_id TEXT, date TEXT NOT NULL, opening_minor INTEGER NOT NULL DEFAULT 0, cash_sales_minor INTEGER NOT NULL DEFAULT 0, cash_in_minor INTEGER NOT NULL DEFAULT 0, cash_out_minor INTEGER NOT NULL DEFAULT 0, expected_minor INTEGER NOT NULL DEFAULT 0, counted_minor INTEGER, UNIQUE(tenant_id, branch_id, date));
CREATE TABLE IF NOT EXISTS draft_sale (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, branch_id TEXT NOT NULL, terminal_id TEXT NOT NULL, shift_id TEXT, user_id TEXT, status TEXT NOT NULL DEFAULT 'open');
CREATE TABLE IF NOT EXISTS draft_line (id TEXT PRIMARY KEY, tenant_id TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, deleted_at INTEGER, version INTEGER NOT NULL DEFAULT 1, draft_sale_id TEXT NOT NULL REFERENCES draft_sale(id), sku_id TEXT NOT NULL, qty_milli INTEGER NOT NULL, unit_price_e4 INTEGER NOT NULL, discount_minor INTEGER NOT NULL DEFAULT 0);

-- Indexes (04 + spec 001b-1 req. 1).
CREATE INDEX IF NOT EXISTS idx_barcode_tenant_code ON barcode(tenant_id, code);
CREATE INDEX IF NOT EXISTS idx_sku_tenant_code ON sku(tenant_id, code);
CREATE INDEX IF NOT EXISTS idx_sale_branch_completed ON sale(branch_id, completed_at);
CREATE INDEX IF NOT EXISTS idx_sale_line_sale ON sale_line(sale_id);
CREATE INDEX IF NOT EXISTS idx_sale_line_sku_sale ON sale_line(sku_id, sale_id);
CREATE INDEX IF NOT EXISTS idx_stock_movement_branch_sku_time ON stock_movement(branch_id, sku_id, occurred_at);
CREATE INDEX IF NOT EXISTS idx_outbox_unsent ON outbox(sent_at) WHERE sent_at IS NULL;
CREATE INDEX IF NOT EXISTS idx_user_tenant_username ON user(tenant_id, username);
CREATE INDEX IF NOT EXISTS idx_audit_terminal_at ON audit_log(terminal_id, at);
CREATE INDEX IF NOT EXISTS idx_sale_line_tax_line ON sale_line_tax(sale_line_id);
CREATE INDEX IF NOT EXISTS idx_draft_line_draft ON draft_line(draft_sale_id);
CREATE INDEX IF NOT EXISTS idx_receipt_counter_terminal_date ON receipt_counter(terminal_id, date);

-- FTS5 product search over name/code/barcodes only (brand dropped).
-- sku_fts is a standalone FTS5 table (not external-content) so the
-- triggers below can maintain it without a content-table coupling.
CREATE VIRTUAL TABLE IF NOT EXISTS sku_fts USING fts5(sku_id UNINDEXED, name, code, barcodes, tokenize='unicode61');

-- FTS5 maintenance triggers (sku + barcode writes).
CREATE TRIGGER IF NOT EXISTS trg_fts_sku_insert AFTER INSERT ON sku BEGIN
  INSERT INTO sku_fts(sku_id, name, code, barcodes) VALUES (NEW.id, NEW.name, NEW.code, '');
END;
CREATE TRIGGER IF NOT EXISTS trg_fts_sku_update AFTER UPDATE ON sku BEGIN
  UPDATE sku_fts SET name = NEW.name, code = NEW.code WHERE sku_id = OLD.id;
END;
CREATE TRIGGER IF NOT EXISTS trg_fts_sku_delete AFTER DELETE ON sku BEGIN
  DELETE FROM sku_fts WHERE sku_id = OLD.id;
END;
CREATE TRIGGER IF NOT EXISTS trg_fts_barcode_insert AFTER INSERT ON barcode BEGIN
  UPDATE sku_fts SET barcodes = (SELECT COALESCE(group_concat(code, ' '), '') FROM barcode WHERE sku_id = NEW.sku_id AND deleted_at IS NULL) WHERE sku_id = NEW.sku_id;
END;
CREATE TRIGGER IF NOT EXISTS trg_fts_barcode_update AFTER UPDATE ON barcode BEGIN
  UPDATE sku_fts SET barcodes = (SELECT COALESCE(group_concat(code, ' '), '') FROM barcode WHERE sku_id = OLD.sku_id AND deleted_at IS NULL) WHERE sku_id = OLD.sku_id;
  UPDATE sku_fts SET barcodes = (SELECT COALESCE(group_concat(code, ' '), '') FROM barcode WHERE sku_id = NEW.sku_id AND deleted_at IS NULL) WHERE sku_id = NEW.sku_id;
END;
CREATE TRIGGER IF NOT EXISTS trg_fts_barcode_delete AFTER DELETE ON barcode BEGIN
  UPDATE sku_fts SET barcodes = (SELECT COALESCE(group_concat(code, ' '), '') FROM barcode WHERE sku_id = OLD.sku_id AND deleted_at IS NULL) WHERE sku_id = OLD.sku_id;
END;

-- Append-only enforcement (spec 001b-1 req. 13): UPDATE/DELETE are
-- refused on stock_movement, audit_log, and on COMPLETED sale rows.
-- Draft sale rows stay editable until completion (001b-2).
CREATE TRIGGER IF NOT EXISTS trg_stock_movement_no_update BEFORE UPDATE ON stock_movement BEGIN
  SELECT RAISE(ABORT, 'stock_movement is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_stock_movement_no_delete BEFORE DELETE ON stock_movement BEGIN
  SELECT RAISE(ABORT, 'stock_movement is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_audit_log_no_update BEFORE UPDATE ON audit_log BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_audit_log_no_delete BEFORE DELETE ON audit_log BEGIN
  SELECT RAISE(ABORT, 'audit_log is append-only');
END;
CREATE TRIGGER IF NOT EXISTS trg_sale_no_update_completed BEFORE UPDATE ON sale WHEN OLD.status = 'completed' BEGIN
  SELECT RAISE(ABORT, 'completed sale is immutable');
END;
CREATE TRIGGER IF NOT EXISTS trg_sale_no_delete_completed BEFORE DELETE ON sale WHEN OLD.status = 'completed' BEGIN
  SELECT RAISE(ABORT, 'completed sale is immutable');
END;
CREATE TRIGGER IF NOT EXISTS trg_sale_line_no_update_completed BEFORE UPDATE ON sale_line WHEN (SELECT status FROM sale WHERE id = OLD.sale_id) = 'completed' BEGIN
  SELECT RAISE(ABORT, 'completed sale_line is immutable');
END;
CREATE TRIGGER IF NOT EXISTS trg_sale_line_no_delete_completed BEFORE DELETE ON sale_line WHEN (SELECT status FROM sale WHERE id = OLD.sale_id) = 'completed' BEGIN
  SELECT RAISE(ABORT, 'completed sale_line is immutable');
END;
CREATE TRIGGER IF NOT EXISTS trg_sale_payment_no_update_completed BEFORE UPDATE ON sale_payment WHEN (SELECT status FROM sale WHERE id = OLD.sale_id) = 'completed' BEGIN
  SELECT RAISE(ABORT, 'completed sale_payment is immutable');
END;
CREATE TRIGGER IF NOT EXISTS trg_sale_payment_no_delete_completed BEFORE DELETE ON sale_payment WHEN (SELECT status FROM sale WHERE id = OLD.sale_id) = 'completed' BEGIN
  SELECT RAISE(ABORT, 'completed sale_payment is immutable');
END;

-- Catalog version counter (spec 001b-1 req. 4, amendment 9): every
-- catalog write bumps settings[tenant/catalog.version], so the
-- in-memory prefix index can never serve a stale catalog. The
-- settings row is system-generated by the trigger itself; its id
-- is a random 128-bit hex string (the one exception to the UUIDv7
-- convention — no client generates this row, and nothing references
-- its id).
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_item_INSERT AFTER INSERT ON item BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_item_UPDATE AFTER UPDATE ON item BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_item_DELETE AFTER DELETE ON item BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_sku_INSERT AFTER INSERT ON sku BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_sku_UPDATE AFTER UPDATE ON sku BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_sku_DELETE AFTER DELETE ON sku BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_barcode_INSERT AFTER INSERT ON barcode BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_barcode_UPDATE AFTER UPDATE ON barcode BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_barcode_DELETE AFTER DELETE ON barcode BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_price_list_INSERT AFTER INSERT ON price_list BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_price_list_UPDATE AFTER UPDATE ON price_list BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_price_list_DELETE AFTER DELETE ON price_list BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_price_list_entry_INSERT AFTER INSERT ON price_list_entry BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_price_list_entry_UPDATE AFTER UPDATE ON price_list_entry BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_price_list_entry_DELETE AFTER DELETE ON price_list_entry BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_tax_group_INSERT AFTER INSERT ON tax_group BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_tax_group_UPDATE AFTER UPDATE ON tax_group BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_tax_group_DELETE AFTER DELETE ON tax_group BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_tax_rate_INSERT AFTER INSERT ON tax_rate BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_tax_rate_UPDATE AFTER UPDATE ON tax_rate BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_tax_rate_DELETE AFTER DELETE ON tax_rate BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_uom_INSERT AFTER INSERT ON uom BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_uom_UPDATE AFTER UPDATE ON uom BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), NEW.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
CREATE TRIGGER IF NOT EXISTS trg_catalog_version_uom_DELETE AFTER DELETE ON uom BEGIN
  INSERT INTO settings (id, tenant_id, scope, scope_id, key, value, created_at, updated_at, version)
  VALUES (lower(hex(randomblob(16))), OLD.tenant_id, 'tenant', '', 'catalog.version', '1', CAST(strftime('%s', 'now') AS INTEGER) * 1000, CAST(strftime('%s', 'now') AS INTEGER) * 1000, 1)
  ON CONFLICT(tenant_id, scope, scope_id, key) DO UPDATE SET value = CAST(CAST(settings.value AS INTEGER) + 1 AS TEXT), updated_at = CAST(strftime('%s', 'now') AS INTEGER) * 1000, version = settings.version + 1;
END;
