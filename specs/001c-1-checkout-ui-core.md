# Spec 001c-1 — Checkout UI core: commands, sell screen, status bar

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 000b (Button/TextField + dark theme + `useMotionTier`), 001a (checkout domain), 001b-1 (foundation), 001b-2 (transaction), 002 (benchmark harness), 005 (outbox)

## Amendments (approved 2026-11-10, applied in this spec)
1. `draft_apply_discount` and `complete_sale_cash` commands move to
   001c-2 (the ADR-013 drift guard requires a UI caller for every
   command — those two have no UI caller until the discount popover and
   the payment panel land).
2. F9 is NOT bound in 001c-1 (it opens the payment panel, which lands
   in 001c-2).
3. IPC money convention: DTO integers travel as JSON numbers, Rust
   range-checks them to ≤ 2^53-1, ts-rs types them as `number`; the UI
   formats with integer arithmetic only. Same for `qty_milli`. Tests
   for both.
4. A `SessionProvider` abstraction supplies the actor (user, terminal,
   shift) to the sell screen; a feature-gated dev auto-session fills it
   in dev. Real sign-in/lock is a new spec, 001d.
5. `ScanInput` = a plain focused input + Enter (burst detection arrives
   with spec 004). Search results: a simple list of ≤ 20.
6. "Cancel sale" (with confirm) + Ctrl+Z undo, wired to `draft_cancel`
   / `draft_restore_line` (001b-2).
7. Accessibility: a live region announces the scanned item and the new
   total; tested.
8. Graceful shutdown (owner decision 2026-11-12, ADR-036): the app
   holds ONE `Core` (`shopcore_core::Core`, owning the `Db` and the
   `ScratchDb`) in Tauri managed state, and the Tauri exit event
   MUST call `Core::shutdown()` before the process exits — the
   actor and timer threads are joined (5 s timeout) and the
   connections closed, so the process cannot segfault during
   teardown. A test asserts the exit path calls `shutdown` (the
   desktop crate exposes the shutdown for the test, or the test
   drives the `Core` directly).

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
2. `ScanInput` is a plain focused input + Enter (burst detection —
   distinguishing a fast scanner burst from typed input — arrives with
   spec 004). Scanning a known barcode adds a line (qty 1) or increments
   the existing line; unknown barcode shows an INLINE message with the
   SCANNED CODE ("Barcode 1234567890123 not found") and KEEPS the cart.
   NO "Add item" action until the catalog spec — the message offers no
   catalog action.
3. Typing searches SKUs by name/code/barcode; Enter adds the highlighted
   result. Search results render as a simple list of ≤ 20 (no
   virtualization, no grouping — the virtualized search list is a later
   spec if the 20-row list ever proves too slow).
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
10. Keyboard map per `10` (Ctrl+Z undo, Esc cancel/close, Enter
    confirm, ↑/↓ cart selection, +/− qty, Del remove, `*` set
    quantity). F9 is NOT bound in 001c-1 (it opens the payment panel,
    which lands in 001c-2 — a dead F9 key that does nothing is worse
    than no F9 key). F6 (discount) is also unbound until 001c-2 (the
    discount popover lands there). Shortcut hints visible in buttons
    and a bottom strip.
11. "Cancel sale" button (with a confirm step) discards the draft via
    `draft_cancel` (001b-2 — writes an audit row if the cart had
    lines). Ctrl+Z is wired to `draft_restore_line` (001b-2 —
    single-level undo of a removal).
12. A `SessionProvider` abstraction supplies the actor (user, terminal,
    shift) to the sell screen. In dev, a feature-gated dev auto-session
    fills it (no sign-in screen in dev). Real sign-in/lock is a NEW
    spec, 001d — this spec does not build it.
13. Accessibility: a live region (`aria-live="polite"`) announces the
    scanned item ("Added 2 × Milk 1L") and the new total after every
    scan/add/remove ("Total 52.32"). Tested with a screen-reader-style
    assertion on the live region's text.

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
`draft_cancel`, `draft_restore_line`. `draft_apply_discount` and
`complete_sale_cash` MOVE to 001c-2 — the ADR-013 drift guard requires
a UI caller for every command, and those two have no UI caller until
the discount popover and the payment panel land (001c-2). Events
(core → UI): `sale.completed`, `hardware.status` (03 line 71). Error
codes (stable, mapped to localized messages): `BARCODE_NOT_FOUND`,
`SHIFT_NOT_OPEN`, `DISCOUNT_NEEDS_APPROVAL`, `INSUFFICIENT_TENDER`,
`PRINT_QUEUED`. Permissions enforced in 001b; the UI only renders the
outcomes. `just gen-types` output must stay fresh (ADR-013 drift-guard
test covers command ↔ invoke parity).

IPC money convention: DTO integers travel as JSON NUMBERS (not strings);
Rust range-checks every integer DTO to ≤ 2^53-1 (the safe-integer limit
for JSON numbers in JavaScript) and rejects anything larger; ts-rs
types them as `number`. The UI formats money with INTEGER arithmetic
only (never float division — no `minor / 100` in floating point
anywhere). The same convention applies to `qty_milli` (integer
milli-units as a JSON number, range-checked the same way). Tests: a
Rust test sends 2^53 and 2^53+1 and asserts accept/reject; a TS test
asserts the formatter never produces a float artifact (e.g. 0.1 + 0.2
style rounding) for any minor-unit input.

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
  (loading, empty cart, error, offline) + keyboard (Ctrl+Z, Enter, Esc,
  ↑/↓, +/−, Del, `*`) + RTL + 150% text scaling. F9 and F6 are NOT
  bound (a test asserts pressing F9/F6 does nothing in 001c-1).
- Money formatting tests: integer minor units → display string for the
  tenant currency exponent (incl. exponent 0 and 3 currencies); no float
  division anywhere (a test greps / asserts the formatter never uses
  floating-point arithmetic on money). IPC convention tests: Rust
  accepts 2^53 and rejects 2^53+1 for every integer DTO (money AND
  `qty_milli`); ts-rs types them as `number`.
- Unknown barcode: inline message shows the scanned code, cart is kept,
  and NO "Add item" action is rendered.
- Status bar: shows "Sync: not configured" when no server is configured
  (never a fake synced state); shift/printer states render from events.
- Cancel sale: the confirm step discards the draft (audit row written
  when the cart had lines); Ctrl+Z restores the most recently removed
  line once.
- Live region: after a scan, the live region's text contains the scanned
  item and the new total (asserted in a component test).
- Tauri command tests: each command round-trips through `TauriClient`
  with the real core (in-process); ADR-013 drift-guard proves command ↔
  invoke parity. `draft_apply_discount` and `complete_sale_cash` have
  NO command in 001c-1 (they land in 001c-2) — the drift guard must not
  see them here.
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
