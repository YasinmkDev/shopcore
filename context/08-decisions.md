# 08 — Decision Log (ADRs)

Format: ID, status, decision, why, rejected alternatives, consequences. Agents add a new ADR for any change to stack, invariant, schema direction, or budget. Do not silently contradict an accepted ADR.

## ADR-001 — Headless core + thin UI. Accepted
Business logic, data, sync, hardware in Rust; React renders. UI talks only through `CoreClient`. Why: testability, performance, hub mode, replaceable shell (1Password-style pattern). Rejected: logic in the renderer.

## ADR-002 — Tauri 2 shell (fallback: Electron). Accepted
Why: lightest footprint with full web-UI/animation flexibility. Rejected: Electron as default (heavy), Flutter (desktop text input/RTL/accessibility/hardware plugin weakness), .NET (slower UI iteration, no shared code). Consequence: we own the hardware layer in Rust. **Fallback trigger:** Phase 0 hardware spike or benchmark failure on weakest PC → Electron shell with the Rust core as a sidecar (possible via the `CoreClient` boundary).

## ADR-003 — Rust for core and server, shared `domain` crate. Accepted
Why: identical pricing/tax/stock logic on till and server (server revalidates sales), small native hub binary, compile-time guardrails for agent-written code. Escape hatch: `domain` can compile to WASM/native Node module if a TypeScript server is ever preferred. Rejected: TS everywhere (duplicate logic or heavy runtime).

## ADR-004 — SQLite (WAL, SQLCipher) local; PostgreSQL cloud. Accepted
Single-writer actor with read pool. `synchronous=FULL` for durability. Rejected: embedded Postgres (size), IndexedDB (not enough query power).

## ADR-005 — Money as integer minor units; fixed-point quantities; basis-point rates. Accepted
No floating point in business logic, ever. Global half-away-from-zero rounding once per line.

## ADR-006 — Inventory as append-only ledger + maintained balance. Accepted
Why: auditability, sync-safe merging, no lost updates. Balances updated in the same transaction; verify job reconciles.

## ADR-007 — Offline-first with embedded / hub / client modes. Accepted
Checkout never depends on network. Mega marts use a store hub; tills degrade to local queue if hub unreachable.

## ADR-008 — UUIDv7 client-generated IDs; soft deletes; tenant_id everywhere. Accepted
Why: offline creation, idempotent sync, index locality, multi-tenant from day one.

## ADR-009 — Custom outbox/inbox sync (not generic CRDT). Accepted
Why: retail data is mostly append-only; a few last-write-wins fields; stock is ledger-merged. Simpler, auditable, predictable.

## ADR-010 — UI: React + Tailwind tokens + Radix; white-first theme; restrained design; motion library for layout animation. Accepted
See `10-ui-system`. No gradients/neon/glass. Adaptive animation tiers for weak hardware.

## ADR-011 — ASSUMPTION: weakest target PC = 4 GB RAM, Windows 10, SSD. Open
To be replaced with real data. Drives initial budgets in `05-performance-budgets`. Windows 7/8 not supported until confirmed needed (WebView2 limitation).

## ADR-012 — Phase 0 spikes gate all feature work. Accepted
Receipt print + scanner + drawer, 500k SKU/5M lines benchmark on weakest PC, two-machine sync. Record measured numbers here when done.

