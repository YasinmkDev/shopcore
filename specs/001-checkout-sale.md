# Spec 001 — Checkout: scan → cart → cash sale → receipt

Status: Draft (needs human approval before implementation)
Phase: 1   Depends on: 000 (scaffold), Phase 0 (002, 003, 004, 005) — blocked until Phase 0 passes

## Goal
A cashier scans items, sees the cart and total instantly, takes cash payment with change, and a receipt prints, entirely offline.

## Scope
In: scan/search add, qty change, remove line, line discount (permission-gated), cash tender with change, completion, receipt print (queued), cart persistence across crash, sell screen UI per `10-ui-system`.
Out: split payments, returns, hold/recall, promotions, customers, card/wallet (separate specs).

## Functional requirements
1. Scanning a known barcode adds a line (qty 1) or increments existing line; unknown barcode shows inline "Add item?" action and keeps the cart.
2. Typing searches SKUs by name/code/barcode; Enter adds the highlighted result.
3. `*` then number sets quantity for the next scan; +/− adjusts selected line; Del removes with undo (Ctrl+Z).
4. Totals (subtotal, discount, tax, total) recompute via the `domain` crate after every change.
5. Line discount percent/fixed; above the cashier's threshold requires manager PIN (audited).
6. F9 opens the payment panel; cash tender with quick amounts; change computed live; Enter completes.
7. Completion writes ONE transaction (sale, lines, payment, stock movements, balances, aggregates, audit, outbox); then prints receipt and kicks drawer asynchronously.
8. Draft cart is persisted in core on every change; after crash/restart the cart is restored.
9. Print failure never blocks or rolls back; a reprint task is queued and the status bar shows the problem.
10. Focus returns to the scan input within 100 ms after completion.

## Business rules
`02-domain-model`: Pricing and tax (order of operations, inclusive/exclusive), Sales (receipt number, states), Inventory (sale movement + balance), Shifts (a shift must be open to take cash).

## Data changes
Tables: `sale`, `sale_line`, `sale_payment`, `stock_movement`, `stock_balance`, `daily_sales_summary`, `daily_item_sales`, `daily_cash_summary`, `audit_log`, `outbox`, draft cart tables (`draft_sale`, `draft_line`). Migrations + indexes per `04`. Receipt number generator per terminal.

## Core/API
Commands: `search_skus`, `lookup_barcode`, `draft_add_line`, `draft_set_qty`, `draft_remove_line`, `draft_apply_discount`, `complete_sale_cash`. Events: `sale.completed`, `hardware.status`. Error codes: `BARCODE_NOT_FOUND`, `SHIFT_NOT_OPEN`, `DISCOUNT_NEEDS_APPROVAL`, `INSUFFICIENT_TENDER`, `PRINT_QUEUED`. Permissions: `sale.create`, `sale.discount.line`.

## UI
Sell screen layout, ScanInput, CartTable (virtualized), TotalsPanel, PaymentPanel (side panel), NumPad, ManagerPinPopover, StatusBar. All states (loading, empty cart, error, offline). Keyboard map per `10`. Motion: sell-screen exceptions only.

## Performance
Scan → line < 50 ms; tender → commit < 200 ms; receipt starts < 500 ms; focus return < 100 ms; no long tasks > 50 ms.

## Tests
- Golden: tax inclusive/exclusive and discount examples from `02`.
- Property: totals invariants over random carts.
- Integration: sale transaction atomicity, rollback on injected failure, stock balance == ledger, audit rows, outbox row created.
- Crash: kill during commit → consistent state; draft restored.
- Hardware: mock printer offline → sale completes, reprint queued.
- UI: component tests for states + keyboard; Playwright flow: scan → qty → discount (PIN) → pay → receipt.
- Bench: seeded DB, budgets above.

## Acceptance (Given/When/Then)
- Given an open shift and a known SKU, when I scan its barcode twice, then one line with qty 2 shows within 50 ms of each scan.
- Given a total of 52.32 and tender 60.00, when I press Enter, then change 7.68 shows, the sale is committed, the receipt prints, and the cart is empty with focus on the scan input.
- Given the printer is offline, when I complete a sale, then the sale is saved, a "Printer offline — receipt queued" banner appears, and the next sale is not delayed.
- Given a power cut during completion, when the app restarts, then there is either one complete sale or none, and the draft cart is restored if no sale committed.
- Given a cashier discount limit of 5%, when I apply 10%, then a manager PIN prompt appears and the approval is audited with both users.

## Risks / open questions
Scanner discrimination under fast typing (Phase 0 spike). Receipt layout for RTL (bitmap mode). Cash rounding setting default per market.
