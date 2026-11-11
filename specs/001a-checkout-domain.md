# Spec 001a — Checkout domain: Qty, Rate, pricing, tax, discounts

Status: Approved (with the six amendments below, 2026-11-10)
Phase: 1   Depends on: 000 (scaffold), 005 (`Money` in `crates/domain`)

## Amendments (approved 2026-11-10, applied in this spec)
1. i128 intermediates for `price_e4 × qty_milli` with checked conversion
   back; overflow tested (req. 3).
2. Multiple taxes: round EACH tax component separately (req. 8; golden
   net 0.30 with 5% + 13% → 0.06).
3. Inclusive tax + discount: the discounted gross is the gross for the
   inclusive formula (req. 7; golden 118.00 @18% + 10% bill discount →
   discounted gross 106.20, net 90.00, tax 16.20).
4. Cash rounding DEFAULT OFF (granularity = minor unit); when ON it
   applies only to the cash-tendered total after tax, as a separate
   untaxed line (req. 9; goldens 52.32 → 52.30, 52.33 → 52.35,
   52.35 @0.10 → 52.40). Acceptance examples use the default
   (52.32 − 60.00 → change 7.68).
5. Percent bill discount = percent of the subtotal after line discounts,
   rounded once, then largest-remainder allocated (req. 5); zero-value
   lines and all-zero carts tested.
6. LCG property tests: ≥ 10,000 cases, failing case prints its seed;
   the 200-line bench uses a loose/relative threshold (Tests,
   Performance).

## Goal (one sentence, user-visible outcome)
The checkout math is correct and provable: every cart total, tax, discount, rounding, and change figure comes from pure, tested domain functions.

## Users and scenario
A cashier scans items and the totals on screen are always right; a developer
calling the domain crate gets the same numbers the server revalidation gets.

## In scope / Out of scope
In: `Qty` (i64 fixed-point, scale 10^3), `Rate` (u32 basis points), `UnitPrice`
/ `UnitCost` (i64 fixed-point, scale 10^4), line pricing, tax (exclusive and
inclusive, per tax group), line discounts (percent/fixed), bill-discount
allocation (largest remainder), cash rounding (per-tenant setting), change
calculation (cash only), receipt-number generation
(`{branch_code}-{terminal_code}-{yyyymmdd}-{seq}`), cart-total invariants.
Out: promotions (separate spec — 001's scope excludes them; the `02`
Promotions section is not implemented here), split payments, returns,
hold/recall, customers, card/wallet, any UI, any DB, any network, any clock
(time is injected).

## Functional requirements (numbered, testable)
1. `Qty`: i64 milli-units (scale 10^3); whole items are multiples of 1000;
   arithmetic is checked (overflow is an error, never a wrap); negative
   quantities are rejected at construction.
2. `Rate`: u32 basis points (1800 = 18.00%); range 0–10000.
3. `UnitPrice` / `UnitCost`: i64 fixed-point, scale 10^4 of the major unit;
   conversion to `Money` happens only via the rounding rule (req. 4).
   The multiplication `price_e4 × qty_milli` uses an **i128
   intermediate** (price_e4 up to i64::MAX, qty_milli up to i64::MAX —
   the i64 product would overflow); the result is converted back to
   `Money` with a CHECKED conversion (overflow is an error, never a
   wrap or silent clamp). Test the overflow boundary.
4. Rounding rule (global, from `02`): round half away from zero, applied
   once per line at defined points. Never round intermediate sums twice.
5. Line pricing follows the `02` order of operations exactly: (1) `unit_price
   × qty` → line gross (rounded to Money, via the i128 intermediate of
   req. 3); (2) line discount (percent or fixed; cannot exceed line
   total); (3) bill-discount allocation: a PERCENT bill discount is a
   percent of the subtotal AFTER line discounts, rounded ONCE, then
   allocated across lines proportionally by post-line-discount amount
   using largest remainder (allocations sum exactly; ties go to the
   earlier line); (4) tax on the discounted amount per line; (5) bill
   tax = sum of line taxes, bill total = sum of line totals, no
   bill-level re-rounding.
