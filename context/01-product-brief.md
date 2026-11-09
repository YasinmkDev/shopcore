# 01 — Product Brief

## What we are building
A fast, offline-first desktop POS and back-office system for retail: single shops up to mega marts (10–30 tills per store, multiple branches). It must match everything established retail software offers, with a faster, calmer, easier-to-learn UX.

## Users and what they need
| Persona | Needs | Priority UX trait |
|---|---|---|
| Cashier | Sell fast: scan, pay, print, next customer. Zero training for basics | Speed, keyboard-first, never blocked |
| Supervisor / store manager | Approvals (void, discount, price override), cash-up, stock fixes | Fast PIN approval, clear exceptions |
| Inventory clerk | Receive goods, stocktake, transfers, labels | Bulk entry, spreadsheet-like grids |
| Owner / head office | Sales, margin, stock value, cashier and branch performance, remote view | Clear reports, trust in numbers |
| Accountant | Tax reports, ledgers, exports | Exactness, export to Excel/PDF |

## Customer segments
1. Single shop: one till, embedded mode, no server.
2. Multi-branch small chain: several stores, each one or few tills, cloud sync.
3. Mega mart: 10–30 tills in one store behind a store hub, plus cloud for head office.

## Scope v1 (must ship; parity with existing apps)
- Checkout: barcode/search/quick tiles, hold/recall, split payments (cash/card/wallet/credit), line + bill discounts, returns/exchanges, voids with approval, reprint, weighted items, price check.
- Catalog: variants, multiple barcodes per item, units and pack conversions, categories, brands, batch/expiry, label printing, bulk import/export (Excel/CSV).
- Pricing: price lists, tax groups (inclusive/exclusive), promotions (percent, fixed, buy X get Y, bundle).
- Inventory: stock ledger, adjustments, stocktake, low-stock alerts, branch transfers.
- Purchasing: purchase orders, goods receipt, supplier ledger and payables.
- Customers: records, credit ledger (customer account/khata-style), basic loyalty points.
- Cash: shift open/close, cash in/out, Z-report, expected vs counted.
- Security: roles/permissions, manager override by PIN, audit log, lock screen.
- Reports: daily sales, profit/margin, stock valuation, item/category/cashier performance, tax report, export Excel/PDF.
- Operations: offline mode, multi-branch sync, backup/restore, receipt/invoice designer, multi-language incl. RTL, multi-currency.
- **Migration importer** (products, customers, opening balances, supplier balances). This is the #1 adoption blocker; it ships in v1.

## v2 (do not build early)
Advanced promotions engine, online/e-commerce order sync, auto-reorder suggestions, mobile owner dashboard, fiscal/e-invoicing integrations per market (research early, build later), accounting exports, scale-embedded barcodes.

## Non-goals (v1)
Restaurant/table service, manufacturing, payroll, full accounting ledger, mobile POS, marketplace features.

## Success metrics
- Scan to line on screen < 50 ms; full sale (scan to receipt print start) < 1 s after tender.
- Zero lost sales: app keeps selling with no internet and survives power loss mid-sale.
- A new cashier completes a basic sale within 5 minutes of first use.
- Data import of a 50k-item catalog < 2 minutes.
- Sync convergence: tills and cloud agree on totals (verified by server revalidation using the same domain crate).

## Open items (fill when known)
- Target markets and their tax/fiscal rules: __TBD__ (affects `07-security-compliance`).
- Weakest supported PC: ASSUMED 4 GB RAM, Windows 10, SSD (see `08-decisions` ADR-011).
- Currencies and languages for launch: __TBD__.
