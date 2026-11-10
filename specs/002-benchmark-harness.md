# Spec 002 — Benchmark harness and Phase 0 performance gate

Status: Draft (needs human approval)   Phase: 0   Depends on: 000
Decides: ADR-002 (Tauri vs Electron fallback), initial budget values in `context/05-performance-budgets.md`.

## Goal
Measure, on the weakest target PC, whether the stack meets the budgets with a realistic dataset, and produce numbers to freeze into `context/08-decisions.md`.

## In scope
- `crates/bench`: deterministic seeder + scenario runner (release builds only).
- A debug-only route in the desktop app with a 100k-row virtualized grid and a synthetic cart, for UI measurements inside the real WebView2.
- Results written as JSON + a markdown table ready to paste into `08-decisions.md`.

## Out of scope
Real schema/migrations (the bench uses a throwaway schema that mirrors `04-data-and-sync.md` tables: sku, barcode, sale, sale_line, stock_movement, stock_balance, daily aggregates, FTS5). No business logic. No sync (spec 005 reuses this harness).

## Dataset (fixed seeds, reproducible)
500k SKUs (100k flagged active), 600k barcodes, 100k customers, 5M sale lines across ~1.2M sales, 2M stock movements, aggregates pre-built. `just seed-large` creates it; record DB file size.

## Scenarios and metrics (p50 / p95 / p99, median of 5 runs)
1. Barcode exact lookup (cold cache and warm).
2. Name search: FTS5 prefix, and in-memory hot index (100k active SKUs); index build time at startup and its RAM.
3. Keyset page of 200 rows (item list, sales list) at depth 0 / 100k / 1M.
4. Sale commit transaction (6 lines, full write path from `04`), `synchronous=FULL`, on the actual disk type.
5. Sale commit while a background writer inserts batches of 200 rows (simulated sync) and while a read-heavy report runs.
6. Report: 90-day sales from aggregates; same range from raw `sale_line` (to show why aggregates exist).
7. Startup: process start → DB open → sell screen interactive (cold boot of the app, and after Windows reboot).
8. UI (in WebView2): scroll 100k-row grid for 10 s (frame p95, long tasks > 50 ms), add 50 cart rows rapidly (re-render count per row), route change time.
9. Memory: total RAM across all ShopCore + WebView2 processes at idle, after 30 min soak (full 8 h soak is a later release gate). Record CPU idle %.
10. Installer sizes (exe, NSIS, MSI).

## Pass criteria
Per `05-performance-budgets.md`. Any miss is reported with the cause and a proposed fix. Final recommendation: keep Tauri, or trigger the Electron-sidecar fallback (ADR-002).

## Machine record (required)
CPU, RAM, disk type (SSD/HDD), Windows build, WebView2 version, power plan. Run on (a) your dev machine and (b) the weakest PC available to you. Only (b) counts for the gate.

## Dependencies to propose (for approval)
`rusqlite` (bundled, FTS5), `hdrhistogram`, `rand` (seeded), `serde_json`. No others.

## Tests
Seeder determinism test (same seed → same checksums); scenario runner unit tests with a tiny dataset; CI runs the harness on a 10k-row smoke dataset only (full run is manual on the target PC).

## Acceptance
- `just seed-large && just bench` produces results JSON + markdown table.
- Results table filled in `08-decisions.md` with machine record and date.
- A written recommendation (keep Tauri / fallback) with the evidence.

## Risks
Dev-machine numbers will flatter the stack; do not accept them for the gate. HDD results may fail `synchronous=FULL` budgets, so report and propose options (do not weaken durability silently).