### Phase 0 results (fill in)
| Metric | Budget | Measured | Date |
|---|---|---|---|
| Cold start | < 4 s | not measured — needs Windows WebView2 run (spec 002 scenario 7, debug route). No evidence yet. | — |
| Idle RAM total | < 400 MB | PROVISIONAL (dev VM): 20.2 MB peak RSS (runner process only — proxy, not the full app; real gate needs the app) | 2026-11-10 |
| Scan → line | < 50 ms | PROVISIONAL (dev VM): barcode warm p95 0.015 ms; fresh-connection (warm OS page cache, NOT a true cold read — the cache was not dropped for this run) 2.180 ms | 2026-11-10 |
| Search latency | < 100 ms | PROVISIONAL (dev VM): FTS5 prefix search p95 14.495 ms (p99 30.767 ms) at 500k SKUs | 2026-11-10 |
| Sale commit (tender → commit) | < 200 ms | PROVISIONAL (dev VM): p95 36.607 ms (p99 50.879 ms), 6-line sale, synchronous=FULL | 2026-11-10 |
| Sale commit under load | < 200 ms | PROVISIONAL (dev VM): p95 33.759 ms (p99 103.679 ms) while background writer + report run | 2026-11-10 |
| Report (90-day, aggregates) | < 1 s | PROVISIONAL (dev VM): 0.098 ms from daily_sales_summary | 2026-11-10 |
| Report (90-day, raw scan) | < 3 s (informational) | PROVISIONAL (dev VM): 100,259 ms (100 s) from raw sale_line — FAILS, by design: proves the daily aggregates are required. Re-recorded 2026-11-10 (was 122,014 ms; variance is disk/cache state). `EXPLAIN QUERY PLAN` shows a plain `SCAN sale_line` — no join, no missing index that would help a full `SUM`. The "honest 90-day" alternative (`sale_line JOIN sale` on `completed_at`) ran 6+ min without finishing, so the full scan is the honest baseline; the 122 s was cold-disk scan time (~4 MB/s), not a query bug. | 2026-11-10 |
| Keyset page (200 rows) | — | PROVISIONAL (dev VM): p95 ≤ 0.212 ms at depth 0 / 10% / 50% — no degradation with depth | 2026-11-10 |
| Hot-index build (100k active SKUs) | — | PROVISIONAL (dev VM): 294.6 ms | 2026-11-10 |
| Installer sizes | NSIS < 60 MB / MSI ~150 MB | PROVISIONAL (dev VM): largest artifact 8.479 MB (spec-000 fixed Windows exe) | 2026-11-10 |
| Hardware spike pass/fail | all pass | pending — specs 003/004 need hardware. No evidence yet. | — |

