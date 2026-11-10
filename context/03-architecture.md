# 03 — Architecture

## Stack (decided; changes need an ADR in `08-decisions.md`)
| Layer | Choice |
|---|---|
| Shell | Tauri 2, Windows first (WebView2) |
| UI | React 19 + TypeScript (strict) + Vite, Tailwind with custom tokens, Radix primitives, TanStack Table + Virtual + Query, Zustand, `motion` for layout animation |
| Core | Rust (stable, pinned in `rust-toolchain.toml`), tokio, `rusqlite` (WAL) with SQLCipher, `tracing`, `serde`, `thiserror` |
| Domain | `crates/domain`: pure Rust, no I/O |
| Server | Rust: Axum + sqlx + PostgreSQL, tenant row-level security, monthly partitions, job queue |
| Type bridge | `specta` (or `ts-rs`) generates TS types from Rust; never hand-edit generated files |
| Tooling | `just`, `cargo-nextest`, `proptest`, Vitest, Testing Library, Playwright |
| Release | Tauri bundler, code signing, Tauri updater with staged rollout |

## Core idea: headless core + thin UI
All business logic, data access, sync, and hardware live in Rust. The React UI renders state and sends intents. The UI never touches the DB, filesystem, or hardware directly. A single `CoreClient` TypeScript interface has two transports, so the UI is unaware of deployment mode:
- `TauriClient`: in-process commands/events (embedded mode).
- `HttpClient`: HTTP + WebSocket to a store hub (client mode; also used for browser-based e2e tests).

## Deployment modes (same code, one binary family)
1. **Embedded**: single-till shop. Core runs inside the desktop app. Syncs to cloud when online.
2. **Hub**: one PC in a multi-till store runs `hub` (headless service, installable as a Windows service). It owns the store DB and syncs with cloud.
3. **Client**: other tills connect to the hub over LAN. They keep a local read cache and an outbox; if the hub is unreachable they continue selling and queue sales (append-only, merges safely).
4. **Cloud**: head office, multi-branch reporting, remote admin, backups.

## Repo layout
```
Cargo.toml (workspace) | package.json (pnpm workspace) | justfile | rust-toolchain.toml
crates/
  domain/        # pure: Money, Qty, pricing, tax, promotions, stock rules, validation
  core/          # application services/use cases, DB actor, sync engine, permissions
  db/            # migrations (SQL), queries, FTS, aggregates
  hardware/      # driver traits + implementations + mock drivers
  protocol/      # shared request/response/event types, sync wire format
  desktop/       # Tauri app (embeds core). The crate root is
                 # `crates/desktop/src-tauri/` (Cargo.toml, build.rs,
                 # tauri.conf.json, src/main.rs, capabilities/, icons/) —
                 # the Tauri CLI requires `src-tauri/Cargo.toml` (ADR-023).
                 # `crates/desktop/` itself holds only `src-tauri/`.
  hub/           # headless LAN service
  server/        # cloud API (Axum)
  bench/         # perf benchmarks + large-dataset seeding (see 05)
apps/
  ui/            # React app (Vite)
packages/
  ui-kit/        # design tokens, components (see 10-ui-system)
  i18n/          # translations
  protocol-ts/   # GENERATED TypeScript types from Rust (never hand-edit)
tests/
  golden/        # JSON golden datasets (tax, discounts, stock)
  e2e/
context/ specs/ AGENTS.md
```

## Layering (dependencies point inward only)
`domain` ← `core` ← (`desktop` | `hub` | `server`). `domain` depends on nothing but std + small pure crates. Adapters (SQLite, hardware, HTTP, clock, UUID) are traits defined in `core`/`domain` and implemented at the edges, so everything is testable with fakes.

## Modules (bounded contexts)
catalog, pricing, promotions, sales, payments, cash-shifts, inventory, purchasing, customers, reporting, identity-rbac, audit, sync, settings, hardware. Each module exposes use cases (functions) and its own tables; cross-module calls go through use cases, not shared table access.

## Process and thread model
- Tauri main thread: window + IPC only. Never blocks.
- **DB actor**: one dedicated thread owning the single write connection; commands arrive over a channel. A pool of read-only connections serves queries (WAL allows concurrent readers).
- **Hardware worker(s)**: isolated tasks per device with timeouts and restart on failure; a printer hang can never stall checkout.
- **Sync worker**: background task, small batches, separate connection, yields to checkout.
- UI state in renderer is ephemeral (cart in progress mirrors core draft state; core is the source of truth for drafts so a crash does not lose a cart).

## IPC contract
- Commands are typed, versioned, and validated in Rust. Return `Result<T, AppError>` with stable error codes (UI maps codes to localized messages).
- Large lists use keyset pagination (`after_cursor`, `limit`), never OFFSET, never full dumps. Max page 200 rows.
- Events (core → UI): `sale.completed`, `sync.status`, `hardware.status`, `stock.low`, `settings.changed`. Payloads are small; UI refetches via query keys.

## The sale write path (single transaction)
```
BEGIN IMMEDIATE
  insert sale + sale_lines + sale_payments
  insert stock_movements (type=sale) per line; update stock_balances
  update aggregates (daily_sales_summary, daily_item_sales, daily_cash_summary)
  update customer ledger/loyalty if applicable
  insert audit_log rows (overrides, discounts, price changes)
  insert outbox row(s) for sync
COMMIT   -- synchronous=FULL on the write connection
then (outside the transaction, async): print receipt, kick drawer, emit event
```
Printing failure never rolls back a completed sale; it queues a reprint.

## Error handling
`thiserror` enums per module; `AppError` at the boundary with stable codes. No panics. Every error path has a user-visible, actionable message ("Barcode not found. Add as new item?"). Hardware errors degrade gracefully (sale completes, receipt queued).

## Observability
`tracing` structured logs to rotating local files (no PII, no card data). Sentry (opt-in per tenant) for crashes. A "Diagnostics bundle" button exports logs, versions, DB integrity check, sync status for remote support.

## Configuration and feature flags
Tenant/branch/terminal settings live in the DB (synced). Feature flags in settings table, evaluated in core. No config in UI code.

## Windows specifics
Install per-machine, data under `%ProgramData%\ShopCore` (hub) or `%AppData%` (embedded). SQLite DB key protected with Windows DPAPI. Auto-start option for hub as service. WebView2 runtime bundled in installer (offline installer option).
