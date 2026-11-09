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
| Cold start | < 4 s | | |
| Idle RAM total | < 400 MB | | |
| Scan → line | < 50 ms | | |
| Search latency | < 100 ms | | |
| Hardware spike pass/fail | all pass | | |

## ADR-013 — Type bridge: ts-rs 12.0.1 (stable). Accepted 2026-11-09
TS types are generated from Rust with `ts-rs` via `just gen-types` into `packages/protocol-ts`. Why: stable release; the Tauri-2-compatible specta line (`specta 2.0.0-rc.28` + `tauri-specta 2.0.0-rc.26`) is pre-release, and stable `specta` 1.0.5 / `tauri-specta` 1.0.2 target Tauri v1 only. Rejected: specta RC (re-evaluate when 2.x ships stable), hand-written types (drift). Consequence: command bindings stay hand-written in `TauriClient`, typed by the generated types.

## ADR-014 — TypeScript 5.9.3 pinned; TypeScript 7 re-evaluated later. Accepted 2026-11-09
Why: 5.9.3 is the proven line; npm `latest` (7.0.2) is the brand-new native rewrite with immature tooling support. Rejected: adopting TS 7 now. Consequence: re-evaluate after Vite/eslint support for TS 7 matures.

## ADR-015 — CI runs on `windows-latest` only (for now). Accepted 2026-11-09
Why: Windows-first target; the installer build needs Windows; Tauri on Linux needs webkit2gtk system libs. Rejected: Linux/macOS matrix now (comes later). Consequence: `just check` must be green on Windows CI; Linux dev runs are best-effort until the matrix lands.

## ADR-016 — Installer budget split: online bootstrapper vs offline installer. Accepted 2026-11-09
The < 60 MB installer budget in `05-performance-budgets.md` applies to the online bootstrapper (app payload, excluding the WebView2 runtime, which downloads at install time). The offline installer (WebView2 bundled) has a ~150 MB budget. Why: the full offline WebView2 runtime alone exceeds 60 MB, so one budget cannot cover both artifacts. Consequence: record before Phase 6; release ships both artifacts.

## ADR-017 — Single currency per tenant in v1. Accepted 2026-11-09
`Money` stays `i64` minor units of the tenant's single currency. Multi-currency is Phase 3 reporting/conversion only. Why: the `Money` newtype carries no currency field, so two currencies would be indistinguishable in the type system. Rejected: multi-currency money in v1. Consequence: if true multi-currency money is ever needed, `Money` gains a currency field via ADR.

## ADR-018 — Tauri layout: `crates/desktop/src-tauri`, `frontendDist` → `../../apps/ui/dist`. Accepted 2026-11-09
Why: keeps the repo layout in `03-architecture.md` (Tauri app in `crates/desktop`, UI in `apps/ui`) while satisfying Tauri 2's `src-tauri` convention. Consequence: `just dev` runs `cargo tauri dev` from `crates/desktop`; `beforeDevCommand`/`beforeBuildCommand` run the UI via pnpm filter.

## ADR-019 — Clippy denies at workspace level; `#[allow]` only at the top of test modules. Accepted 2026-11-09
Denied outside tests: `unwrap_used`, `expect_used`, `panic`, `indexing_slicing` (plus `clippy::pedantic` selectively, enforced by `-D warnings` in `just check`). Why: the denies must cover all lib code; test code needs unwraps to stay readable. Rejected: per-crate lint tables (drift), blanket allows in lib code (never).

## ADR-020 — Toolchain pins: Rust 1.99.1, Node 22.23.4, pnpm 12.18.0. Accepted 2026-11-09
Rust pinned in `rust-toolchain.toml` (edition 2024); Node/pnpm pinned via the root `packageManager` field and CI `setup-node`. Why: reproducible builds and CI. Note: Rust 1.100.0 is due 2026-11-12 — bump deliberately after reading the release notes, never as a side effect. Consequence: local machines must install the pinned toolchain (`just setup` does this).

## ADR-021 — `just check` is defined once, in AGENTS.md. Accepted 2026-11-09
`just check` = fmt + clippy (`-D warnings`) + `cargo deny` + nextest + tsc + eslint + vitest. CI adds `just bench`, migration tests, installer build, `pnpm audit`, and secret scanning when their specs land. Why: `AGENTS.md` and `09-engineering-standards.md` previously defined `just check` differently (deny included in one, missing in the other).

## Open questions
- Launch markets, fiscal/tax rules, currencies, languages.
- First payment terminal integration.
- Pricing/licensing model.
- Weakest supported PC (replace ADR-011).
