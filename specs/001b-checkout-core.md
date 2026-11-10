# Spec 001b — Checkout core: migrations, DB actor, use cases, single-transaction sale

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 001a (checkout domain), 002 (benchmark harness), 005 (outbox)

## Goal (one sentence, user-visible outcome)
A cash sale is one atomic database transaction: it either completes fully
(sale, lines, payment, stock, balances, aggregates, audit, outbox) or not
at all, and it survives a power cut mid-commit.

## Users and scenario
A cashier completes a cash sale; the till never loses it, double-counts it,
or leaves stock wrong — even if the power dies during the commit.

## In scope / Out of scope
In: migrations for all checkout tables (catalog, sale, stock, shift, user,
audit, outbox, draft cart); DB actor (one write connection, read pool);
use cases (`search_skus`, `lookup_barcode`, `draft_add_line`,
`draft_set_qty`, `draft_remove_line`, `draft_apply_discount`,
`complete_sale_cash`); single-transaction sale commit; draft-cart
persistence and restore; stock posting (append-only movements, balances
only via the posting function); aggregates update; audit rows; outbox row
in the same transaction; shift-open gate; permission checks
(`sale.create`, `sale.discount.line`) with manager-PIN approval path;
Argon2id PIN hashing (see Dependencies); receipt-number sequence per
terminal; dev seed with the real tables (see Prerequisite table — catalog,
shift, users, terminal/branch identity).
Out: any UI, any Tauri command, any hardware driver (traits + mocks only —
the hardware crate's traits land here, mocks included; real drivers are
specs 003/004), promotions, split payments, returns, hold/recall,
customers, card/wallet, sync worker (005 already built the client; this
spec only writes the outbox row).

## Functional requirements (numbered, testable)
1. Migrations (forward-only, numbered, never edited after shipping) create:
   catalog (`item`, `sku`, `barcode`, `price_list`, `price_list_entry`,
   `tax_group`, `tax_rate`, `category`, `brand`, `uom`,
   `uom_conversion`), sale (`sale`, `sale_line`, `sale_payment`),
   stock (`stock_movement` append-only, `stock_balance`),
   aggregates (`daily_sales_summary`, `daily_item_sales`,
   `daily_cash_summary`), `shift`, `cash_movement`, `user`, `role`,
   `audit_log`, `outbox`, draft cart (`draft_sale`, `draft_line`) —
   with the indexes in `04` (`barcode(tenant_id, code)` unique,
   `sku(tenant_id, code)`, `sale(branch_id, completed_at)`,
   `sale_line(sale_id)`, `stock_movement(branch_id, sku_id, occurred_at)`,
   `outbox(sent_at) WHERE sent_at IS NULL` partial index) and FTS5 over
   name/code/barcodes/brand (04 line 29).
2. DB actor: one dedicated thread owning the single write connection
   (WAL, `synchronous=FULL`, SQLCipher); a pool of read-only connections
   serves queries. All access goes through the actor — no direct DB
   access from use cases.
3. `search_skus`: barcode exact match = single indexed lookup; name
   search = in-memory prefix index for hot SKUs + FTS5 for the rest
   (05 budgets).
4. `lookup_barcode`: known barcode → SKU + active price; unknown barcode
   → `BARCODE_NOT_FOUND` (the UI offers "Add item?" — that action is
   001c).
5. Draft cart: every change persists the draft (`draft_sale` /
   `draft_line`) in core; after crash/restart the draft is restored.
6. `complete_sale_cash`: ONE SQLite transaction writing sale, lines,
   payment, stock movements, balances, aggregates, audit, and the outbox
   row. Totals come from the 001a domain functions. Stock balances are
   written ONLY by the posting function (never directly). The sale
   commits only if a shift is open (`SHIFT_NOT_OPEN` otherwise) and the
   caller holds `sale.create`.
7. Line discount above the cashier's threshold requires manager PIN
   (approval recorded with both users, audited); without approval →
   `DISCOUNT_NEEDS_APPROVAL`. PINs are verified against Argon2id hashes
   (07 line 11); never logged.
8. Cash tender below total → `INSUFFICIENT_TENDER`; change computed by
   the 001a function; cash rounding recorded as a separate line.
9. Receipt number is generated per terminal from the 001a function with
   the last sequence read from the DB (injected into the pure function).
10. Print/drawer actions go through the `ReceiptPrinter` / `CashDrawer`
    traits with `MockPrinter` in this spec; the sale completes and the
    receipt queues even if the printer is offline (reprint queued,
    `PRINT_QUEUED`); hardware never blocks the commit. Real drivers plug
    in via specs 003/004 with no change to this code (ADR-030: scanner
    events use `tokio::sync::mpsc`, not `Stream`).
11. Audit rows are written for: sale completion, discount, price
    override, drawer open, shift open/close, reprint. Each row: who,
    what, entity, before/after, reason, terminal, time (07).
12. Crash safety: kill during commit → on restart there is either one
    complete sale or none (never partial), balances match the ledger,
    and the draft is restored if no sale committed.

## Business rules touched (link to `context/02-domain-model.md` sections)
- Sales (states `draft → held → completed` / `voided`; completed sales
  immutable; receipt number; cash rounding; void vs return).
- Inventory (sale movement + balance; `balance == sum(movements)` per
  branch+sku; append-only ledger).