6. Tax exclusive: `tax = round(net × rate)`, `total = net + tax`.
7. Tax inclusive: `net = round(gross / (1 + rate))`, `tax = gross − net`
   (net + tax always equals the shelf price). When a discount applies to
   an inclusive-tax line, the DISCOUNTED GROSS is the gross for this
   formula: `net = round(discounted_gross / (1 + rate))`,
   `tax = discounted_gross − net`.
8. Multiple taxes per tax group are additive by default (compound is a
   per-group configuration, not implemented here unless the group says so).
   EACH tax component is rounded SEPARATELY (`tax_i = round(net ×
   rate_i)`), and the line tax is the sum of the rounded components.
   (Rounding the combined rate instead gives a different, wrong answer:
   net 0.30 with 5% + 13% → 0.02 + 0.04 = 0.06, vs 0.05 if the 18%
   were combined first.) Tax-exempt lines set tax to 0 with a reason
   carried on the line.
9. Cash rounding: DEFAULT OFF — the granularity is the minor unit
   (no rounding). When a tenant turns it ON, the total is rounded to
   the nearest granularity (e.g. 0.05, or the currency minimum coin) and
   the rounding applies ONLY to the cash-tendered total AFTER tax, as a
   separate UNTAXED line, never hidden in prices. Acceptance examples
   use the default (rounding OFF).
10. Change: `change = tender − total` (cash only); tender below total is an
    error (`INSUFFICIENT_TENDER`); overpayment on non-cash is rejected
    (not implemented here — non-cash is out of scope, but the rule is
    recorded for the payment spec).
11. Receipt numbers are generated locally as
    `{branch_code}-{terminal_code}-{yyyymmdd}-{seq}`; unique offline
    because the terminal code is part of it. The sequence is per terminal
    per day; the generator takes the last sequence as an injected input
    (no DB, no clock — the date is injected).
12. Every public function is pure: no I/O, no clock, no randomness, no DB,
    no network. Time, randomness, and IDs are injected via traits defined
    in `crates/domain` (AGENTS.md purity invariant).

## Business rules touched (link to `context/02-domain-model.md` sections)
- Numeric types (`Money`, `Qty`, `Rate`, `UnitPrice`/`UnitCost`, rounding
  rule); tenant-currency rule (set at setup, immutable after the first
  transaction; formatting uses the tenant currency; `Money` stays
  currency-agnostic — ADR-017).
- Pricing and tax (order of operations, exclusive/inclusive, golden
  examples, largest remainder, multiple taxes, tax-exempt).
- Sales (receipt number, cash rounding, change on cash only, void vs
  return — the state machine itself lands in 001b; this spec provides the
  pure total/receipt-number functions only).

## Data changes (tables/columns/migrations/indexes; sync impact)
None. No migration. Pure domain code only.

## Core/API changes (commands, events, errors with codes, permissions)
None. No Tauri command, no IPC surface. Error type: a `DomainError` enum
in `crates/domain` (overflow, negative quantity, discount exceeds line,
tender below total, invalid rate); `just gen-types` output unchanged.

## UI (screens, components from registry, states, keyboard shortcuts, motion tier behavior)
None.

## Performance (which budgets apply; expected numbers)
Pure functions; no hot-path concern beyond the checkout budgets they feed:
scan → line < 50 ms and tender → commit < 200 ms (measured in 001b/001c
against the seeded DB). The pricing/tax computation itself must be
sub-millisecond per cart (asserted by a bench test on a 200-line cart
with a loose/relative threshold — see Tests).

## Security / audit (permissions, audit events)
None. Discount permission-gating (`sale.discount.line`, manager PIN) is
enforced in 001b/001c, not in the pure functions — the functions take the
discount as an input and do not decide who may apply it.

