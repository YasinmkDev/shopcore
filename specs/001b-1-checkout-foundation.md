# Spec 001b-1 — Checkout foundation: migrations, DB actor, search, users/PIN, hardware traits, dev seed

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 001a (checkout domain), 002 (benchmark harness), 005 (outbox)

## Goal (one sentence, user-visible outcome)
The checkout flow has a real database to stand on: the tables it uses, a
safe way to reach them, users who can sign in with a PIN, and mock
hardware to develop against — with nothing it does not need.

## Users and scenario
A developer building the checkout use cases (001b-2) and the sell screen
(001c) works against real migrations, a real DB actor, and mock hardware —
never against a throwaway schema.

## In scope / Out of scope
In: migrations for ONLY the tables this flow uses (list in Data changes —
`cash_movement`, `uom_conversion`, `brand`, `category` are DROPPED: the
001 flow does not need them); DB actor (one write connection, read pool);
search/lookup queries (`search_skus`, `lookup_barcode`); users/roles/
permission checker + Argon2id PIN hashing (with lockout); hardware traits
(`ReceiptPrinter`, `BarcodeScanner`, `CashDrawer`) + `MockPrinter` /
`MockScanner`; dev seed with the real tables; `sale_line_tax` table for
per-component taxes.
Out: draft cart use cases, discount approval, `complete_sale_cash`,
stock posting, aggregates writing, audit writing, outbox writing,
receipt numbering, print queue, crash tests (all 001b-2); any UI, any
Tauri command (001c); real hardware drivers (specs 003/004); promotions,
split payments, returns, hold/recall, customers, card/wallet.

## Functional requirements (numbered, testable)
1. Migrations (forward-only, numbered; see the ADR in req. 9) create ONLY
   these tables: catalog (`item`, `sku`, `barcode`, `price_list`,
   `price_list_entry`, `tax_group`, `tax_rate`, `uom`); sale (`sale`,
   `sale_line`, `sale_payment`, `sale_line_tax`); stock
   (`stock_movement` append-only, `stock_balance`); aggregates
   (`daily_sales_summary`, `daily_item_sales`, `daily_cash_summary`);
   `shift`; `user`, `role`; `audit_log`; `outbox`; draft
   (`draft_sale`, `draft_line`). Indexes per `04` (`barcode(tenant_id,
   code)` unique, `sku(tenant_id, code)`,
   `sale(branch_id, completed_at)`, `sale_line(sale_id)`,
   `stock_movement(branch_id, sku_id, occurred_at)`,
   `outbox(sent_at) WHERE sent_at IS NULL` partial index) and FTS5 over
   name/code/barcodes (04 line 29 — brand is dropped, so FTS5 covers
   name/code/barcodes only).
2. `sale_line_tax` (NEW): one row per tax component per sale line
   (`sale_line_id`, `tax_rate_id`, `tax_minor`, `rate_bps`) — the
   per-component rounding from 001a req. 8 needs somewhere to live; the
   line's total tax is the sum of its `sale_line_tax` rows.
3. DB actor: one dedicated thread owning the single write connection
   (WAL, `synchronous=FULL`, SQLCipher); a pool of read-only connections
   serves queries. All access goes through the actor — no direct DB
   access from use cases.
4. `search_skus`: barcode exact match = single indexed lookup; name
   search = in-memory prefix index for hot SKUs + FTS5 for the rest
   (05 budgets).
5. `lookup_barcode`: known barcode → SKU + active price; unknown barcode
   → `BARCODE_NOT_FOUND`.
6. Users/roles: `user` (username, Argon2id PIN hash, role, failed
   attempts, lockout state), `role` (permissions). Permission checker:
   `has_permission(user, permission)` for `sale.create`,
   `sale.discount.line` and the rest of the 07 list (the checker is
   generic; the checkout permissions are the first users).
7. PIN hashing: Argon2id via the `argon2` crate (0.6.0, approved
   conditional on `cargo deny` passing in the real workspace — see
   Dependencies). Parameters start from the OWASP minimum (19 MiB
   memory, 2 iterations, 1 lane). Verification runs OFF the UI thread
   (a blocking thread / the DB actor — never the Tauri main thread).
   The measured verification time is recorded in a new ADR (ADR-032)
   during implementation; if it is too slow for the till, the ADR
   records the chosen trade-off instead of silently weakening it.
8. PIN lockout: after repeated failed verifications (threshold per
   07 — lockout after repeated failures), the user is locked out for a
   configured duration; the lockout state lives on the `user` row
   (failed attempts + locked-until). Every lockout is AUDITED — the
   audit row is written by 001b-2's audit writer from the lockout event
   this spec emits (the seam is explicit: 001b-1 emits the event,
   001b-2 persists it).
