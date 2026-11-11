# Spec 001c-1 — Checkout UI core: commands, sell screen, status bar

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 000b (Button/TextField + dark theme + `useMotionTier`), 001a (checkout domain), 001b-1 (foundation), 001b-2 (transaction), 002 (benchmark harness), 005 (outbox)

## Goal (one sentence, user-visible outcome)
A cashier scans, totals, and sees the cart on a fast, keyboard-first sell
screen — with the till's real state (shift, printer, sync) always visible.

## Users and scenario
A cashier on a busy till scans items, sees the cart and total instantly,
and always knows whether the shift is open, the printer is up, and sync
is configured — all by keyboard, all offline.

## In scope / Out of scope
In: Tauri commands (`search_skus`, `lookup_barcode`, `draft_add_line`,
`draft_set_qty`, `draft_remove_line`, `draft_apply_discount`,
`complete_sale_cash`) + events (`sale.completed`, `hardware.status`);
sell screen core (ScanInput, CartTable virtualized, TotalsPanel,
StatusBar); keyboard map per `10`; motion tiers via `useMotionTier`
(000b); money formatting from integer minor units (never float division).
Out: discount + PIN popover, cash payment panel, completion flow,
receipt/print status, e2e (all 001c-2); split payments, returns,
hold/recall, promotions, customers, card/wallet (separate specs — the
PaymentPanel renders Cash only, and it lands in 001c-2); receipt
designer; real hardware drivers (003/004).

## Functional requirements (numbered, testable)
1. Sell screen layout per `10-ui-system.md` line 78–96: left ~2/3 cart,
   right ~1/3 totals and actions; scan input always focused.
2. Scanning a known barcode adds a line (qty 1) or increments the
   existing line; unknown barcode shows an INLINE message with the
   SCANNED CODE ("Barcode 1234567890123 not found") and KEEPS the cart.
   NO "Add item" action until the catalog spec — the message offers no
   catalog action.
3. Typing searches SKUs by name/code/barcode; Enter adds the highlighted
   result.
4. `*` then number sets quantity for the next scan; +/− adjusts the
   selected line; Del removes with undo (Ctrl+Z).
5. Totals (subtotal, discount, tax, total) recompute via the 001a domain
   functions after every change; totals format in the tenant currency
   (ADR-017). Money is formatted from INTEGER minor units with the
   tenant currency exponent — NEVER float division (no
   `minor / 100` in floating point anywhere in the UI).
6. Draft cart restores after crash/restart (001b-2 persists it; this spec
   renders the restored draft).
7. Status bar shows: shift state, printer state, and sync state. Sync
   shows "Sync: not configured" until sync is actually configured —
   NEVER a fake synced state. (The sync client exists from 005, but no
   server is configured for the till yet; the status bar must say so.)
8. All states: loading, empty cart, error, offline (status bar shows
   printer/shift/sync state). All user-visible strings via i18n keys.
9. Motion: sell-screen exceptions only (cart row insert 100 ms opacity +
   4 px translate; total changes 250 ms background tint fade), gated on
   the `useMotionTier` tier — tier 2 = opacity-only, ≤ 100 ms. Nothing
   else animates in checkout.
10. Keyboard map per `10` (F6 discount, Ctrl+Z undo, Esc cancel/close,
    Enter confirm, ↑/↓ cart selection, +/− qty, Del remove, `*` set
    quantity). F9 (pay) opens the payment panel — the panel itself lands
    in 001c-2; in 001c-1, F9 is wired but shows the panel as "coming in
    001c-2" only if the panel is absent (preferably: 001c-1 ships the
    command wiring and the screen, and F9 is enabled once 001c-2 lands).
    Shortcut hints visible in buttons and a bottom strip.

## Business rules touched (link to `context/02-domain-model.md` sections)
- Sales (receipt number format — displayed, not generated here; cash
  rounding OFF by default, so the totals shown are exact to the minor
  unit).
- Pricing and tax (via the 001a functions — the UI never computes totals
  and never divides money as a float).
- Shifts and cash (a shift must be open; the UI shows the shift state).
- Tenant-currency rule (ADR-017): totals format in the tenant currency,
  from integer minor units.

