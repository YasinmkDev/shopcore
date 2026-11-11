# Spec 001b — Checkout core: migrations, DB actor, use cases, single-transaction sale

Status: SUPERSEDED (2026-11-10) — split into two sub-specs, each with its
own approval gate and session. Do not implement this file.

- `specs/001b-1-checkout-foundation.md` — migrations for only the tables
  used by 001, DB actor, search/lookup, users/roles/permission checker +
  PIN hashing, hardware traits + mocks, dev seed.
- `specs/001b-2-checkout-transaction.md` — draft cart, discount approval,
  `complete_sale_cash` transaction, stock posting, aggregates, audit,
  outbox, receipt numbering, print queue, crash tests.

The original single-spec text is preserved in git history (commit 71b5e78
and earlier). Required changes from the 2026-11-10 review are applied
across the split: draft persistence never puts a synchronous=FULL flush
on the scan path (coalesced async flush ~250 ms or a scratch DB;
documented loss window); audit_log hash chain from day one; PIN lockout
(audited); Argon2id OWASP-minimum params measured + ADR; `sale_line_tax`
table; bench switches to the real schema (throwaway deleted, bench-smoke
re-run); ADR-031 (migrations squashable until first customer install,
then strictly forward-only); only tables this flow uses (cash_movement,
uom_conversion, brand, category dropped); dev-seed-absent-from-release
test; crash tests use fault injection + real kills on the Windows CI job.
