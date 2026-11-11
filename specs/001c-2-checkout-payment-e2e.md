# Spec 001c-2 — Checkout UI: discount/PIN, cash payment, completion, receipt status, e2e

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 000b (Button/TextField + dark theme + `useMotionTier`), 001a (checkout domain), 001b-1 (foundation), 001b-2 (transaction), 001c-1 (UI core), 002 (benchmark harness), 005 (outbox)

## Amendments (approved 2026-11-10, applied in this spec)
1. E2E option A with all five conditions stands. The DESKTOP crate
   (`crates/desktop`) serves HTTP under the `e2e-shim` cargo feature —
   axum is added as an OPTIONAL dependency of the desktop crate,
   enabled only by that feature. The implementation proves with
   `cargo tree` that the RELEASE dependency graph excludes axum (and
   the shim). The `HttpClient` transport does NOT exist yet — the class
   exists in `apps/ui/src/lib/core-client.ts` but is an unimplemented
   stub (throws "not implemented yet (hub spec, Phase 5)"). 001c-2
   builds the HTTP transport for the commands the e2e flow uses, over
   the SHARED dispatch table (no second implementation) — and ONLY that
   much: build only the transport the e2e flow needs, shaped so Phase 5
   can extend it (same interface, same error mapping, no e2e-only
   shortcuts baked into the transport). The RELEASE UI bundle must NOT
   contain shim URLs or token handling — a build-time flag strips them
   (the e2e transport code is excluded from the release bundle, not
   just disabled at runtime); a test GREPS the release UI bundle for
   the shim URL/token markers and fails if found. The implementation
   also runs `cargo deny` AND `cargo tree` for BOTH feature sets
   (default/release AND `e2e-shim`) and records both outputs.

## Goal (one sentence, user-visible outcome)
A cashier discounts (with a manager PIN when needed), takes cash with
change, and gets a receipt — and the whole flow is proven end to end
against the real core.

## Users and scenario
A cashier on a busy till applies a discount, takes cash, sees the change,
and a receipt prints — all by keyboard, all offline, all proven by an
automated end-to-end test.

## In scope / Out of scope
In: discount + PIN popover (`ManagerPinPopover`); cash payment panel
(`PaymentPanel`, cash only); completion flow; receipt/print status;
Playwright e2e flow against the real core (option A, approved with
conditions — see "E2E approach"); money formatting from integer minor
units (payment panel).
Out: split payments, returns, hold/recall, promotions, customers,
card/wallet (separate specs — the PaymentPanel renders Cash only; the
Card/Wallet/Credit buttons and split-payment list from
`10-ui-system.md` arrive with their specs); receipt designer; real
hardware drivers (003/004).

## Functional requirements (numbered, testable)
1. Line discount percent/fixed; above the cashier's threshold the manager
   PIN popover appears (`DISCOUNT_NEEDS_APPROVAL`); approval is audited
   with both users (001b-2).
2. F9 opens the payment panel as a side panel (not a modal) per `10`;
   cash tender with quick amounts; change computed live from integer
   minor units (never float division); Enter completes. PaymentPanel
   renders Cash only in this spec.
