# 12 — Build Plan

Ship thin slices. Do not build hub, cloud, or multi-tenant features before a real shop is trading on the embedded single-store version. Each phase has exit criteria; do not start the next phase until they pass.

## Phase 0 — Foundations and spikes (gate deferred, ADR-029)
- Repo scaffold: Cargo workspace, pnpm workspace, `justfile`, CI, lint/format configs, toolchain pins, `just check` green on an empty app.
- Tokens + `ui-kit` skeleton (tokens.css, Button, TextField, StatusPill) in white theme + dark theme.
- Spikes (see `06`): receipt print (Latin + RTL, logo, QR) + cut + drawer kick; keyboard-wedge scanner discrimination; scale (if targeted).
- Benchmark app: seed 500k SKUs / 5M sale lines; measure budgets on the weakest PC; fill the table in `08-decisions.md`.
- Two-node sync spike (outbox → server → pull) with duplicate/out-of-order tests.
- **Status (2026-11-10, owner decision):** the Phase 0 benchmark gate is DEFERRED (ADR-029) — no weak-spec PC is available. Development continues (spec 000b, then spec 001); the full Windows benchmark (scenarios 7–8, 500k snapshot, checkout under sync) is a hard checkpoint before the real-shop pilot (Phase 4) at the latest. The benchmark tooling stays. The original exit criteria (budgets met or Electron fallback decided; all hardware spikes pass) still apply at that checkpoint.

## Phase 1 — Single-store POS (embedded, offline)
Domain crate (Money/Qty/Rate, pricing, tax, discounts, promotions v1) with golden tests · DB schema + migrations + DB actor · catalog basics (items, SKUs, barcodes, categories, price list, tax groups) · sell screen (scan, cart, discounts, hold/recall, split payments, returns, void with approval) · receipt printing + reprint queue · users, roles, PIN, lock screen, manager override · shifts, cash in/out, Z-report · audit log · settings + setup wizard + training mode · local backup/restore.
- **Exit:** a cashier can run a full day offline; power-loss and crash tests pass; budgets pass; pilot-ready installer.

## Phase 2 — Inventory and purchasing
Stock ledger + balances + verify job · adjustments, stocktake, low-stock alerts · suppliers, POs, GRN, weighted-average costing, supplier payables · batch/expiry tracking · label printing · bulk import/export (catalog, stock, customers) + migration importer.
- **Exit:** an owner can load their catalog from the old system and run purchasing and stocktake; stock always reconciles.

## Phase 3 — Customers, credit, reporting
Customer records, credit ledger, loyalty basics · aggregates + reports (sales, margin, stock valuation, item/category/cashier, tax) with Excel/PDF export · receipt/invoice designer · multi-language/RTL, multi-currency reporting + foreign-currency tender at the payment layer (ADR-017).
- **Exit:** owner reports match golden totals; exports open correctly in Excel.

## Phase 4 — Pilot in one real shop (do not skip)
Install in a real shop with real cashiers. Collect: scan-to-sale times, friction points, crashes, hardware quirks. Fix before scaling. Record learnings in the tracker.
- **Exit:** two weeks of trading without data loss; top 10 friction issues resolved.

## Phase 5 — Multi-till store hub and cloud
Hub mode + client tills, LAN protocol, store-level consistency · cloud server (tenants, RLS, partitions) · initial snapshot sync + incremental sync · server revalidation · branch transfers · head-office dashboards and remote views.
- **Exit:** 10-till store soak test (8 h) passes; sync chaos tests pass; cloud revalidation mismatch rate = 0 on golden data.

## Phase 6 — Hardening and release
Signed installers, staged updater, licensing, diagnostics bundle, pen-test, accessibility audit, documentation, support runbooks, restore drills.
- **Exit:** release checklist complete; rollback tested.

## Later (v2)
Fiscal/e-invoicing per market (research starts in Phase 1), online order sync, mobile owner dashboard, auto-reorder, advanced promotions, accounting exports.

## Working rhythm
Each phase is broken into specs (`specs/NNN-name.md`). Each spec is one agent session sized (vertical slice). Order by risk: unknowns first, polish last.
