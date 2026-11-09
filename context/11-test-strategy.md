# 11 — Test Strategy

Tests are the main control over agent-written code. They encode the business rules in `02-domain-model.md`. Never weaken, skip, or delete a test to get green.

## Pyramid
1. **Domain unit + property tests (largest).** `proptest` for invariants: line/bill totals consistent, discount allocation sums exactly, tax inclusive/exclusive round-trips, balance == sum(movements), idempotent replay. Golden datasets in `tests/golden/*.json` (input cart → expected lines, tax, total) include every worked example in `02`.
2. **Core integration tests.** Real SQLite (in-memory/tempfile) with migrations applied, fake clock/ids/hardware. Cover each use case end-to-end including the single-transaction sale path, rollback on failure, permissions, audit rows, aggregate updates.
3. **Sync tests.** Two or more simulated nodes (till/hub/cloud) with a controllable network: duplicate delivery, out-of-order, drop mid-batch, clock skew, concurrent last-unit sale, upgrade with unsent outbox, snapshot initial sync, resume. Assert convergence and server revalidation equality.
4. **Crash-safety tests.** Kill the process at random points during sale commit, migration, and backup; on restart the DB is consistent (no partial sale, balances match ledger).
5. **Hardware contract tests.** Mock drivers simulate slow/offline/out-of-paper/duplicate scans; the sale must still complete and the action must queue.
6. **UI component tests.** Vitest + Testing Library for components in all states, keyboard behavior, RTL snapshot checks, axe accessibility checks.
7. **E2E flows.** Playwright against the **browser build of the UI connected to the hub HTTP transport** (fast, deterministic); a smaller smoke suite on the real Tauri app via `tauri-driver`/WebDriver in CI on Windows.
8. **Performance tests.** `just bench` against budgets in `05` (blocking in CI). Soak test (8 h simulated trading) before each release.
9. **Migration tests.** Take DB from previous release, apply new migrations, run verify job.
10. **Security tests.** Permission matrix tests (every permission denied/allowed per role), audit hash-chain verification, IPC input fuzzing for commands.

## Acceptance scenarios (seed list; expand per spec, Given/When/Then)
- Scan known barcode adds line in one keystroke; scanning the same item increments quantity.
- Scan unknown barcode offers "Add item" without losing the cart.
- Weighted item reads scale and prices by weight; embedded-weight barcode parses correctly.
- Cash sale with change; split cash + card; overpay on card rejected.
- Line discount above role threshold requires manager PIN; approval is audited with both users.
- Bill discount allocates exactly across lines; totals equal the golden dataset.
- Hold sale, switch cashier, recall sale: cart restored exactly.
- Internet cut mid-shift: selling continues; reconnect syncs with zero duplicates.
- Power loss during payment commit: after restart there is either a complete sale or no sale, never partial.
- Return with receipt restores stock and reverses tax at original rates; return without receipt requires permission.
- Shift close: expected vs counted cash, variance flagged, Z-report immutable and reprint logged.
- Receive goods (GRN) updates weighted average cost correctly (10 @ 5.00 + 10 @ 7.00 → 6.00).
- Stocktake adjustments post movements; sales during the count are not blocked.
- Hub unreachable: client till keeps selling and later merges into the hub.
- Import 50k products with 200 bad rows: valid rows imported, error report downloadable, nothing corrupted.
- Backup → wipe → restore yields identical data (hash comparison).

## Test data
Deterministic generators (`seed-large`: 500k SKUs, 5M sale lines, 2M movements) with fixed seeds. Fixtures contain no real customer data.

## Rules for agents
- Test first or alongside; each spec lists the tests it adds.
- Do not mock the unit under test; mock only hardware, clock, network.
- Flaky tests are bugs: fix or quarantine with a tracked issue; never retry-until-green.
- Coverage is a signal, not a goal: domain and core business logic should stay near-complete; UI judged by scenario coverage.