9. Migrations ADR (ADR-031, recorded in `08-decisions.md` in this
   spec's commit): migrations may be SQUASHED / REWRITTEN until the
   first customer install; after the first customer install they are
   STRICTLY forward-only (never edited, only new numbered migrations).
10. Hardware traits in `crates/hardware` (`ReceiptPrinter`,
    `BarcodeScanner`, `CashDrawer`) with `MockPrinter` / `MockScanner`
    (ADR-030: scanner events use `tokio::sync::mpsc::Receiver<ScanEvent>`,
    not `Stream`). The stale `crates/hardware/src/lib.rs` comment ("traits
    land with the Phase 0 hardware spike specs") is updated — the traits
    land HERE. Real drivers plug in via specs 003/004 with no change to
    this code.
11. Dev seed: a small, realistic dataset using the REAL tables (one
    branch, one terminal, one open shift, a cashier + a manager with
    PINs, a sample catalog with barcodes/prices/tax groups, stock
    balances). The seed is dev/test ONLY: it is `cfg`-gated out of
    release builds, and a test asserts the seed is ABSENT from release
    builds (no seed code, no seed route, no seed data in the shipped
    binary).

## Business rules touched (link to `context/02-domain-model.md` sections)
- Numeric types (via the 001a domain types — this spec stores them, it
  does not recompute them).
- Inventory (the ledger tables; the posting rules land in 001b-2).
- Shifts and cash (the `shift` table; the open-shift gate lands in
  001b-2).
- Tenant-currency rule (ADR-017): totals are stored in the tenant's
  single currency.

## Data changes (tables/columns/migrations/indexes; sync impact)
Migrations per requirement 1 (full column lists in `04-data-and-sync.md`;
`draft_sale` / `draft_line` are defined here — gap fix 1 from the 001
re-check: add them to 04 as part of this spec; `sale_line_tax` is defined
here too). Sync impact: none in this spec — the outbox table is created
here, but outbox WRITING lands in 001b-2 (the schema is the one from
spec 005).

## Core/API changes (commands, events, errors with codes, permissions)
Internal API only (no Tauri commands — those are 001c). Errors:
`BARCODE_NOT_FOUND` (stable code, mapped to a localized message at the
boundary per 03/09). Permissions: the checker from req. 6 (generic;
`sale.create`, `sale.discount.line` are the first users). Events
emitted: a PIN-lockout event (consumed by 001b-2's audit writer).

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
None.

## Performance (which budgets apply; expected numbers)
- Scan → line < 50 ms (the lookup path: barcode exact match = single
  indexed lookup; FTS5 for name search). Measured against the seeded DB
  from the 002 harness (500k SKUs) in the bench test below.
- PIN verification is off the UI thread (req. 7); its measured time is
  recorded in ADR-032.

## Security / audit (permissions, audit events)
- PINs/passwords hashed with Argon2id (argon2 0.6.0); never logged
  (07 lines 11, 26).
- PIN lockout after repeated failures (req. 8); lockout events are
  audited (via 001b-2's writer).
- The `audit_log` table is created here; the tamper-evident hash chain
  (07) is implemented in 001b-2 — from day one, so no audit row is ever
  written without it.

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- Migration tests: migrations apply cleanly on a fresh DB, in order, and
  are idempotent on re-run; the squashed/rewritten rule (ADR-031) is
  documented, not tested.
- DB actor: single write connection enforced; read pool serves queries
  concurrently; a write from outside the actor is impossible (compile /
  API level).
- Search/lookup: barcode exact match returns the SKU in one indexed
  lookup; FTS5 finds by name/code; unknown barcode → `BARCODE_NOT_FOUND`.
- PIN: Argon2id hash verifies a correct PIN and rejects a wrong one;
  verification is off the UI thread (test asserts the call does not run
  on the main thread); lockout after the configured failures, unlock
  after the duration; lockout event emitted.
- Hardware: `MockPrinter` / `MockScanner` implement the traits;
  `MockScanner` feeds scans through the mpsc channel.
- Dev seed: seed applies on a fresh DB and produces a usable till
  (branch, terminal, open shift, users, catalog, stock); a test asserts
  the seed is ABSENT from release builds.
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given a fresh DB, when I apply the migrations, then only the tables
  in req. 1 exist (no `cash_movement`, `uom_conversion`, `brand`,
  `category`).
- Given a known barcode, when I look it up, then I get the SKU + active
  price; given an unknown barcode, then I get `BARCODE_NOT_FOUND`.
- Given a correct PIN, when I verify it, then it passes off the UI
  thread; given repeated wrong PINs, then the user locks out and the
  lockout event is emitted.
- Given a release build, when I inspect it, then no dev-seed code, route,
  or data is present.

## Risks / open questions
- ADR-032 (Argon2id measured verification time) is written during
  implementation — the OWASP minimum may be slow on the weakest PC;
  the measurement decides.
- The lockout audit row depends on 001b-2's audit writer — the seam is
  the lockout event (req. 8).

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, budgets pass on the seeded DB, tracker +
decisions updated (incl. ADR-031 and ADR-032), session summary in the
standard format.

## Dependencies proposed for approval
| Dependency | Version | Why |
|---|---|---|
| `argon2` | 0.6.0 | Argon2id PIN/password hashing, required by `07-security-compliance.md` line 11. Pure Rust, MIT OR Apache-2.0, rust-version 1.85. Default features (alloc, getrandom, password-hash) give the PHC string format. Proof: `cargo info argon2` → 0.6.0; scratch lockfile (17 packages: base64ct 1.8.3, blake2 0.11.0, digest 0.11.3, getrandom 0.4.3, password-hash 0.6.1); `cargo deny check licenses` + `bans` → ok with the project's deny.toml. APPROVED conditional on `cargo deny` passing in the real workspace (this spec's implementation runs it). |
No other new dependency. tokio, rusqlite, `shopcore-domain`/`core`/`db`/
`hardware`/`protocol` are already workspace dependencies/members.
`tokio::sync::mpsc` (already enabled via tokio `sync`) carries scanner
events (ADR-030) — no `futures`/`tokio-stream` dependency.
