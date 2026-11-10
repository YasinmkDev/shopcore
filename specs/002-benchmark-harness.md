# Spec 002 — Benchmark harness and Phase 0 performance gate

Status: Approved 2026-11-10 (with amendments) — implemented; PROVISIONAL full run done, gate still open (target-PC run + scenarios 7–8 pending)   Phase: 0   Depends on: 000
Decides: ADR-002 (Tauri vs Electron fallback), initial budget values in `context/05-performance-budgets.md`.

## Goal
Measure, on the weakest target PC, whether the stack meets the budgets with a realistic dataset, and produce numbers to freeze into `context/08-decisions.md`.

## In scope
- `crates/bench`: deterministic seeder + scenario runner (release builds only).
- A debug-only route in the desktop app with a 100k-row virtualized grid and a synthetic cart, for UI measurements inside the real WebView2.
- Results written as JSON + a markdown table ready to paste into `08-decisions.md`.

## Out of scope
Real schema/migrations (the bench uses a throwaway schema that mirrors `04-data-and-sync.md` tables: sku, barcode, sale, sale_line, stock_movement, stock_balance, daily aggregates, FTS5). When the real migrations land (spec 001), the bench switches to the real schema and the throwaway is deleted; until then scenario 4 measures the mirrored schema and is labelled as such in the results. No business logic. No sync (spec 005 reuses this harness).

## Data changes
Throwaway bench schema only (in a separate bench DB file, never the app DB): sku, barcode, sale, sale_line, stock_movement, stock_balance, daily aggregates, FTS5. No changes to the app schema. Results JSON + markdown table are the outputs.

## Security / audit
Seeded data is synthetic; no customer data, no secrets. Nothing leaves the machine except the results table pasted into `08-decisions.md`.

## Definition of done
Standard checklist in `AGENTS.md` + all acceptance criteria below met; the `08-decisions.md` Phase 0 results table is filled and the ADR-002 recommendation is written.

## Dataset (fixed seeds, reproducible)
500k SKUs (100k flagged active), 600k barcodes, 100k customers, 5M sale lines across ~1.2M sales, 2M stock movements, aggregates pre-built. `just seed-large` creates it; record DB file size.

## Scenarios and metrics (p50 / p95 / p99, median of 5 runs)
1. Barcode exact lookup (cold cache and warm).
2. Name search: FTS5 prefix, and in-memory hot index (100k active SKUs); index build time at startup and its RAM.
3. Keyset page of 200 rows (item list, sales list) at depth 0 / 100k / 1M.
4. Sale commit transaction (6 lines, full write path from `04`), `synchronous=FULL`, on the actual disk type.
5. Sale commit while a background writer inserts batches of 200 rows (simulated sync) and while a read-heavy report runs.
6. Report: 90-day sales from aggregates; same range from raw `sale_line` (to show why aggregates exist).
7. Startup: process start → DB open → main window interactive (cold boot of the app, and after Windows reboot). The sell screen does not exist yet (spec 001); re-measure against it when it lands.
8. UI (in WebView2): scroll 100k-row grid for 10 s (frame p95, long tasks > 50 ms), add 50 cart rows rapidly (re-render count per row), route change time.
9. Memory: total RAM across all ShopCore + WebView2 processes at idle, after 30 min soak (full 8 h soak is a later release gate). Record CPU idle %.
10. Installer sizes (exe, NSIS, MSI) — each reported against its own ADR-016 budget (online bootstrapper < 60 MB excl. WebView2; offline installer ~150 MB, WebView2 bundled).

## Pass criteria
Per `05-performance-budgets.md`. Any miss is reported with the cause and a proposed fix. Final recommendation: keep Tauri, or trigger the Electron-sidecar fallback (ADR-002).

## Machine record (required)
CPU, RAM, disk type (SSD/HDD), Windows build, WebView2 version, power plan. Run on (a) your dev machine and (b) the weakest PC available to you. Only (b) counts for the gate. Dev-machine numbers are labelled PROVISIONAL in the results; the target-machine record is supplied separately and the gate is re-run on it — the weakest-PC machine does not block building the harness.

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

## Implementation notes (2026-11-10)
Implemented: `crates/bench` (lib + `shopcore-bench` binary), deterministic seeder, 10 scenarios with hdrhistogram percentiles (median of 5 runs), results JSON + markdown, baseline tripwire (5× tolerance, relative — not an absolute budget check), `just bench` / `bench-smoke` / `bench-update-baseline` / `seed-large` / `seed-smoke`, CI `bench-smoke` job, debug route (Ctrl+Shift+B, 100k-row virtualized grid + synthetic cart, `bench_startup_ms` / `bench_ui_results` commands). FTS5 gate test passed (FTS5 supported in the SQLCipher-bundled build). Deps pinned with proof: rusqlite 0.41.1, rand 0.10.3, hdrhistogram 7.6.0. `run_id` (epoch ms) makes bench rows unique per run (fixes a UNIQUE constraint failure on re-runs). Warmup runs one of each operation before measuring (cold-cache noise). Scenario 9 is infallible by design (no Result). Full seed (~60 min, 2.3 GB DB) was killed by a VM restart during the final balance aggregation — recovered by re-running the exact `seed.rs` aggregation SQL; the `(branch_id, sku_id)` index made the planner do 5M random lookups, fixed with `NOT INDEXED` + 256 MB cache (279.9 s). Dataset deviations: 5,397,732 sale lines (spec 5M) and 5,397,732 stock movements (spec 2M — one per line, the spec's own numbers imply 5M+). Full PROVISIONAL run 2026-11-10T09:26:38Z (AMD EPYC 9D64 / 7,934 MiB / Linux): barcode warm p95 0.015 ms, FTS5 p95 14.495 ms, sale-commit p95 36.607 ms, under load 33.759 ms, report aggregates 0.098 ms, raw-report scan 122,014 ms (informational), keyset p95 ≤ 0.212 ms, installer largest 8.479 MB. Results table + ADR-002 recommendation (keep Tauri) in `08-decisions.md`. Still open: target-PC run, scenarios 7–8 on Windows/WebView2.