- Shifts and cash (a shift must be open to take payments; Z-report at
  close is immutable).
- Pricing and tax (via the 001a functions — this spec does not recompute
  totals itself).
- Tenant-currency rule (ADR-017): totals are stored in the tenant's
  single currency; formatting uses the tenant currency.

## Data changes (tables/columns/migrations/indexes; sync impact)
Migrations per requirement 1 (full column lists in `04-data-and-sync.md`;
`draft_sale` / `draft_line` are defined here — see gap fix 1: add them to
04 as part of this spec). Sync impact: every local mutation writes an
outbox row in the same transaction (04 line 38); the outbox schema is the
one from spec 005 (`sync-client` helpers `ensure_outbox_schema` /
`mark_sent`); the server revalidates each synced sale with the same
domain crate (04 line 53).

## Core/API changes (commands, events, errors with codes, permissions)
Internal use-case API only (no Tauri commands — those are 001c). Errors:
`BARCODE_NOT_FOUND`, `SHIFT_NOT_OPEN`, `DISCOUNT_NEEDS_APPROVAL`,
`INSUFFICIENT_TENDER`, `PRINT_QUEUED` (stable codes, mapped to localized
messages at the boundary per 03/09). Permissions: `sale.create`,
`sale.discount.line`. Events emitted: `sale.completed`,
`hardware.status` (03 line 71).

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
None.

## Performance (which budgets apply; expected numbers)
- Scan → line < 50 ms; tender → commit < 200 ms; commit → receipt start
  < 500 ms (async); no long tasks > 50 ms (05). Measured against the
  seeded DB from the 002 harness (500k SKUs) in the bench test below.
- Search: barcode exact match = single indexed lookup; FTS5 for the rest.

## Security / audit (permissions, audit events)
- Permissions enforced in the use cases (not the UI): `sale.create`,
  `sale.discount.line` (+ manager-PIN approval path, approver ≠ cashier
  unless policy allows, 07 line 9).
- PINs/passwords hashed with Argon2id (argon2 0.6.0, default features);
  never logged (07 line 11, 07 line 26).
- Audit rows per requirement 11; the audit table cannot be edited or
  deleted through any API (07 line 19).

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- Integration (real SQLite, migrations applied, fake clock/ids/hardware):
  sale-transaction atomicity; rollback on injected failure at each step;
  stock balance == ledger after every sequence; audit rows present;
  outbox row created in the same transaction; aggregates updated.
- Crash: kill the process at random points during sale commit (and
  during migration); on restart the DB is consistent — one complete sale
  or none, balances match ledger, draft restored (11 lines 9/26).
- Hardware: `MockPrinter` offline → sale completes, reprint queued
  (`PRINT_QUEUED`); `MockScanner` feeds scans through the mpsc channel.
- Permission: discount above threshold without PIN → rejected + audited;
  with PIN → approved + audited with both users; `SHIFT_NOT_OPEN` when
  no shift is open; `INSUFFICIENT_TENDER` when tender < total.
- Bench: seeded DB (002 harness), budgets above (scan, tender→commit).
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given an open shift and a known SKU, when I complete a cash sale, then
  one transaction writes the sale, lines, payment, movements, balances,
  aggregates, audit, and outbox row — and a second read shows them all.
- Given a power cut during completion, when the app restarts, then there
  is either one complete sale or none, and the draft cart is restored if
  no sale committed.
- Given the printer offline, when I complete a sale, then the sale is
  saved, a reprint is queued, and the next sale is not delayed.
- Given a cashier discount limit of 5%, when I apply 10% without a
  manager PIN, then it is rejected and audited; with a PIN, then it is
  approved and audited with both users.
- Given tender 60.00 on a total of 52.32, when I complete, then change
  7.68 is recorded and the receipt number matches
  `{branch_code}-{terminal_code}-{yyyymmdd}-{seq}`.

## Risks / open questions
- Catalog/shift/user/terminal seed: the dev seed (real tables) is part of
  this spec — see the Prerequisite table in the approval reply. Real
  catalog import is a later spec (migration importer); the seed is for
  development and tests only.
- Receipt layout (RTL bitmap mode) is proven by spec 003, not here —
  this spec queues a `PrintDoc`; the layout renders in 003.

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, budgets pass on the seeded DB, tracker +
decisions updated, session summary in the standard format.

## Dependencies proposed for approval
| Dependency | Version | Why |
|---|---|---|
| `argon2` | 0.6.0 | Argon2id PIN/password hashing, required by `07-security-compliance.md` line 11 ("PINs/passwords hashed with Argon2id; never logged"). Pure Rust, MIT OR Apache-2.0, rust-version 1.85. Default features (alloc, getrandom, password-hash) give the PHC string format. Proof: `cargo info argon2` → 0.6.0; scratch lockfile (17 packages: base64ct 1.8.3, blake2 0.11.0, digest 0.11.3, getrandom 0.4.3, password-hash 0.6.1); `cargo deny check licenses` + `bans` → ok with the project's deny.toml. |
No other new dependency. tokio, rusqlite, `shopcore-domain`/`core`/`db`/
`hardware`/`protocol` are already workspace dependencies/members.
`tokio::sync::mpsc` (already enabled via tokio `sync`) carries scanner
events (ADR-030) — no `futures`/`tokio-stream` dependency.
