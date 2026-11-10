# Spec 001c — Checkout UI: Tauri commands, sell screen, e2e

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 000b (Button/TextField + dark theme + `useMotionTier`), 001a (checkout domain), 001b (checkout core), 002 (benchmark harness), 005 (outbox)

## Goal (one sentence, user-visible outcome)
A cashier scans, totals, discounts, and takes cash on a fast, keyboard-first
sell screen — and the whole flow is proven end to end against the real core.

## Users and scenario
A cashier on a busy till scans items, sees the cart and total instantly,
takes cash with change, and a receipt prints — all by keyboard, all offline.

## In scope / Out of scope
In: Tauri commands (`search_skus`, `lookup_barcode`, `draft_add_line`,
`draft_set_qty`, `draft_remove_line`, `draft_apply_discount`,
`complete_sale_cash`) + events (`sale.completed`, `hardware.status`);
sell screen (ScanInput, CartTable virtualized, TotalsPanel, PaymentPanel
cash-only, NumPad, ManagerPinPopover, StatusBar); keyboard map per `10`;
motion tiers via `useMotionTier` (000b); Playwright e2e flow against the
real core (see "E2E approach" below — Playwright itself is NOT added in
this spec; the approach is proposed for approval).
Out: split payments, returns, hold/recall, promotions, customers,
card/wallet (separate specs — the PaymentPanel renders Cash only in this
spec; Card/Wallet/Credit buttons and the split-payment list from
`10-ui-system.md` arrive with their specs), receipt designer, real
hardware drivers (003/004).

## Functional requirements (numbered, testable)
1. Sell screen layout per `10-ui-system.md` line 78–96: left ~2/3 cart,
   right ~1/3 totals and actions; scan input always focused; payment
   opens as a side panel (not a modal).
2. Scanning a known barcode adds a line (qty 1) or increments the
   existing line; unknown barcode shows an inline "Add item?" action and
   keeps the cart (`BARCODE_NOT_FOUND` from 001b).
3. Typing searches SKUs by name/code/barcode; Enter adds the highlighted
   result.
4. `*` then number sets quantity for the next scan; +/− adjusts the
   selected line; Del removes with undo (Ctrl+Z).
5. Totals (subtotal, discount, tax, total) recompute via the 001a domain
   functions after every change; totals format in the tenant currency
   (ADR-017 — gap fix 5).
6. Line discount percent/fixed; above the cashier's threshold the manager
   PIN popover appears (`DISCOUNT_NEEDS_APPROVAL`); approval is audited
   with both users (001b).
7. F9 opens the payment panel; cash tender with quick amounts; change
   computed live; Enter completes. PaymentPanel renders Cash only in this
   spec (gap fix 4).
