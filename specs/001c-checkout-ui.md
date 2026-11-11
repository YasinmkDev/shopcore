# Spec 001c — Checkout UI: Tauri commands, sell screen, e2e

Status: SUPERSEDED (2026-11-10) — split into two sub-specs, each with its
own approval gate and session. Do not implement this file.

- `specs/001c-1-checkout-ui-core.md` — commands, sell screen core,
  status bar.
- `specs/001c-2-checkout-payment-e2e.md` — discount + PIN popover, cash
  payment panel, completion, receipt/print status, e2e.

The original single-spec text is preserved in git history (commit 71b5e78
and earlier). Required changes from the 2026-11-10 review are applied
across the split: unknown barcode shows an inline message with the
scanned code and keeps the cart (NO "Add item" action until the catalog
spec); status bar shows "Sync: not configured" (never a fake synced
state); e2e option A approved with conditions (cargo feature gate, bind
127.0.0.1, random per-run token, ONE shared dispatch table,
release-absence test); `@playwright/test` 1.65.0 approved to add when
001c-2 starts; money formatting from integer minor units with the tenant
currency exponent, never float division.
