# 02 — Domain Model and Business Rules

This file is the source of truth for business logic. If a rule is missing, STOP and ask. Every rule here must have tests (golden cases in `tests/golden/`).

## Numeric types (all in `crates/domain`)
- `Money`: `i64` minor units (cents/paisa). Currency exponent comes from a currency table (e.g., 2 for most, 0 for JPY, 3 for KWD).
- `UnitPrice` / `UnitCost`: `i64` fixed-point, scale 10^4 of the major unit (needed for precise weighted-average cost). Convert to `Money` only by the rounding rule below.
- `Qty`: `i64` fixed-point, scale 10^3 (weights, litres). Whole items are multiples of 1000.
- `Rate`: `u32` basis points (1800 = 18.00%).
- **Rounding rule (global):** round half away from zero, applied once per line at defined points. Never round intermediate sums twice.

## Glossary
SKU (sellable variant), Item (parent product), Barcode (many per SKU, one primary), UoM (unit of measure) and pack conversions (1 carton = 12 pieces), Batch (lot with expiry), Price list, Tax group, Promotion, Sale (receipt), Return, Void, Shift (cashier session on a till), Till (terminal), Branch, Hub, Tenant, Z-report (shift/day close), GRN (goods receipt note), PO (purchase order), Stock movement (ledger row), Stocktake.

## Core entities (see `04-data-and-sync.md` for columns)
tenant, branch, terminal, user, role, item, sku, barcode, uom, uom_conversion, category, brand, price_list, price_list_entry, tax_group, tax_rate, promotion, customer, supplier, sale, sale_line, sale_payment, return_doc, shift, cash_movement, purchase_order, goods_receipt, stock_movement, stock_balance, stocktake, transfer, audit_log, outbox, inbox.

## Pricing and tax
Order of operations for each sale line:
1. Start with `unit_price × qty` (from active price list, with promo pricing if applicable) → line gross (rounded to Money).
2. Apply line discount (percent or fixed; cannot exceed line total).
3. Allocate bill-level discount across lines proportionally by post-line-discount amount using **largest remainder** (so allocations sum exactly; ties go to the earlier line).
4. Compute tax on the discounted amount per line.
5. Bill tax = sum of line taxes. Bill total = sum of line totals. No bill-level re-rounding.

Tax modes (per tax group, per tenant default):
- **Exclusive:** `tax = round(net × rate)`, `total = net + tax`.
- **Inclusive:** `net = round(gross / (1 + rate))`, `tax = gross − net` (so net + tax always equals the shelf price).

Golden examples (must exist as tests):
- Exclusive: net 100.00 @18% → tax 18.00, total 118.00.
- Inclusive: gross 118.00 @18% → net 100.00, tax 18.00.
- Inclusive rounding: gross 99.99 @18% → net 84.74, tax 15.25 (99.99/1.18 = 84.7373…).
- Bill discount: lines A 100.00, B 50.00; 10% bill discount = 15.00 → A −10.00, B −5.00; exclusive tax 18% → A tax 16.20, B tax 8.10; net 135.00 + tax 24.30 = total 159.30.
- Largest remainder: discount 1.00 over three equal lines → 0.34, 0.33, 0.33.

Multiple taxes (compound or additive) are configured per tax group; default additive. Tax-exempt customers/items set tax to 0 with an audit-visible reason.

## Promotions (v1)
Types: percent off item/category, fixed amount off, buy X get Y (cheapest or specified free), bundle price, time-windowed, customer-group restricted. Rules:
- Evaluated by the domain crate as pure functions over (cart, promo set, time injected).
- Default: best single promotion per line wins; promotions do not stack unless a promo is flagged `stackable`.
- Promotions apply before manual line discounts; manual discounts require permission and cannot make a line negative.
- Every applied promotion is recorded on the line (promo id, amount) for reporting.

