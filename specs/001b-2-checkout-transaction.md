# Spec 001b-2 — Checkout core: draft cart, sale transaction, stock, audit, outbox, crash safety

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 001a (checkout domain), 001b-1 (foundation), 002 (benchmark harness), 005 (outbox)

## Goal (one sentence, user-visible outcome)
A cash sale is one atomic database transaction: it either completes fully
(sale, lines, payment, stock, balances, aggregates, audit, outbox) or not
at all, and it survives a power cut mid-commit.

## Users and scenario
A cashier completes a cash sale; the till never loses it, double-counts it,
or leaves stock wrong — even if the power dies during the commit.

## In scope / Out of scope
In: draft cart use cases (`draft_add_line`, `draft_set_qty`,
`draft_remove_line`, `draft_apply_discount`); discount approval
(manager-PIN path); `complete_sale_cash` (single-transaction sale);
stock posting (append-only movements, balances only via the posting
function); aggregates update; audit writing (with the tamper-evident
hash chain from day one); outbox writing in the same transaction;
receipt numbering (per-terminal sequence); print queue (via the
`ReceiptPrinter` trait + `MockPrinter`); crash tests (fault injection +
real process kills, Windows CI).
Out: migrations, DB actor, search/lookup, users/roles/PIN hashing,
hardware traits/mocks, dev seed (all 001b-1); any UI, any Tauri command
(001c); real hardware drivers (specs 003/004); promotions, split
payments, returns, hold/recall, customers, card/wallet.

## Functional requirements (numbered, testable)
1. Draft cart: every change goes through the draft use cases
   (`draft_add_line`, `draft_set_qty`, `draft_remove_line`,
   `draft_apply_discount`); the draft is persisted in `draft_sale` /
   `draft_line` (migrated in 001b-1) and restored after crash/restart.