3. Completion calls `complete_sale_cash`; on success the cart clears,
   focus returns to the scan input within 100 ms, and the receipt prints
   (queued if the printer is offline — banner "Printer offline — receipt
   queued", next sale not delayed).
4. Receipt/print status: after completion the UI shows the receipt state
   (printed / queued / failed) and offers reprint (reprint is logged,
   07).
5. Cash rounding is OFF by default (001a req. 9): the acceptance example
   is total 52.32, tender 60.00, change 7.68 with no rounding line. When
   a tenant turns rounding ON, the rounding shows as a separate untaxed
   line in the payment panel.
6. All states: loading, empty cart, error, offline. All user-visible
   strings via i18n keys.
7. Motion: payment panel = 150 ms slide (the sell-screen exception in
   `10`), gated on the `useMotionTier` tier — tier 2 = opacity-only,
   ≤ 100 ms.
8. Keyboard: F9 pay, F6 discount, Ctrl+Z undo, Esc cancel/close, Enter
   confirm. Shortcut hints visible in buttons and a bottom strip.

## E2E approach (APPROVED — option A, with conditions)
The e2e flow (scan → qty → discount (PIN) → pay → receipt) runs against
the REAL core (real SQLite, real use cases), not a mocked UI, via a thin
dev-only HTTP shim over the real core. Conditions (all required):
- **Cargo feature gate** (not only `debug_assertions` + an env flag):
  the shim is behind a Cargo feature (e.g. `e2e-shim`), enabled only in
  dev/test builds. The release profile never enables it. The shim lives
  in the DESKTOP crate (`crates/desktop`) — axum is added as an OPTIONAL
  dependency of the desktop crate, enabled only by the `e2e-shim`
  feature. The implementation proves with `cargo tree` that the RELEASE
  dependency graph excludes axum (and the shim).
- **Bind 127.0.0.1 only** — the shim never listens on a LAN interface.
- **Random per-run token**: the shim requires a token generated fresh
  per run (passed to the test runner via env); requests without it are
  rejected.
- **ONE dispatch table**: the Tauri commands and the shim share a single
  dispatch table — the shim calls the same use-case functions the Tauri
  commands call (no second implementation of any command).
- **Release-absence test**: a test asserts the shim is absent from
  release builds (no shim route, no shim feature, no token handling in
  the shipped binary).
Playwright (`@playwright/test` 1.65.0) is approved to add WHEN 001c-2
STARTS (not before). The e2e drives the browser build of the UI pointed
at the shim; the UI uses the `CoreClient` HTTP transport (03 — the UI is
unaware of deployment mode, so the same code runs against the shim and
the real Tauri app). The `HttpClient` transport does NOT exist yet — the
class exists in `apps/ui/src/lib/core-client.ts` but is an unimplemented
stub (throws "not implemented yet (hub spec, Phase 5)"). 001c-2 builds
the HTTP transport for the commands the e2e flow uses, over the shared
dispatch table (no second implementation) — and ONLY that much: build
only the transport the e2e flow needs, shaped so Phase 5 can extend it
(same interface, same error mapping, no e2e-only shortcuts baked into
the transport). The RELEASE UI bundle must NOT contain shim URLs or
token handling — a build-time flag strips them (the e2e transport code
is excluded from the release bundle, not just disabled at runtime); a
test GREPS the release UI bundle for the shim URL/token markers and
fails if found. The implementation also runs `cargo deny` AND
`cargo tree` for BOTH feature sets (default/release AND `e2e-shim`)
and records both outputs.

## Business rules touched (link to `context/02-domain-model.md` sections)
- Sales (receipt number format, cash rounding OFF by default / ON as a
  separate untaxed line, change on cash only, void vs return — void/return
  themselves are later specs).
- Pricing and tax (via the 001a functions — the UI never computes totals
  and never divides money as a float).
- Shifts and cash (a shift must be open; the UI blocks payment with
  `SHIFT_NOT_OPEN` when closed).
- Tenant-currency rule (ADR-017): totals format in the tenant currency,
  from integer minor units.

## Data changes (tables/columns/migrations/indexes; sync impact)
None. All tables land in 001b-1. No migration in this spec.

## Core/API changes (commands, events, errors with codes, permissions)
New commands in THIS spec (moved from 001c-1 — the ADR-013 drift guard
requires a UI caller for every command, and these two get their callers
here): `draft_apply_discount` (called by the ManagerPinPopover) and
`complete_sale_cash` (called by the PaymentPanel). The e2e shim exposes
the SAME commands through the shared dispatch table (see E2E approach)
— no second API surface. Error codes: `BARCODE_NOT_FOUND`,
`SHIFT_NOT_OPEN`, `DISCOUNT_NEEDS_APPROVAL`, `INSUFFICIENT_TENDER`,
`PRINT_QUEUED`. Permissions enforced in 001b; the UI only renders the
outcomes. `just gen-types` output must stay fresh (ADR-013 drift-guard
test covers command ↔ invoke parity).

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
Screens: sell screen (payment panel + completion). Components:
`PaymentPanel` (cash only), `NumPad`, `ManagerPinPopover`, plus the
001c-1 components. States per requirement 6. Keyboard per requirement 8.
Motion per requirement 7 (`useMotionTier` from 000b).

## Performance (which budgets apply; expected numbers)
- Scan → line < 50 ms; tender → commit < 200 ms; commit → receipt start
  < 500 ms (async); focus return < 100 ms (05); no long tasks > 50 ms;
  sell-screen JS < 300 kB gzip (05).
- Measured with the 002 harness seeded DB + the Playwright e2e (option A)
  timing the flow.

## Security / audit (permissions, audit events)
None new — permissions and audit rows are enforced/written in 001b. The
UI never decides a permission; it renders the 001b outcomes. Manager PIN
entry uses the popover (07 line 9); PINs are never logged or displayed.
The e2e shim is bound to 127.0.0.1 with a per-run token and is absent
from release builds (see E2E approach).

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- UI component tests (Vitest): PaymentPanel, NumPad, ManagerPinPopover in
  all states + keyboard (F9, F6, Enter, Esc) + RTL + 150% text scaling.
- Money formatting tests: payment-panel amounts from integer minor units
  with the tenant currency exponent (never float division).
- E2E (option A, after approval): Playwright flow — scan → qty →
  discount (PIN) → pay → receipt — against the dev-only HTTP shim over
  the real core; asserts the sale, the change (52.32 / 60.00 / 7.68),
  the receipt queue, and the cleared cart with focus on the scan input.
  Plus the release-absence test (shim absent from release builds) and a
  test that the shim rejects requests without the per-run token. Plus a
  test that GREPS the release UI bundle for shim URLs / token handling
  and fails if found (the build-time flag strips them — the e2e
  transport code is excluded from the release bundle, not just disabled
  at runtime). `cargo deny` AND `cargo tree` are run and recorded for
  BOTH feature sets (default/release AND `e2e-shim`).
- Bench: seeded DB, budgets above (scan, tender→commit, focus return).
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given a total of 52.32 and tender 60.00 (cash rounding OFF, the
  default), when I press Enter, then change 7.68 shows, the sale is
  committed, the receipt prints, and the cart is empty with focus on the
  scan input.
- Given the printer is offline, when I complete a sale, then the sale is
  saved, a "Printer offline — receipt queued" banner appears, and the
  next sale is not delayed.
- Given a cashier discount limit of 5%, when I apply 10%, then a manager
  PIN prompt appears and the approval is audited with both users.
- Given a power cut during completion, when the app restarts, then there
  is either one complete sale or none, and the draft cart is restored if
  no sale committed.
- Given a release build, when I probe for the e2e shim, then no shim
  route, feature, or token handling exists.

## Risks / open questions
- Receipt layout (RTL bitmap mode) is proven by spec 003; 001c renders
  the `ReceiptPreview` from the queued `PrintDoc` only.
- The e2e shim is a dev-only code path — the release-absence test and the
  single dispatch table are what keep it from becoming a second API.

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, budgets pass on the seeded DB, UI matches
`10-ui-system.md` (all states, keyboard-operable, i18n keys only, AA
contrast, RTL checked), tracker + decisions updated, session summary in
the standard format.

## Dependencies proposed for approval
| Dependency | Version | Why |
|---|---|---|
| `@playwright/test` | 1.65.0 | E2E flow test (option A shim). APPROVED to add when 001c-2 starts (not before). Proof: `npm view @playwright/test version` → 1.65.0; engines node >=20 (pinned Node 22.23.4 satisfies). |
No other new dependency. The dev-only HTTP shim reuses the existing
Axum/server stack patterns (no new external crate — the shim is a
`cfg(feature = "e2e-shim")` route on the same dispatch table).