8. Completion calls `complete_sale_cash`; on success the cart clears,
   focus returns to the scan input within 100 ms, and the receipt prints
   (queued if the printer is offline — banner "Printer offline — receipt
   queued", next sale not delayed).
9. Draft cart restores after crash/restart (001b persists it; this spec
   renders the restored draft).
10. All states: loading, empty cart, error, offline (status bar shows
    printer/shift/sync state). All user-visible strings via i18n keys.
11. Motion: sell-screen exceptions only (cart row insert 100 ms opacity +
    4 px translate; total changes 250 ms background tint fade; payment
    panel 150 ms slide), gated on the `useMotionTier` tier — tier 2 =
    opacity-only, ≤ 100 ms. Nothing else animates in checkout.
12. Keyboard map per `10` (F9 pay, F6 discount, Ctrl+Z undo, Esc
    cancel/close, Enter confirm, ↑/↓ cart selection, +/− qty, Del remove,
    `*` set quantity). Shortcut hints visible in buttons and a bottom
    strip.

## E2E approach (proposed — Playwright NOT added in this spec)
The e2e flow (scan → qty → discount (PIN) → pay → receipt) must run
against the REAL core (real SQLite, real use cases), not a mocked UI.
Options:
- **A (recommended, user preference): thin dev-only HTTP shim over the
  real core.** A `dev-e2e` feature on the desktop binary (or a tiny
  `shopcore-e2e` binary) that exposes the same use cases over HTTP on
  localhost, only in dev builds (`cfg(debug_assertions)` + an env flag;
  never compiled into release). Playwright drives the browser build of
  the UI pointed at the shim; the UI uses the `CoreClient` HTTP
  transport (03 — the UI is unaware of deployment mode, so the same code
  runs against the shim and the real Tauri app). Pros: tests the real
  core end to end, deterministic, fast. Cons: a dev-only code path that
  must be proven absent from release builds (a test asserts the shim
  route 404s / the feature is off in release).
- **B: Playwright against the real Tauri app via `tauri-driver` /
  WebDriver.** 11-test-strategy line 12 already names this for a smaller
  smoke suite on Windows CI. Pros: tests the exact shipped app. Cons:
  slower, flakier, needs a display/driver on CI, harder to control the
  DB state.
- **C: Playwright against the browser build with a mocked core.**
  Rejected — it would not test the real sale transaction, which is the
  point of the e2e.
Proposal: approve A for the 001c e2e (with the release-build exclusion
test), keep B for the smaller Windows smoke suite (11 line 12). Playwright
(`@playwright/test` 1.65.0, proof in the approval reply) is added when
001c is approved — not now.

## Business rules touched (link to `context/02-domain-model.md` sections)
- Sales (receipt number format, cash rounding as a separate line, change
  on cash only, void vs return — void/return themselves are later specs).
- Pricing and tax (via the 001a functions — the UI never computes totals).
- Shifts and cash (a shift must be open; the UI shows the shift state
  and blocks payment with `SHIFT_NOT_OPEN` when closed).
- Tenant-currency rule (ADR-017): totals format in the tenant currency.

## Data changes (tables/columns/migrations/indexes; sync impact)
None. All tables land in 001b. No migration in this spec.

## Core/API changes (commands, events, errors with codes, permissions)
Commands (typed, versioned, validated in Rust per 03): `search_skus`,
`lookup_barcode`, `draft_add_line`, `draft_set_qty`, `draft_remove_line`,
`draft_apply_discount`, `complete_sale_cash`. Events (core → UI):
`sale.completed`, `hardware.status` (03 line 71). Error codes (stable,
mapped to localized messages): `BARCODE_NOT_FOUND`, `SHIFT_NOT_OPEN`,
`DISCOUNT_NEEDS_APPROVAL`, `INSUFFICIENT_TENDER`, `PRINT_QUEUED`.
Permissions enforced in 001b; the UI only renders the outcomes.
`just gen-types` output must stay fresh (ADR-013 drift-guard test
covers command ↔ invoke parity).

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
Screens: sell screen only. Components (registry first, then
sell-screen-specific in `apps/ui`): `Button`, `TextField` (000b),
`ScanInput`, `CartTable` (virtualized via `@tanstack/react-virtual`,
already present), `CartRow`, `TotalsPanel`, `PaymentPanel` (cash only),
`NumPad`, `ManagerPinPopover`, `StatusBar`, `StatusPill`. States per
requirement 10. Keyboard per requirement 12. Motion per requirement 11
(`useMotionTier` from 000b).

## Performance (which budgets apply; expected numbers)
- Scan → line < 50 ms; tender → commit < 200 ms; commit → receipt start
  < 500 ms (async); focus return < 100 ms (05 — gap fix 3 adds this row
  to 05; it already lives in 10); no long tasks > 50 ms; sell-screen JS
  < 300 kB gzip (05).
- Measured with the 002 harness seeded DB + the Playwright e2e (option A)
  timing the flow.

## Security / audit (permissions, audit events)
None new — permissions and audit rows are enforced/written in 001b. The
UI never decides a permission; it renders the 001b outcomes. Manager PIN
entry uses the popover (07 line 9); PINs are never logged or displayed.

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- UI component tests (Vitest): every sell-screen component in all states
  (loading, empty cart, error, offline) + keyboard (F9, `*`, +/−, Del,
  Ctrl+Z, Enter, Esc, ↑/↓) + RTL + 150% text scaling.
- Tauri command tests: each command round-trips through `TauriClient`
  with the real core (in-process); ADR-013 drift-guard proves command ↔
  invoke parity.
- E2E (option A, after approval): Playwright flow — scan → qty →
  discount (PIN) → pay → receipt — against the dev-only HTTP shim over
  the real core; asserts the sale, the change, the receipt queue, and
  the cleared cart with focus on the scan input. Plus a release-build
  test proving the shim is absent from release.
- Bench: seeded DB, budgets above (scan, tender→commit, focus return).
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given an open shift and a known SKU, when I scan its barcode twice,
  then one line with qty 2 shows within 50 ms of each scan.
- Given a total of 52.32 and tender 60.00, when I press Enter, then
  change 7.68 shows, the sale is committed, the receipt prints, and the
  cart is empty with focus on the scan input.
- Given the printer is offline, when I complete a sale, then the sale is
  saved, a "Printer offline — receipt queued" banner appears, and the
  next sale is not delayed.
- Given a power cut during completion, when the app restarts, then there
  is either one complete sale or none, and the draft cart is restored if
  no sale committed.
- Given a cashier discount limit of 5%, when I apply 10%, then a manager
  PIN prompt appears and the approval is audited with both users.

## Risks / open questions
- E2E approach: option A (dev-only HTTP shim) vs B (tauri-driver) —
  proposed A, awaiting approval. Playwright is NOT added until 001c is
  approved.
- Receipt layout (RTL bitmap mode) is proven by spec 003; 001c renders
  the `ReceiptPreview` from the queued `PrintDoc` only.

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, budgets pass on the seeded DB, UI matches
`10-ui-system.md` (all states, keyboard-operable, i18n keys only, AA
contrast, RTL checked), tracker + decisions updated, session summary in
the standard format.

## Dependencies proposed for approval
| Dependency | Version | Why |
|---|---|---|
| `@playwright/test` | 1.65.0 | E2E flow test (option A shim). NOT added in this turn — added when 001c is approved. Proof: `npm view @playwright/test version` → 1.65.0; engines node >=20 (pinned Node 22.23.4 satisfies). |
No other new dependency. `@tanstack/react-virtual` 3.14.26 is already in
`apps/ui`. The dev-only HTTP shim is a `cfg(debug_assertions)` feature on
the existing desktop binary (or a tiny new binary reusing `shopcore-core`)
— no new external crate.