## Sales
- States: `draft → held → completed`, or `voided`. Completed sales are immutable; corrections happen via returns or reversing documents.
- Receipt number: `{branch_code}-{terminal_code}-{yyyymmdd}-{seq}` generated locally; unique offline because terminal code is part of it.
- Payments: cash, card (terminal-handled), wallet, customer credit, other. Split allowed. Change is calculated only on cash. Overpayment on non-cash is rejected.
- Rounding of cash totals to the nearest 0.05 (or currency minimum coin) is a per-tenant setting, recorded as a separate `cash_rounding` line, never hidden in prices.
- Void (before completion) vs refund/return (after completion). Void of a completed sale is not allowed; use return. Voids and price overrides require the corresponding permission or manager PIN.

## Returns and exchanges
- Return references the original sale line where possible (within configurable window); without a receipt requires a permission and a reason.
- Return restores stock via a `return_in` movement (condition: resellable or damaged→write-off movement).
- Refund goes to original payment method by default; refund to store credit allowed. Taxes reverse with the same rates as the original line.
- Exchange = return + new sale in one transaction group with netted payment.

## Shifts and cash
- A cashier must open a shift (opening float counted) to take payments.
- `expected_cash = opening_float + cash_sales − cash_refunds + cash_in − cash_out`.
- `variance = counted_cash − expected_cash`. Variance beyond tolerance requires supervisor sign-off.
- Z-report is generated at shift close and is immutable; reprints are logged.

## Inventory
- Movement types: `receipt`, `sale`, `return_in`, `return_out`, `adjustment_in`, `adjustment_out`, `transfer_out`, `transfer_in`, `stocktake_adjust`, `write_off`, `opening`.
- `stock_balances` is maintained inside the posting function in the same transaction as the movement. Balances are never computed from the ledger at read time, never edited directly, and reconcilable by a verify job (sum of movements must equal balance).
- Negative stock: per-tenant policy, default **allow sale and flag** (shelf stock is often ahead of system stock). Flagged items surface in a "needs attention" list.
- Costing: weighted average by default. On receipt: `new_avg = (on_hand × avg + recv_qty × recv_cost) / (on_hand + recv_qty)`. Example: 10 @ 5.00 + 10 @ 7.00 → 6.00. Sales use the average at time of sale for COGS. FIFO/batch costing is a later option; batches track expiry only in v1.
- Pack/UoM sales convert to base unit at posting time using the conversion in effect.
- Stocktake: freeze snapshot of expected quantity at count start, record counted quantities, post differences as `stocktake_adjust`. Sales during the count are handled by posting adjustments relative to the snapshot, not by blocking the till.

## Purchasing
`PO (draft→approved→partially received→closed)` → `GRN` (posts `receipt` movements, updates cost) → supplier invoice match → supplier payable. Supplier payments reduce payable. Returns to supplier post `return_out`.

## Customers and credit
- Customer account ledger is append-only (charges, payments, adjustments). Balance derived and cached like stock.
- Credit limit per customer; exceeding requires permission.
- Loyalty (v1 basic): points earned = floor(eligible_total × earn_rate); redemption converts points to a discount line; points ledger append-only.

## Reports (definitions)
- Gross sales, discounts, net sales, tax, returns, net after returns; margin = net sales − COGS (using average cost at sale time).
- Stock valuation = on_hand × current average cost per branch.
- All reports read aggregate tables (`daily_sales_summary`, `daily_item_sales`, `daily_cash_summary`) maintained inside the sale transaction; ranges beyond local hot window query the server.

## Invariants (property-tested)
1. For any cart, `sum(line totals) == bill total` and `net + tax == total` per line.
2. `sum(discount allocations) == bill discount` exactly.
3. After any sequence of movements, `balance == sum(movements)` per (branch, sku).
4. Replaying the same sync batch twice produces identical state (idempotency).
5. A completed sale cannot be altered; totals recomputed by the server equal the till's totals.
