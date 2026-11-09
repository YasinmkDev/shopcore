# 05 — Performance Budgets and Data-Handling Rules

Budgets are requirements, enforced by `just bench` in CI on a seeded dataset (500k SKUs, 5M sale lines, background sync running). A change that regresses a budget does not merge. Numbers marked (initial) are calibrated in Phase 0 on the weakest target PC and then frozen in `08-decisions`.

## Budgets
| Interaction | Budget |
|---|---|
| Scan barcode → line visible | < 50 ms |
| Type-to-search results (100k active SKUs) | < 100 ms first results |
| Add/remove line, qty change, discount | < 50 ms UI feedback |
| Tender → sale committed | < 200 ms |
| Commit → receipt printing starts | < 500 ms (async, never blocks next sale) |
| Cold start → sell screen usable (initial) | < 4 s on weakest PC |
| Switch route (back office) | < 150 ms to first paint |
| Open a 100k-row grid, scroll | 60 fps, no jank (virtualized) |
| Report over 90 days from aggregates | < 1 s |
| Report over 2M raw rows (server) | < 3 s |
| Total RAM, all processes, idle (initial) | < 400 MB |
| Total RAM after 8 h simulated trading (initial) | no growth beyond +10% |
| Installer size (initial) | < 60 MB |
| Sync running in background | checkout latency unaffected (same budgets) |

## Data-handling rules (core)
1. **Never load a table into the UI.** Pages of ≤ 200 rows via keyset pagination. Counts are cached/approximate where exact is unnecessary.
2. **Search:** barcode exact match = single indexed lookup. Name search = in-memory prefix index for hot SKUs (build at startup from the DB, update on change) + FTS5 for the rest. Debounce 0 ms for barcode scan, 60 ms for typing.
3. **Prepared statements** cached on hot paths. No ORM on hot paths; raw SQL with typed row mappers.
4. **Write path is short.** One transaction per sale, no network, no hardware, no logging at debug level in the transaction.
5. **Aggregates, not scans.** Reports read pre-aggregated tables. Any new report must define its aggregate or run server-side.
6. **IPC payloads small.** Send IDs and compact DTOs. No base64 images over IPC; images served from local files via asset protocol, thumbnails pre-generated (WebP, ≤ 128 px) and cached.
7. **Backpressure:** sync, imports, and archival run in small batches (≤ 200 rows / ≤ 50 ms of write-lock time) and yield when the write queue has checkout work.
8. **Memory:** streaming for imports/exports (never read a whole file into memory). Cap in-memory caches by size with LRU.
9. **Startup:** lazy init. Open DB, load settings, render sell screen; build search index, start sync, init hardware afterwards in the background.
10. **No N+1 queries.** Batch fetches; check with query-count assertions in tests for list endpoints.

## UI performance rules (React)
- Virtualize every list/table over ~50 rows (TanStack Virtual).
- Cart state in a Zustand store with **selector-based subscriptions**; each cart row re-renders only when its own data changes. No React Context for hot state.
- Memoize row components; stable keys; avoid inline object/array props in hot components.
- Code-split back-office routes (`React.lazy`); the sell screen bundle must stay minimal (budget: initial JS < 300 KB gzip).
- No heavy libraries on the sell screen (no chart lib, no date lib beyond a tiny one). Charts lazy-loaded; use `uPlot` for time series and hand-rolled SVG for simple bars.
- Images: lazy, fixed dimensions, no layout shift.
- Long tasks > 50 ms on the main thread fail the perf test. Move heavy work to Rust or a worker.
- Animate only `transform` and `opacity` (see `10-ui-system`). Adaptive animation tiers protect weak PCs.

## Measuring
- `crates/bench`: seeds data (`just seed-large`) and runs the budgets above using criterion + scripted scenarios.
- UI: Playwright traces + `PerformanceObserver` long-task checks on the browser build against the hub HTTP transport; real-device run on the weakest PC before each release.
- Every PR touching hot paths includes before/after numbers.
- Release gate: 8-hour soak test (simulated sales + sync + prints) with memory and latency tracking.