## Tests (list exactly what will be added: unit, property, integration, e2e, golden)
- Golden: `tests/golden/*.json` with EVERY worked example in `02`:
  exclusive 100.00 @18% → tax 18.00, total 118.00; inclusive 118.00 @18%
  → net 100.00, tax 18.00; inclusive rounding 99.99 @18% → net 84.74,
  tax 15.25; bill discount (A 100.00, B 50.00, 10% → A −10.00, B −5.00,
  tax 16.20/8.10, net 135.00 + tax 24.30 = 159.30); largest remainder
  (1.00 over three equal lines → 0.34, 0.33, 0.33). PLUS the amended
  goldens: per-component tax rounding — net 0.30 with 5% + 13% → tax
  0.06 (0.02 + 0.04), NOT 0.05; inclusive tax + bill discount — gross
  118.00 @18% inclusive with 10% bill discount → discounted gross
  106.20, net 90.00, tax 16.20; cash rounding ON — 52.32 → 52.30,
  52.33 → 52.35 (granularity 0.05), 52.35 → 52.40 (granularity 0.10).
- Property: totals invariants over random carts with a fixed-seed LCG
  (same pattern as `Money` — NO proptest dependency), at LEAST 10,000
  cases, and a failing case prints its seed so it can be reproduced:
  line/bill totals consistent, discount allocations sum exactly, tax
  inclusive/exclusive round-trips, `balance == sum(movements)` is NOT
  here (that's 001b), change ≥ 0 when tender ≥ total, rounding is
  idempotent. Edge cases covered: zero-value lines, all-zero carts
  (bill discount on an all-zero cart allocates 0, never divides by
  zero), and the i128 overflow boundary (req. 3).
- Unit: each requirement above (Qty/Rate construction + overflow,
  rounding half-away-from-zero at .5 boundaries, discount-cannot-exceed,
  receipt-number format + sequence rollover at day boundary, i128
  intermediate overflow boundary, cash rounding OFF default + ON
  behaviour, percent bill discount = percent of subtotal after line
  discounts rounded once).
- Bench: 200-line cart pricing/tax computation. The threshold is LOOSE /
  RELATIVE (not a tight absolute number): it asserts the computation
  stays sub-millisecond-order per cart and does not regress vs the
  stored baseline by more than 5× (the same relative-tripwire pattern as
  spec 002), so it does not flake on a slow CI runner.
- `just check` green (fmt, clippy -D warnings, deny, nextest, tsc,
  eslint, vitest).

## Acceptance criteria (Given/When/Then)
- Given the golden dataset, when I run the domain tests, then every
  worked example in `02` reproduces exactly.
- Given a random cart (fixed seed), when I compute totals, then line
  totals sum to the bill total, discount allocations sum exactly, and
  inclusive tax never changes the shelf price.
- Given a line discount larger than the line total, when I apply it, then
  it is rejected (never negative).
- Given tender below total, when I compute change, then it errors
  (`INSUFFICIENT_TENDER`); given tender above total, then change is
  exact to the minor unit.
- Given cash rounding OFF (the default), when I compute a total of
  52.32 with tender 60.00, then change is 7.68 with no rounding line.
  Given cash rounding ON at 0.05, when I compute 52.32, then the total
  is 52.30; 52.33 → 52.35; given granularity 0.10, 52.35 → 52.40 — each
  as a separate untaxed line.
- Given a bill discount on a cart with zero-value lines, when I allocate
  it, then zero lines get 0 and the allocation still sums exactly; given
  an all-zero cart, when I apply a percent bill discount, then the
  discount is 0 (no division by zero).

## Risks / open questions
- Promotions are out of scope here but `02` defines them — the promotion
  spec comes later; 001a must not bake in promotion hooks that constrain
  it.
- Cash-rounding granularity (0.05 vs currency minimum coin) is a
  per-tenant setting; the setting store lands in a later spec — 001a
  takes the granularity as an injected parameter.

## Definition of done
Standard checklist in `AGENTS.md` + anything specific here: tests added
and passing, `just check` green, tracker + decisions updated, session
summary in the standard format.
