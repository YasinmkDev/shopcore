# 04 — Data, Storage, and Sync

## SQLite configuration (local core)
- `journal_mode=WAL`, `synchronous=FULL` on the write connection (power loss is common in shops), `foreign_keys=ON`, `busy_timeout=5000`, `temp_store=MEMORY`, page cache sized by hardware tier, `mmap_size` moderate.
- SQLCipher encryption; key sealed with Windows DPAPI (see `07-security-compliance`).
- One write connection (DB actor). Read-only pool for queries. Short transactions only; background jobs use small batches so they never hold the write lock long.
- Integrity: `PRAGMA quick_check` on startup (full check in diagnostics), automatic backup before every migration.

## Conventions
- Table/column names `snake_case`. Every table: `id` (UUIDv7, TEXT or BLOB), `tenant_id`, `created_at`, `updated_at`, `deleted_at` (nullable), `version` (int, optimistic concurrency), plus `branch_id` where applicable.
- Money columns end `_minor` (i64). Unit price/cost columns end `_e4`. Quantities end `_milli`. Rates end `_bp`.
- No floats anywhere. Timestamps stored as UTC ISO-8601 or epoch ms (pick epoch ms; UI formats locally).
- Migrations: forward-only SQL files, numbered, idempotent, tested against a database from the previous release (customers upgrade in place). Never edit a shipped migration.
- IDs are client-generated, so offline creation and idempotent sync are free.

## Key tables (columns abbreviated)
- `item`, `sku` (item_id, code, name, base_uom_id, tax_group_id, track_batch, active), `barcode` (sku_id, code UNIQUE per tenant, is_primary, pack_qty_milli).
- `price_list`, `price_list_entry` (sku_id, price_e4, valid_from/to).
- `sale` (branch_id, terminal_id, shift_id, receipt_no UNIQUE, status, customer_id, totals *_minor, completed_at), `sale_line` (sku_id, qty_milli, unit_price_e4, discount_minor, tax_minor, total_minor, promo_json), `sale_payment` (method, amount_minor, reference).
- `stock_movement` (branch_id, sku_id, batch_id, type, qty_milli, unit_cost_e4, ref_type, ref_id, occurred_at) — append-only.
- `stock_balance` (branch_id, sku_id, qty_milli, avg_cost_e4) PK (branch_id, sku_id).
- `shift`, `cash_movement`, `customer`, `customer_ledger`, `supplier`, `purchase_order`, `goods_receipt`, `stocktake`, `transfer`.
- Aggregates: `daily_sales_summary`, `daily_item_sales`, `daily_cash_summary`.
- `audit_log` (user_id, action, entity, entity_id, before_json, after_json, reason, at) — append-only.
- `outbox` (id, entity, payload, created_at, sent_at, attempts), `sync_state` (cursor per stream).

## Indexes (non-negotiable ones)
- `barcode(tenant_id, code)` unique; `sku(tenant_id, code)`; `sale(branch_id, completed_at)`; `sale_line(sale_id)`; `sale_line(sku_id, sale_id)` for item history; `stock_movement(branch_id, sku_id, occurred_at)`; `outbox(sent_at) WHERE sent_at IS NULL` partial index.
- Product search: FTS5 over `name`, `code`, barcodes, brand with prefix index; plus an in-memory index of hot SKUs (see `05-performance-budgets`).
- Every list query must use keyset pagination on an indexed column. `EXPLAIN QUERY PLAN` check in CI for hot queries (no table scans).

## Aggregates and balances
Updated in the same transaction as the source write (sales, movements). A background `verify` job reconciles balances and aggregates against the ledger and reports drift (never silently "fixes" without an audit entry).

## Sync design
**Principles:** local DB is the till's source of truth; sync is background; idempotent; resumable; scoped; never in the checkout path.

**Outbox/inbox.** Every local mutation writes an outbox row in the same transaction. The sync worker pushes batches (≤ 200 rows or ~256 KB) with an idempotency key per row; the server acknowledges per row. Pull uses per-stream cursors (monotonic server sequence), applying changes in batches inside short transactions.

**Streams (scoped to the branch):** catalog (items, skus, barcodes, prices, tax, promotions), customers, users/roles, settings (pull); sales, movements, cash, audit, customer ledger (push); stock balances (server-authoritative for other branches, locally maintained for own branch).

**Conflict rules (per entity type)**
| Entity | Rule |
|---|---|
| Sales, returns, movements, ledgers, audit | Append-only, never conflict; deduped by id |
| Stock balance | Derived from movements; never synced as a value for own branch |
| Prices, catalog fields | Per-field last-write-wins by server time; history kept in audit |
| Settings | Server wins; local override flagged |
| Deletes | Soft delete with tombstones; deletes win over edits unless restored explicitly |

**Initial sync:** download a compressed SQLite snapshot (catalog + customers + settings + current balances for the branch) over HTTPS with resume, verify checksum, then continue with cursor-based incremental. Never page 100k rows through JSON.

**Server revalidation:** the server recomputes each synced sale with the same `domain` crate. Mismatch → flagged for review, never silently accepted or altered.

**Failure modes to test (see `11-test-strategy`):** duplicate delivery, out-of-order delivery, connection drop mid-batch, clock skew on tills (server time is authoritative for ordering; tills record both local and server-corrected time), two tills selling the last unit simultaneously (both allowed, flagged), upgrade with unsent outbox rows.

## Hub mode data flow
Client tills talk to the hub over LAN (HTTP/WS with a shared secret + per-terminal token). Hub applies sales in a single writer, so store-level stock is consistent. Tills hold a read cache (catalog, prices, balances snapshot) and a local outbox if the hub is unreachable.

## Retention and archival
- Till/hub keep a hot window: all catalog/customers/balances + last 90 days of sales and movements (configurable).
- After server acknowledgment and window expiry, old rows are archived (compressed export) and pruned, in small batches off-peak (`VACUUM` only in maintenance windows; use `incremental_vacuum`).
- Returns against old receipts query the server when online; offline shows a clear message with a manager override path.

## Cloud (PostgreSQL)
- `tenant_id` on every table + row-level security policies; no cross-tenant query possible from application role.
- `sale`, `sale_line`, `stock_movement`, `audit_log` partitioned by month; archive old partitions to cold storage.
- Read replica for reporting when needed; heavy analytics run here, never on tills.
- Background jobs (imports, report generation, sync processing) via a durable queue; idempotent handlers.
- Backups: PITR enabled; restore drill documented and tested.

## Backups (local)
Scheduled encrypted backup (default nightly + on shift close) to a configurable folder/drive and to cloud when enabled. Restore wizard verifies integrity and shows what will be replaced. Backups are tested in CI (backup → restore → compare).

## Import/export
Importer: Excel/CSV templates for products, customers, opening stock, supplier balances. Streaming parser, validation report with row-level errors, dry-run mode, resumable, runs in a background job with progress. Export: Excel/CSV/PDF for all reports and lists.
