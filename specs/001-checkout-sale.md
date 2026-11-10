# Spec 001 — Checkout: scan → cart → cash sale → receipt

Status: SUPERSEDED (2026-11-10) — split into three sub-specs, each with its
own approval gate and session. Do not implement this file.

- `specs/001a-checkout-domain.md` — pure domain (Qty, Rate, pricing, tax,
  discounts, golden + property tests, no UI/DB).
- `specs/001b-checkout-core.md` — migrations, DB actor, use cases,
  single-transaction sale, crash tests.
- `specs/001c-checkout-ui.md` — Tauri commands, sell screen, e2e.

The original single-spec text is preserved in git history (commit b037ed4
and earlier). All five gap fixes from the 001 re-check are applied across
the split (draft tables → 04 via 001b; hardware traits named + ADR-030;
focus budget → 05; cash-only PaymentPanel note in 001c; tenant-currency
formatting note in 001c).