**Machine record (all numbers above are PROVISIONAL):** AMD EPYC 9D64 88-Core Processor / 7,934 MiB RAM / Linux, run 2026-11-10T09:26:38Z on the dev VM — NOT the target PC and NOT Windows. Only a run on the weakest PC (ADR-011, still an assumption) counts for the gate. Dataset: full spec-002 set — 500k SKUs (100k active), 600k barcodes, 100k customers, 1.2M sales, 5,397,732 sale lines, 5,397,732 stock movements, 499,987 stock balances; DB 2,295,103,488 bytes. Checksum: `sku=500000,barcode=600000,customer=100000,sale=1200000,sale_line=5397732,stock_movement=5397732,sale_sum=40528237620,line_sum=40528237620` (sale_sum = line_sum, consistency holds). Dataset deviations from spec 002: sale lines 5.40M vs the spec's 5M (seeder averages ~4.5 lines/sale over 1.2M sales); stock movements 5.40M vs the spec's 2M (one movement per sale line — the spec's own numbers imply 5M+, the seeded count is the consistent one). Scenarios 7 (startup) and 8 (UI timings) were not measured in this run — they need the debug route in the app on Windows/WebView2.

**Seed recovery note (2026-11-10):** the full seed ran ~60 min and was killed by a VM/service restart at ~00:49 during the final stock_balance aggregation (single statement = atomic, rolled back; all 1.2M sales were already committed). Recovered by re-running the exact `seed.rs` aggregation SQL. First two attempts stalled: the `(branch_id, sku_id, occurred_at)` index made the planner do 5M random table lookups for `qty_milli`/`tenant_id` (8–13 GB read, ~0 writes). Fixed with `NOT INDEXED` (one sequential scan + in-memory sort, 256 MB cache) — aggregation finished in 279.9 s. The seeded data is identical to a fresh seed (same deterministic seed).

**"Database is locked" flake — root cause + fix (2026-11-10):** the `run` failed ~3 in 8 times with `database is locked` right after scenario 5 started. Two causes. (a) `open_db` ran `PRAGMA journal_mode=WAL` before setting `busy_timeout`; proven by experiment that `PRAGMA journal_mode` does NOT honor `busy_timeout` — on a fresh rollback-mode DB it fails instantly with `SQLITE_BUSY` under a held lock even with a 5 s timeout. (b) Scenario 5's background writer did 200 autocommit `INSERT`s per batch; each fsyncs under `synchronous=FULL` (~12 ms on this disk: 200 inserts = 2.5 s), starving the main thread's commit past its 5 s `busy_timeout`. Fix: `open_db` sets `busy_timeout=5000` first and VERIFIES the DB is already in WAL mode (fails loudly if not — the seeder creates WAL DBs); scenario 5 opens the main connection before spawning threads and the writer wraps 200 inserts in one transaction per batch. Regression tests in `crates/bench/src/lib.rs` (`open_db_waits_for_brief_write_lock`, `scenario_sale_commit_under_load_succeeds` x3 on real disk). Trap: `/tmp` is tmpfs here, where an fsync is free — the starvation never reproduces there, so the scenario-5 test MUST use a real-disk path.

**Cold-barcode relabel (2026-11-10):** the "cold" number above (2.180 ms) was a fresh-connection lookup on a warm OS page cache — the cache was never dropped, so calling it cold was wrong. The runner now drops the OS file cache before the cold lookup when it can (Linux root, via `/proc/sys/vm/drop_caches`) and labels the result "cold" only then; otherwise it labels it "fresh connection" and says so in the description. On Windows there is no non-admin equivalent (needs RAMMap or a reboot), so Windows cold numbers are always "fresh connection" unless the operator drops the standby list first.

**Resumable seeder + dataset proof (2026-11-10):** `seed` now resumes an interrupted seed by default (10k-sale batches; `--fresh` to start over) and prints a deterministic `checksum:` at the end. Resume is proven by two tests (`seed_resume_is_idempotent`, `seed_resume_completes_partial_dataset` — a resumed seed produces the identical checksum as a fresh seed). `verify` subcommand (`just bench-verify`) checks `sale_sum == line_sum` and per-SKU balance == sum(movements) on the full dataset. **Full-dataset verify PASSED 2026-11-10** on the recovered DB: `sku=500000,barcode=600000,customer=100000,sale=1200302,sale_line=5399544,stock_movement=5399544,sale_sum=40530049620,line_sum=40530049620`, `sale_sum == line_sum: true`, 0 balance mismatches, 0 movements without a balance row — `verify: OK`. (sale/line counts are above the original 1.2M / 5,397,732 because later bench runs added ~302 sales; the sums still match, which is the consistency proof.) Two resume subtleties worth remembering: (a) a size smaller than one batch must not round its resume point to 0 (re-inserts everything); (b) sale-movement IDs share the line counter, not the total movement counter — receipts occupy the high IDs, so continuing from the total count collides with them. One verify trap, same as the seed recovery: the `movement_sums` GROUP BY must use `NOT INDEXED` — without it the planner uses `idx_movement_branch_sku_time` for index-ordered grouping and does one random table lookup per movement for `qty_milli` (~26 GB re-read, 20+ min); forcing a sequential scan + temp b-tree sort is one pass.

**Windows bench exe (2026-11-10):** CI (`bench-windows` job) builds `shopcore-bench.exe` (release) and uploads it as the `shopcore-bench-windows` artifact with `crates/bench/RUN.md` (one-page instructions). The exe prints required vs free disk space before seeding, resumes interrupted seeds, runs all scenarios, auto-collects the machine record (CPU model/cores, RAM, disk type, Windows build, WebView2 version, power plan), and writes `results.json` + `results.md`. Scenarios 7–8 come from the app's debug route (Ctrl+Shift+B) and are merged afterwards with `merge-ui --results-dir <dir> --ui-results <shopcore-ui-results.json>`. ADR-013 drift guard: a test in `crates/desktop/src-tauri/src/main.rs` fails if any Rust `#[tauri::command]` has no matching `invoke("name")` in `apps/ui/src`, or vice versa.

**ADR-002 recommendation (2026-11-10): PROVISIONAL, not evidence — the harness is accepted, the gate result is not.** The numbers below were measured on a Linux EPYC dev VM, not on Windows, not on WebView2, not on weak hardware. Scenarios 7–8 (startup, UI) were not measured at all. Nothing about WebView2, Windows, or weak-hardware performance is established by this run. The recommendation to keep Tauri is a provisional reading of dev-VM numbers only; the Electron fallback trigger (Phase 0 benchmark failure on the weakest PC) has not been tested, because the gate run has not happened. The gate still needs: (a) a run on the weakest PC (ADR-011) on Windows, (b) scenarios 7–8 measured via the debug route, (c) specs 003/004 hardware spikes passing. Until then, Phase 0 stays In progress and spec 001 stays blocked.

## ADR-013 — Type bridge: ts-rs 12.0.1 (stable). Accepted 2026-11-09
TS types are generated from Rust with `ts-rs` via `just gen-types` into `packages/protocol-ts`. Why: stable release; the Tauri-2-compatible specta line (`specta 2.0.0-rc.28` + `tauri-specta 2.0.0-rc.26`) is pre-release, and stable `specta` 1.0.5 / `tauri-specta` 1.0.2 target Tauri v1 only. Rejected: specta RC (re-evaluate when 2.x ships stable), hand-written types (drift). Consequence: command bindings stay hand-written in `TauriClient`, typed by the generated types.

## ADR-014 — TypeScript 5.9.3 pinned; TypeScript 7 re-evaluated later. Accepted 2026-11-09
Why: 5.9.3 is the proven line; npm `latest` (7.0.2) is the brand-new native rewrite with immature tooling support. Rejected: adopting TS 7 now. Consequence: re-evaluate after Vite/eslint support for TS 7 matures.

## ADR-015 — CI runs on `windows-latest` only (for now). Accepted 2026-11-09
Why: Windows-first target; the installer build needs Windows; Tauri on Linux needs webkit2gtk system libs. Rejected: Linux/macOS matrix now (comes later). Consequence: `just check` must be green on Windows CI; Linux dev runs are best-effort until the matrix lands.
Amendment 2026-11-10 (spec 005 review): the server/Postgres chaos tests run on a separate Linux `server-tests` job (ubuntu-latest, Postgres service container) — service containers are not supported on Windows runners. Till-side unit tests also run on the Windows `check` job. The workflow change lands with spec 005; until then no Linux job exists.

## ADR-016 — Installer budget split: online bootstrapper vs offline installer. Accepted 2026-11-09
The < 60 MB installer budget in `05-performance-budgets.md` applies to the online bootstrapper (app payload, excluding the WebView2 runtime, which downloads at install time). The offline installer (WebView2 bundled) has a ~150 MB budget. Why: the full offline WebView2 runtime alone exceeds 60 MB, so one budget cannot cover both artifacts. Consequence: record before Phase 6; release ships both artifacts.

## ADR-017 — Single currency per tenant in v1. Accepted 2026-11-09
`Money` stays `i64` minor units of the tenant's single currency. Multi-currency is Phase 3 reporting/conversion only. Why: the `Money` newtype carries no currency field, so two currencies would be indistinguishable in the type system. Rejected: multi-currency money in v1. Consequence: if true multi-currency money is ever needed, `Money` gains a currency field via ADR.

## ADR-018 — Tauri layout (SUPERSEDED by ADR-023, 2026-11-10)
Superseded: the layout decision moved wholesale into ADR-023, which also records the 2026-11-10 amendment (crate root inside `src-tauri/`, `frontendDist` corrected). This entry is kept as a pointer only.

## ADR-019 — Clippy denies at workspace level; `#[allow]` only at the top of test modules. Accepted 2026-11-09
Denied outside tests: `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` (plus `clippy::pedantic` selectively, enforced by `-D warnings` in `just check`). Why: the denies must cover all lib code; test code needs unwraps to stay readable. Rejected: per-crate lint tables (drift), blanket allows in lib code (never).

## ADR-020 — Toolchain pins: Rust 1.99.1, Node 22.23.4, pnpm 12.18.0. Accepted 2026-11-09
Rust pinned in `rust-toolchain.toml` (edition 2024); Node/pnpm pinned via the root `packageManager` field and CI `setup-node`. Why: reproducible builds and CI. Note: Rust 1.100.0 is due 2026-11-12 — bump deliberately after reading the release notes, never as a side effect. Consequence: local machines must install the pinned toolchain (`just setup` does this).

## ADR-021 — `just check` is defined once, in AGENTS.md. Accepted 2026-11-09
`just check` = fmt + clippy (`-D warnings`) + `cargo deny` + nextest + tsc + eslint + vitest. CI adds `just bench`, migration tests, installer build, `pnpm audit`, and secret scanning when their specs land. Why: `AGENTS.md` and `09-engineering-standards.md` previously defined `just check` differently (deny included in one, missing in the other).

## ADR-022 — Vendored OpenSSL on Windows CI needs Strawberry Perl, pinned via OPENSSL_SRC_PERL. Accepted 2026-11-10
Risk R2 (spec 000) triggered on the first Windows CI run: `openssl-sys` (vendored OpenSSL, for SQLCipher) failed at OpenSSL's `Configure` because the `perl` the build script resolved was Git's MSYS2 perl, which lacks the core module `Locale::Maketext::Simple`. First fix (prepend `C:\Strawberry\perl\bin` to PATH) did NOT work — the build still resolved the MSYS2 perl even though `perl -v` in the same job showed Strawberry 5.42. Deterministic fix: CI sets job-level `OPENSSL_SRC_PERL=C:\Strawberry\perl\bin\perl.exe` in both jobs (openssl-src reads that variable first, before `PERL` and PATH). The PATH prepend is kept as a belt-and-braces measure. Why this over the R2 fallback (vcpkg OpenSSL + `sqlcipher` feature): it keeps the approved dependency and feature exactly as specified — no dependency change, no new package manager, no extra build time. If Strawberry Perl ever leaves the runner image, fall back to R2's vcpkg route via a new ADR. Consequence: `cargo deny` advisories must stay in CI — OpenSSL is vendored for SQLCipher, so security patches arrive only by rebuilding the vendored copy, and a dropped advisory check would let a known-vulnerable OpenSSL ship silently.

## ADR-023 — Desktop crate lives at the standard Tauri layout: `crates/desktop/src-tauri` is the crate root. Accepted 2026-11-10
The original spec-000 layout put `Cargo.toml` at `crates/desktop/` with `main.rs`/`build.rs` under `src-tauri/` — the Tauri CLI requires `src-tauri/Cargo.toml` and failed instantly ("failed to watch …/src-tauri/Cargo.toml"), so `just dev` and `just build` could never run. The same split masked a second error: `frontendDist: "../../apps/ui/dist"` only resolved by accident from the old crate root (from the config dir it pointed at nonexistent `crates/apps/ui/dist`), so the first Linux debug binary may have embedded no frontend at all. Decision: the crate root IS `crates/desktop/src-tauri/` (Cargo.toml, build.rs, tauri.conf.json, `src/main.rs`, `capabilities/`, `icons/`); the root workspace lists `crates/desktop/src-tauri` as a member; `frontendDist` is `../../../apps/ui/dist` (one value serves CLI + codegen, since the config dir is now the manifest dir); `just dev`/`just build` invoke the Tauri CLI binary directly from `crates/desktop/src-tauri` (pnpm `--filter` re-chroots into `apps/ui` and the CLI cannot find the config). Rejected: keeping `Cargo.toml` at `crates/desktop/` with `config_path` + `codegen` workarounds — fragile, CLI-incompatible, and it hid the frontendDist error. Consequence: the repo layout in `03-architecture.md` now records this; `just dev`/`just build` are the only supported entry points for the desktop app. This supersedes ADR-018 (retained as a pointer only) and formalizes its 2026-11-10 amendment: the layout rationale (keeps the repo layout in `03-architecture.md` while satisfying Tauri 2's `src-tauri` convention) and `just dev` running the Tauri CLI from `crates/desktop/src-tauri` with `beforeDevCommand`/`beforeBuildCommand` running the UI via pnpm filter.

## ADR-024 — Spike specs use the short format plus Data changes, Security/audit, Definition of done. Accepted 2026-11-10
Specs 002–005 use the short format (Goal / In scope / Out of scope / Requirements or Scenarios / Tests / Dependencies / Acceptance / Risks) plus three sections: Data changes, Security/audit, Definition of done. They are NOT converted to the full template in `specs/_template.md`. Why: spikes prove an unknown with mocks and manual checklists; the full template's business-rule and UI sections do not apply yet. Consequence: when a spike graduates to a feature spec (spec 001 uses 003/004), the feature spec uses the full template.

## ADR-025 — Spec 005: sqlx features dropped to avoid the sqlx-sqlite / rusqlite `links` conflict. Accepted 2026-11-10
sqlx 0.9.0's `migrate`, `uuid`, and `json` features each pull in `sqlx-sqlite` (verified by bisection in a scratch crate), whose `libsqlite3-sys` (< 0.38.0) conflicts with rusqlite 0.41.1's `libsqlite3-sys` (0.39.x) — both link the native `sqlite3` library, and Cargo forbids two versions of a `links` crate in one resolve. Decision: sqlx runs with `["postgres", "runtime-tokio", "tls-rustls"]` only. Migrations run via `sqlx::raw_sql` (single migration file); UUIDs are bound as text (Postgres casts text → uuid via explicit `::uuid` casts in SQL) and read back via `::text` casts + `Uuid::parse_str`; JSON payloads are bound as text (Postgres casts text → JSONB via `::jsonb`). Consequence: if sqlx ever needs `migrate`/`uuid`/`json`, the conflict must be resolved first (e.g. a rusqlite version whose `libsqlite3-sys` overlaps sqlx-sqlite's range) via a new ADR.

## ADR-026 — Sync client is a separate crate, never in the desktop binary. Accepted 2026-11-10
`crates/sync-client` holds the till-side HTTP client (`reqwest` + `aws-lc-rs`). It is NOT a dependency of `crates/desktop/src-tauri` (verified by `cargo tree -p shopcore-desktop`: no `reqwest`, no `aws-lc-rs`, no `sync-client`). Consequence: the shipped desktop installer size is unchanged by spec 005 (delta 0 — the desktop binary links no sync code; the server and sync-client are separate binaries, not shipped in the installer). `aws-lc-rs` vs `ring`: `aws-lc-rs` is reqwest 0.13's default rustls crypto provider; `ring` would need the `rustls-no-provider` feature plus a manual `CryptoProvider` install, and `ring` is itself native (C/assembly) code — so `ring` buys no pure-Rust advantage, only more wiring. Both are native crypto, neither is OpenSSL. If a pure-Rust TLS provider is ever required, that is a new ADR.

## ADR-027 — `just check` excludes the Postgres-backed chaos test binary. Accepted 2026-11-10
CI run 42852687980 failed on the Windows `check` job: `just check` runs `cargo nextest run --workspace`, which included the spec-005 chaos tests, and the Windows runner has no Postgres — the tests failed loudly at the `expect("Postgres must be up")` line, exactly as designed (spec 005 amendment 5: never skip silently). Decision: `just check` and `just test` run nextest with `--filter-expr "not binary(chaos)"`, excluding only the `chaos` integration-test binary. The chaos tests run only in the Linux `server-tests` CI job (ADR-015, which has a Postgres service and fails if it is unavailable) and locally via `cargo nextest run -p shopcore-server --test chaos`. Future server unit tests still run in `just check` — only the Postgres-backed binary is excluded.
## ADR-028 — `run_migrations` serializes concurrent runs with a Postgres advisory lock. Accepted 2026-11-10
CI run 42857202536: the new `server-tests` job failed — on its fresh Postgres, the 12 chaos tests (nextest runs each in its own process) called `run_migrations` concurrently, and parallel `CREATE TABLE IF NOT EXISTS` raced: two processes both passed the existence check, then both tried to insert the table's type into `pg_type` and one died with `duplicate key ... pg_type_typname_nsp_index ... (applied_push, 2200) already exists`. Locally it never fired because the tables already existed. Decision: `run_migrations` now runs the whole migration inside one transaction that first takes a transaction-scoped advisory lock (`SELECT pg_advisory_xact_lock(727271)`); the second process blocks until the first commits, then sees the tables exist. An in-process `OnceLock` cannot fix this (separate processes). Verified: 12/12 chaos tests pass against a freshly created database. This also protects two server binaries starting at once.

## ADR-029 — Phase 0 benchmark gate deferred by the owner. Accepted 2026-11-10
The Phase 0 benchmark gate no longer blocks development. Reason: no weak-spec PC is available, so the full Windows benchmark (scenarios 7–8, 500k snapshot, checkout under sync) cannot run on the target machine. Risk: Tauri/WebView2 weight and startup on weak PCs are unmeasured. Mitigations: the `CoreClient` boundary keeps the shell replaceable (ADR-002 fallback stays valid — Electron sidecar with the Rust core); budgets stay enforced by the CI bench-smoke tripwire (relative regression check vs the stored baseline). Hard checkpoint: the full Windows benchmark MUST run before the real-shop pilot (Phase 4) at the latest. Do not remove the benchmark tooling (spec 002 harness, `shopcore-bench.exe`, RUN.md, debug route).

## ADR-031 — Migrations may be squashed/rewritten until the first customer install. Accepted 2026-11-10
Migrations may be squashed or rewritten while the product has no customer
install (development only). After the FIRST customer install, migrations
are STRICTLY forward-only: never edited, only new numbered migrations.
Rationale: the schema is still settling during 001b-1/001b-2 (tables are
being dropped and added per the 001 flow), and rewriting a migration no
customer has run is safe. Once a customer has run a migration, editing it
would diverge their database from the code. The cut-over is the first
customer install, not a version number — the release that ships to the
first customer freezes the migration history.

## ADR-030 — Scanner events use `tokio::sync::mpsc`, not `Stream`. Accepted 2026-11-10
`06-hardware-integrations.md` sketches `BarcodeScanner::events() -> Stream<ScanEvent>`, but neither `futures` nor `tokio-stream` is a workspace dependency (only transitive `futures-core`/`futures-util` in the lockfile). Decision: scanner events are delivered over a `tokio::sync::mpsc::Receiver<ScanEvent>` — no new dependency (tokio `sync` feature is already enabled). The `06` sketch is updated to match. If a `Stream` API is ever needed (e.g. combinators over the event flow), adding `tokio-stream` is a new ADR with its own dependency proof.

## ADR-033 — Production CSP: `style-src 'self'` + `style-src-attr 'unsafe-inline'`; dev relaxations in `devCsp` only. Accepted 2026-11-10
Production CSP (owner decision 2026-11-10, follow-up to the closed spec
000b): `default-src 'self'; script-src 'self'; style-src 'self';
style-src-attr 'unsafe-inline'; img-src 'self' data:; font-src 'self';
connect-src 'self' ipc: http://ipc.localhost`. The `style-src
'unsafe-inline'` that shipped with spec 000 is REMOVED from production.
Why it is safe: the production build emits an external CSS file and
contains no inline `<style>` element (verified against the built dist:
`dist/index.html` has 0 `<style>` tags, no inline `style` attributes in
the HTML, styles arrive via `<link rel="stylesheet">`). The ONLY style
exception is `style-src-attr 'unsafe-inline'`, because the
@tanstack/react-virtual rows in `apps/ui/src/features/bench/BenchDebug.tsx`
(the only `style={{...}}` props in the source) render inline `style`
attributes at runtime — without it the virtualized list loses its
row heights/positions. Inline `<style>` ELEMENTS stay blocked
(`style-src 'self'`). Dev-only relaxations live in `devCsp` only:
`style-src 'self' 'unsafe-inline'` (Vite HMR injects `<style>` tags in
dev); every other directive is identical to production, and `devCsp`
never allows `'unsafe-inline'` in `script-src` or `default-src`
(enforced by the CSP test in `apps/ui/src/test/csp.test.ts`).

## ADR-032 — Argon2id PIN parameters: OWASP minimum kept. Accepted 2026-11-11
PIN hashing is Argon2id (`argon2` 0.6.0, PHC string format) with the OWASP
minimum parameters: 19 MiB memory, 2 iterations, 1 lane, Argon2 version
0x13. Measured verification time on the dev VM (mean over 5 runs, after
one warm-up run, `crates/core/src/pin.rs::measure_verify_time`):
- Release (the shipping till): **53–91 ms** across two runs.
- Debug: **~1,765 ms** — debug codegen is ~30x slower and is not the
shipping artifact; the debug test bound is a 5 s sanity bound only, the
release bound is 1 s.

Decision: KEEP the OWASP minimum — no weakening. A PIN verification
happens once per sign-in / manager approval, never on the scan path, and
~50–90 ms is unnoticeable there. Verification always runs OFF the UI
thread (`verify_pin_async` → `tokio::task::spawn_blocking`); the lockout
(5 failures → 5 minutes, configurable via `settings`) and the dummy-hash
run for unknown usernames cost one verification each by design. If a
future target PC measures materially slower, the trade-off (lower memory
vs longer lockout) is recorded here first — never silently weakened.

## ADR-034 — Draft cart lives in a separate scratch DB; accepted loss window. Accepted 2026-11-11
The open draft cart (`draft_sale` / `draft_line`) is persisted in a
SEPARATE scratch DB file with its own connection, `WAL` +
`synchronous=NORMAL` — never in the main DB, which stays
`synchronous=FULL`. Every draft change goes through the draft use
cases (`draft_add_line`, `draft_set_qty`, `draft_remove_line`,
`draft_apply_discount`, `draft_cancel`, `draft_restore_line`); the
scan path never touches the main DB with a `FULL` flush. The sale
commit (`complete_sale_cash`) is one transaction on the main DB only.

Accepted loss window: in a power cut, at most the scratch DB's
unflushed tail can be lost — the cashier re-scans the last line or
two. A committed sale is NEVER affected: it lives in the main DB
(`synchronous=FULL`) and is idempotent per `draft_id` (UNIQUE), so a
second `complete_sale_cash` with the same `draft_id` returns the
existing sale. A completed draft is TOMBSTONED in the scratch DB and
restore discards any draft whose sale exists in the main DB, so a late
scratch-DB flush can never resurrect a completed draft. A process kill
cannot simulate a power cut (the OS still flushes the file on process
death) — the crash tests prove ATOMICITY and RECOVERY only; the
power-cut loss itself is proven by the manual power-pull test, a
pre-pilot checklist item (owner: user).

## ADR-009 assessment (spec 005, 2026-11-10) — KEEP
The spike's 12 chaos tests all pass against a real Postgres (local) — duplicate delivery, out-of-order batches, connection drop, server restart, clock skew, two tills selling the last unit, upgrade with unsent outbox, snapshot interrupt/resume, full-history replay (byte-identical, unique constraint proven to fire), per-field LWW merge, gap detector, revalidation stub. No chaos test exposed a protocol design flaw, so the stop condition did not trigger. ADR-009 (custom outbox/inbox sync, not generic CRDT) is KEPT for Phase 5. Two notes for Phase 5: (1) the per-tenant sequence lock serializes batches per tenant — under high concurrency this is a throughput ceiling to measure; (2) the snapshot catalog is the set of SKUs the server has seen (via movements/price entries), not the full 500k-SKU catalog — the full-catalog snapshot is the manual target-PC run.

## Spec 005 performance (PROVISIONAL, dev VM, 2026-11-10)
Measured locally (Postgres 16, localhost, AMD EPYC VM): push **986 rows/s** (2000 rows in 10×200-row batches, one HTTP round-trip + one Postgres transaction per batch); pull **6594 rows/s** (50 price entries); snapshot **2070 bytes** for the spike's 1-SKU test data (built 0.010s, downloaded 0.050s, SHA-256 verified, Range resume proven). The 500k-SKU snapshot size/download time is the manual target-PC run, not measured here. Checkout latency while sync runs (spec 002 scenario 4 baseline vs scenario 5 under load) is NOT measured in this spike — it is a Phase 0 gate measurement for the target-PC run (the till's sale commit is a local SQLite transaction; the sync worker pushes over HTTP and does not hold the till's DB lock during the push, so no interference is expected, but it is unproven until measured).

## Open questions
- Launch markets, fiscal/tax rules, currencies, languages.
- First payment terminal integration.
- Pricing/licensing model.
- Weakest supported PC (replace ADR-011).
- **ADR-017 vs `01-product-brief.md` (DECIDED 2026-11-10, Option A):** ADR-017 stands — single currency per tenant in v1, multi-currency is Phase 3 reporting/conversion only. `01` line 30 no longer lists multi-currency in v1; the open item now reads "v1: single currency per tenant, chosen at tenant setup (ADR-017)"; Phase 3 line added (foreign-currency tender at the payment layer + multi-currency reporting). `02-domain-model.md` records the tenant-currency rule (set at setup, immutable after the first transaction; formatting uses the tenant currency; `Money` stays currency-agnostic).