2. Draft persistence MUST NOT put a `synchronous=FULL` flush on the
   scan path (the scan path is the hot path — a FULL flush per keystroke
   would break the < 50 ms budget). Accepted approach: a COALESCED
   ASYNC FLUSH (draft writes batch and flush within ~250 ms of the last
   change) or an ATTACHED SCRATCH DB with `synchronous=NORMAL` for the
   draft (the sale commit stays `synchronous=FULL`). The ACCEPTED LOSS
   WINDOW is documented in the code and the session summary: at most
   the last ~250 ms of draft edits (or the scratch DB's unflushed tail)
   can be lost in a power cut — the committed sale is never affected,
   and the worst case is the cashier re-scans the last line or two.
3. `complete_sale_cash`: ONE SQLite transaction writing sale, lines,
   per-component tax rows (`sale_line_tax`), payment, stock movements,
   balances, aggregates, audit, and the outbox row. Totals come from
   the 001a domain functions. Stock balances are written ONLY by the
   posting function (never directly). The sale commits only if a shift
   is open (`SHIFT_NOT_OPEN` otherwise) and the caller holds
   `sale.create`.
4. Line discount above the cashier's threshold requires manager PIN
   (approval recorded with both users, audited); without approval →
   `DISCOUNT_NEEDS_APPROVAL`. PINs are verified against Argon2id hashes
   (001b-1); never logged.
5. Cash tender below total → `INSUFFICIENT_TENDER`; change computed by
   the 001a function. Cash rounding is OFF by default (001a req. 9):
   the acceptance example is total 52.32, tender 60.00, change 7.68
   with no rounding line. When a tenant turns rounding ON, the rounding
   is a separate untaxed line on the cash-tendered total after tax.
6. Receipt number is generated per terminal from the 001a function with
   the last sequence read from the DB (injected into the pure function).
7. Print/drawer actions go through the `ReceiptPrinter` / `CashDrawer`
   traits with `MockPrinter` in this spec; the sale completes and the
   receipt queues even if the printer is offline (reprint queued,
   `PRINT_QUEUED`); hardware never blocks the commit. Real drivers plug
   in via specs 003/004 with no change to this code.
8. Audit writing: every sensitive action writes an audit row (sale
   completion, discount, price override, drawer open, shift open/close,
   reprint, PIN lockout — the lockout event comes from 001b-1). Each
   row: who, what, entity, before/after, reason, terminal, time (07).
   The audit_log has a TAMPER-EVIDENT HASH CHAIN from day one (07 line
   19): each row stores the hash of the previous row; the chain is
   verified on read and a broken chain is reported, never silently
   repaired. No audit row is ever written without its chain link.
9. Outbox: every local mutation writes an outbox row in the same
   transaction (04 line 38); the outbox schema is the one from spec 005
   (`sync-client` helpers `ensure_outbox_schema` / `mark_sent`); the
   server revalidates each synced sale with the same domain crate
   (04 line 53).
10. Stock posting: sale writes a `stock_movement` per line (append-only)
    and updates `stock_balance` via the posting function only; after any
    sequence, `balance == sum(movements)` per (branch, sku).

## Business rules touched (link to `context/02-domain-model.md` sections)
- Sales (states `draft → held → completed` / `voided`; completed sales
  immutable; receipt number; cash rounding OFF by default, ON only as a
  separate untaxed line; void vs return).
- Inventory (sale movement + balance; `balance == sum(movements)` per
  branch+sku; append-only ledger).
- Shifts and cash (a shift must be open to take payments; Z-report at
  close is immutable).
- Pricing and tax (via the 001a functions — this spec does not recompute
  totals itself).
- Tenant-currency rule (ADR-017): totals are stored in the tenant's
  single currency; formatting uses the tenant currency.

## Data changes (tables/columns/migrations/indexes; sync impact)
None. All tables land in 001b-1. No migration in this spec. Sync impact:
the outbox row is written in the same transaction as the sale (04 line
38); the server revalidates each synced sale with the same domain crate
(04 line 53).

## Core/API changes (commands, events, errors with codes, permissions)
Internal use-case API only (no Tauri commands — those are 001c). Errors:
`BARCODE_NOT_FOUND`, `SHIFT_NOT_OPEN`, `DISCOUNT_NEEDS_APPROVAL`,
`INSUFFICIENT_TENDER`, `PRINT_QUEUED` (stable codes, mapped to localized
messages at the boundary per 03/09). Permissions: `sale.create`,
`sale.discount.line` (checked by 001b-1's permission checker). Events
emitted: `sale.completed`, `hardware.status` (03 line 71).

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
None.

## Performance (which budgets apply; expected numbers)
- Scan → line < 50 ms; tender → commit < 200 ms; commit → receipt start
  < 500 ms (async); no long tasks > 50 ms (05). Measured against the
  seeded DB from the 002 harness (500k SKUs) in the bench test below.
- The draft flush (req. 2) never runs on the scan path — the scan budget
  is measured with draft persistence ON.
- BENCH: the bench switches to the REAL schema (001b-1's migrations)
  and the throwaway schema is DELETED; `just bench-smoke` is re-run
  against the real schema as part of this spec's verification.

## Security / audit (permissions, audit events)
- Permissions enforced in the use cases (not the UI): `sale.create`,
  `sale.discount.line` (+ manager-PIN approval path, approver ≠ cashier
  unless policy allows, 07 line 9).
- PINs/passwords hashed with Argon2id (001b-1); never logged (07 lines
  11, 26).
- Audit rows per req. 8 with the hash chain from day one; the audit
  table cannot be edited or deleted through any API (07 line 19).

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- Integration (real SQLite, migrations applied, fake clock/ids/hardware):
  sale-transaction atomicity; rollback on injected failure at each step;
  stock balance == ledger after every sequence; audit rows present with
  a valid hash chain; outbox row created in the same transaction;
  aggregates updated; per-component tax rows (`sale_line_tax`) sum to
  the line tax.
- Draft: draft persists and restores after restart; the draft flush does
  NOT run synchronously on the scan path (test asserts the scan path
  never calls a FULL flush — the flush is coalesced/async or on the
  scratch DB); the documented loss window is tested (kill within the
  window → at most the last ~250 ms of draft edits lost, never a
  committed sale).
- Crash: FAULT-INJECTION points (injected failure at each commit step)
  PLUS real process kills at random points during sale commit (and
  during migration); on restart the DB is consistent — one complete sale
  or none, balances match ledger, draft restored. Crash tests RUN ON
  THE WINDOWS CI JOB (not only Linux) — the kill/restart behaviour
  differs by OS and the till is Windows.
- Hardware: `MockPrinter` offline → sale completes, reprint queued
  (`PRINT_QUEUED`); `MockScanner` feeds scans through the mpsc channel.
- Permission: discount above threshold without PIN → rejected + audited;
  with PIN → approved + audited with both users; `SHIFT_NOT_OPEN` when
  no shift is open; `INSUFFICIENT_TENDER` when tender < total.
- Bench: seeded DB (002 harness) on the REAL schema, budgets above
  (scan, tender→commit); `just bench-smoke` re-run after the schema
  switch.
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given an open shift and a known SKU, when I complete a cash sale, then
  one transaction writes the sale, lines, per-component tax rows,
  payment, movements, balances, aggregates, audit, and outbox row — and
  a second read shows them all.
- Given a power cut during completion, when the app restarts, then there
  is either one complete sale or none, and the draft cart is restored if
  no sale committed.
- Given the printer offline, when I complete a sale, then the sale is
  saved, a reprint is queued, and the next sale is not delayed.
- Given a cashier discount limit of 5%, when I apply 10% without a
  manager PIN, then it is rejected and audited; with a PIN, then it is
  approved and audited with both users.
- Given tender 60.00 on a total of 52.32 (cash rounding OFF, the
  default), when I complete, then change 7.68 is recorded and the
  receipt number matches `{branch_code}-{terminal_code}-{yyyymmdd}-{seq}`
  with no rounding line.
- Given a kill during draft editing, when the app restarts, then at most
  the last ~250 ms of draft edits are lost and no committed sale is
  affected.

## Risks / open questions
- The draft loss window (req. 2) is an accepted trade-off — documented
  here and in the session summary, not hidden.
- Receipt layout (RTL bitmap mode) is proven by spec 003, not here —
  this spec queues a `PrintDoc`; the layout renders in 003.

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, budgets pass on the seeded DB on the
REAL schema, crash tests green on the Windows CI job, tracker +
decisions updated, session summary in the standard format.

## Dependencies proposed for approval
None. No new dependency. (argon2 lands in 001b-1; everything else is
already a workspace dependency/member.)
