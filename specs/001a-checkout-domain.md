# Spec 001a — Checkout domain: Qty, Rate, pricing, tax, discounts

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), 005 (`Money` in `crates/domain`)

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
4. Rounding rule (global, from `02`): round half away from zero, applied
   once per line at defined points. Never round intermediate sums twice.
5. Line pricing follows the `02` order of operations exactly: (1) `unit_price
   × qty` → line gross (rounded to Money); (2) line discount (percent or
   fixed; cannot exceed line total); (3) bill-discount allocation across
   lines proportionally by post-line-discount amount using largest
   remainder (allocations sum exactly; ties go to the earlier line);
   (4) tax on the discounted amount per line; (5) bill tax = sum of line
   taxes, bill total = sum of line totals, no bill-level re-rounding.
6. Tax exclusive: `tax = round(net × rate)`, `total = net + tax`.
7. Tax inclusive: `net = round(gross / (1 + rate))`, `tax = gross − net`
   (net + tax always equals the shelf price).
8. Multiple taxes per tax group are additive by default (compound is a
   per-group configuration, not implemented here unless the group says so).
   Tax-exempt lines set tax to 0 with a reason carried on the line.
9. Cash rounding: the total is rounded to the nearest 0.05 (or the
   currency minimum coin) per tenant setting, recorded as a separate
   `cash_rounding` line, never hidden in prices.
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
sub-millisecond per cart (asserted by a bench test on a 200-line cart).

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
  (1.00 over three equal lines → 0.34, 0.33, 0.33).
- Property: totals invariants over random carts with a fixed-seed LCG
  (same pattern as `Money` — NO proptest dependency): line/bill totals
  consistent, discount allocations sum exactly, tax inclusive/exclusive
  round-trips, `balance == sum(movements)` is NOT here (that's 001b),
  change ≥ 0 when tender ≥ total, rounding is idempotent.
- Unit: each requirement above (Qty/Rate construction + overflow,
  rounding half-away-from-zero at .5 boundaries, discount-cannot-exceed,
  receipt-number format + sequence rollover at day boundary).
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