## Data changes (tables/columns/migrations/indexes; sync impact)
None. All tables land in 001b-1. No migration in this spec.

## Core/API changes (commands, events, errors with codes, permissions)
Commands (typed, versioned, validated in Rust per 03): `search_skus`,
`lookup_barcode`, `draft_add_line`, `draft_set_qty`, `draft_remove_line`,
`draft_apply_discount`, `complete_sale_cash` (the command exists and is
wired; the completion FLOW — payment panel, receipt status — lands in
001c-2). Events (core → UI): `sale.completed`, `hardware.status`
(03 line 71). Error codes (stable, mapped to localized messages):
`BARCODE_NOT_FOUND`, `SHIFT_NOT_OPEN`, `DISCOUNT_NEEDS_APPROVAL`,
`INSUFFICIENT_TENDER`, `PRINT_QUEUED`. Permissions enforced in 001b;
the UI only renders the outcomes. `just gen-types` output must stay
fresh (ADR-013 drift-guard test covers command ↔ invoke parity).

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
Screens: sell screen (core only — no payment panel yet). Components
(registry first, then sell-screen-specific in `apps/ui`): `Button`,
`TextField` (000b), `ScanInput`, `CartTable` (virtualized via
`@tanstack/react-virtual`, already present), `CartRow`, `TotalsPanel`,
`StatusBar`, `StatusPill`. States per requirement 8. Keyboard per
requirement 10. Motion per requirement 9 (`useMotionTier` from 000b).

## Performance (which budgets apply; expected numbers)
- Scan → line < 50 ms; focus return < 100 ms (05); no long tasks
  > 50 ms; sell-screen JS < 300 kB gzip (05).
- Measured with the 002 harness seeded DB.

## Security / audit (permissions, audit events)
None new — permissions and audit rows are enforced/written in 001b. The
UI never decides a permission; it renders the 001b outcomes.

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- UI component tests (Vitest): every sell-screen component in all states
  (loading, empty cart, error, offline) + keyboard (`*`, +/−, Del,
  Ctrl+Z, Enter, Esc, ↑/↓) + RTL + 150% text scaling.
- Money formatting tests: integer minor units → display string for the
  tenant currency exponent (incl. exponent 0 and 3 currencies); no float
  division anywhere (a test greps / asserts the formatter never uses
  floating-point arithmetic on money).
- Unknown barcode: inline message shows the scanned code, cart is kept,
  and NO "Add item" action is rendered.
- Status bar: shows "Sync: not configured" when no server is configured
  (never a fake synced state); shift/printer states render from events.
- Tauri command tests: each command round-trips through `TauriClient`
  with the real core (in-process); ADR-013 drift-guard proves command ↔
  invoke parity.
- Bench: seeded DB, budgets above (scan, focus return).
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given an open shift and a known SKU, when I scan its barcode twice,
  then one line with qty 2 shows within 50 ms of each scan.
- Given an unknown barcode 1234567890123, when I scan it, then an inline
  message shows "Barcode 1234567890123 not found", the cart is unchanged,
  and no "Add item" action appears.
- Given no sync server configured, when I look at the status bar, then it
  reads "Sync: not configured".
- Given a total of 52.32 (cash rounding OFF, the default), when I view
  the totals, then 52.32 shows exactly (no float artefacts, no rounding
  line).
- Given a power cut during completion, when the app restarts, then there
  is either one complete sale or none, and the draft cart is restored if
  no sale committed.

## Risks / open questions
- F9 (pay) is wired in 001c-1 but the payment panel lands in 001c-2 —
  the spec states the seam (req. 10); the implementer must not ship a
  dead F9 key that does nothing silently.
- Receipt layout (RTL bitmap mode) is proven by spec 003; 001c renders
  the `ReceiptPreview` from the queued `PrintDoc` only (001c-2).

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, budgets pass on the seeded DB, UI matches
`10-ui-system.md` (all states, keyboard-operable, i18n keys only, AA
contrast, RTL checked), tracker + decisions updated, session summary in
the standard format.

## Dependencies proposed for approval
None. No new dependency. `@tanstack/react-virtual` 3.14.26 is already in
`apps/ui`.
