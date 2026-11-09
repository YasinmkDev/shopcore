# AGENTS.md — ShopCore (working codename)

Enterprise retail POS + back-office desktop app for shops and mega marts.
Stack: Tauri 2 shell + React/TypeScript UI + Rust core + Rust server (Axum/Postgres). Windows first.
Offline-first. Money-critical. Performance-critical. Read this file fully every session; it is short on purpose.

## Session protocol (follow in order)
1. Read `context/13-progress-tracker.md` (where we are, what is next).
2. Read the spec for the task in `specs/`. If no spec exists for a non-trivial feature, write one from `specs/_template.md` and STOP for approval.
3. Read only the context files the task needs (routing table below).
4. Propose a short plan (files to touch, tests to add). Wait for approval on anything non-trivial.
5. Write tests first (or alongside), then implement as one thin vertical slice (UI + core + DB + tests).
6. Run `just check`. It must pass. Never claim done on red.
7. Update `13-progress-tracker.md`. Log any new decision in `08-decisions.md`.
8. End with a summary: what changed, what was tested, what is left, any risks.

## Hard rules (never violate)
- Money is `i64` minor units via the `Money` type. NEVER f32/f64 for money, quantity, tax, or discounts.
- Quantities are fixed-point integers (`Qty`, 3 decimals). Tax rates are basis points (1800 = 18.00%).
- Stock is an append-only ledger (`stock_movements`). Never edit or delete movements; never write `stock_balances` outside the posting function.
- `crates/domain` is pure: no I/O, no clock, no randomness, no DB, no network. Inject everything.
- The checkout path makes ZERO network calls and never waits on sync, printing, or disk-heavy work.
- Every sale is ONE SQLite transaction (sale, lines, payments, stock movements, balances, aggregates, audit, outbox).
- Never block the UI or the Tauri main thread. DB access only through the DB actor. Hardware only through driver traits.
- All IDs are UUIDv7 generated client-side. Soft delete only (`deleted_at`). Every table has `tenant_id`.
- Every sensitive action writes an audit row (see `07-security-compliance.md`).
- No `unwrap()`/`expect()`/`panic!` in non-test Rust. No `any`, no `@ts-ignore` in TypeScript.
- Validate every IPC/HTTP input at the boundary (Rust side is authoritative).
- All user-visible strings go through i18n keys. Layout must work RTL.
- No new dependency, no schema change, and no change to a documented invariant without asking first.
- No secrets, keys, or customer data in code, logs, fixtures, or commits.
- Do not edit files outside the task scope. Do not refactor unrelated code.

## Stop and ask when
- A requirement is ambiguous, or two context files conflict (do not guess; propose and ask).
- You need a new dependency, schema migration, public API change, or a performance budget exception.
- A business rule (tax, discount, rounding, costing) is not specified in `02-domain-model.md`.

## Routing table (read these for this kind of task)
| Task type | Read |
|---|---|
| Any task | `13-progress-tracker`, `08-decisions` (skim) |
| Pricing, tax, discounts, returns, shifts, stock | `02-domain-model`, `11-test-strategy` |
| Database, migrations, sync, backup | `04-data-and-sync`, `03-architecture` |
| Anything touching speed, lists, search, startup | `05-performance-budgets` |
| Printer, scanner, drawer, scale, payment terminal | `06-hardware-integrations` |
| Login, roles, audit, encryption, updates, licensing | `07-security-compliance` |
| Any UI work | `10-ui-system` (mandatory) + `05-performance-budgets` |
| New crate, command, module, library usage | `03-architecture`, `09-engineering-standards` |
| Planning / scope | `01-product-brief`, `12-build-plan` |

## Commands
- `just dev` run desktop app with hot reload
- `just check` fmt + clippy(-D warnings) + cargo deny + cargo nextest + tsc + eslint + vitest (must pass). This is the single definition of `just check`; CI adds `just bench`, migration tests, installer build, `pnpm audit`, and secret scanning when their specs land (ADR-015, ADR-021).
- `just test` tests only; `just bench` perf benchmarks against budgets in `05-performance-budgets`
- `just gen-types` regenerate TS types from Rust (never hand-edit generated files)
- `just migrate` apply local DB migrations; `just seed-large` seed 500k SKUs / 5M sale lines

## Definition of done
Tests added and passing, `just check` green, budgets not regressed (`just bench` if touching hot paths), UI matches `10-ui-system` (all states: loading, empty, error, offline), keyboard-operable, i18n keys used, tracker + decisions updated.
