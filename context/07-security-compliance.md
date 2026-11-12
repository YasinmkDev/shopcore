# 07 — Security, Roles, Audit, Compliance

## Threat model (short)
Shared physical machines, cashiers with limited trust, power loss, stolen PCs, tampering with sales data (cash skimming), malware on the till, leaked backups, malicious updates. Design so a stolen disk yields nothing useful and a dishonest cashier cannot erase their tracks.

## Authentication
- Users sign in with username + PIN or password; PIN length ≥ 4, lockout after repeated failures, lock screen after idle (configurable).
- Fast user switching on the till without closing the shift.
- Manager override: inline PIN popover that records who approved what (approver ≠ cashier unless policy allows).
- Cloud/head office: email + password with MFA; sessions with short-lived tokens; device registration for tills.
- PINs/passwords hashed with Argon2id; never logged.

## Authorization (RBAC)
Roles: Cashier, Supervisor, Store Manager, Inventory Clerk, Accountant, Owner/Admin. Permissions are fine-grained, stored in DB, checked in the **core** (never only in UI). Examples:
`sale.create`, `sale.discount.line`, `sale.discount.bill`, `sale.price_override`, `sale.void_line`, `return.create`, `return.no_receipt`, `shift.close`, `cash.drawer_open`, `stock.adjust`, `stock.stocktake.approve`, `catalog.edit`, `price.edit`, `report.view.branch`, `report.view.all`, `user.manage`, `settings.edit`, `backup.restore`.
Per-role thresholds (e.g., cashier line discount ≤ 5%, supervisor ≤ 20%) enforced in core; above threshold requires approval.

## Audit log
Append-only, tamper-evident (hash chain: each row stores hash of previous row). Required events: login/logout/failed login, shift open/close, void, return, discount, price override, drawer open, stock adjustment, price/catalog edit, user/role change, settings change, backup/restore, reprint, report export, sync conflicts. Each row: who, what, entity, before/after, reason (if required), terminal, time. The audit table cannot be edited or deleted through any API.

## Data protection
- Local DB encrypted with SQLCipher. The key is generated per install and sealed with Windows DPAPI (machine scope for hub/embedded); recovery key shown once to the owner.
- Backups encrypted with a key derived from an owner passphrase + stored recovery key.
- TLS 1.2+ for all network traffic; hub LAN traffic also TLS (self-signed per-store CA, pinned on tills) or at minimum a per-store shared secret + signed tokens.
- Secrets never in source, config files, logs, or UI bundles.
- Logs contain no PII, no payment data, no PINs.

### Key management (current state, 2026-11-11)
- Tests: `Db::open` takes `key: Option<&[u8]>` — the key is passed in by the caller. Tests use a hardcoded test key (`b"test-key-32-bytes-long-0123456789"`, `crates/db/tests/actor.rs`); the seed test uses `Db::open_in_memory()` (no key — unencrypted in-memory DB). No test/dev key is a real key.
- Dev: no key — no production/dev caller of `Db::open` exists outside tests yet; no key wiring exists. The dev DB is unencrypted.
- Production: the key is generated per install and sealed with Windows DPAPI, with a recovery key shown once to the owner (see above) — but that flow is NOT yet designed or built. The DPAPI key sealing + recovery key spec is a pre-pilot item (see the tracker). Do not invent a production key flow before that spec lands.
- CI proves no test/dev key or dev-seed PIN reaches a release build: the build-windows job greps the release exe for the dev-seed marker (`SHOPCORE_DEV_SEED_MARKER_V1`, which covers the seed PINs 1234/4321 — they live in the seed file alongside the marker) and for the test key string, and fails if either is found.

### Untrusted database files (RUSTSEC-2026-0502, 2026-11-12)
A database file arriving from outside the till (a restored backup, a downloaded snapshot, any file a less trusted party could have written) is UNTRUSTED until proven otherwise. The WAL-read vulnerability fixed in SQLCipher 4.20.0 (RUSTSEC-2026-0502 / CVE-2026-119375, ADR-037) is exactly this shape: opening a hostile database file can exploit the reader. Rule: before any outside database file is opened as a live till database, (1) verify its checksum against the value recorded at backup/snapshot time, and (2) open it IN ISOLATION (a throwaway copy, never the live path) and run `PRAGMA quick_check` — only a file that passes both is trusted and may replace or seed the live database. A file that fails either check is rejected and the failure is audited. This rule must be written into the future backup/restore spec and the future snapshot spec (spec 005's snapshot is a server-side catalog stream, not a database file — the rule applies when a snapshot is ever delivered as a database file).

## Tauri/WebView hardening
- Strict CSP, no remote content, no `eval`; UI assets bundled.
- Tauri capabilities allowlist: only the commands the UI needs; deny filesystem/shell access from the webview.
- DevTools disabled in production builds.
- External links open in the system browser through an allowlist.
- Validate all IPC inputs in Rust; treat the renderer as untrusted.

## Updates and licensing
- Signed installers and signed update manifests; the updater verifies signatures. Staged rollout (e.g., 1% → 10% → 100%) with automatic halt on crash-rate spike. Rollback path documented.
- Updates never install mid-shift; scheduled on idle or at shift close; migrations run with an automatic pre-migration backup.
- License/activation: per-tenant, per-terminal token; grace period offline (≥ 30 days); expiry never bricks selling (degrades to read-only reports + warnings first). No dark patterns that could block a till during trading hours.

## Payments and PCI scope
Card details never enter our app, DB, or logs. Terminals handle card data; we store only approval status, masked reference, and terminal ID. This keeps us out of most PCI scope; confirm with the chosen payment provider.

## Privacy
Customer data minimized; export/delete on request (soft delete + anonymize PII while keeping financial records). Per-tenant data isolation enforced by row-level security in the cloud.

## Tax and fiscal compliance (research item — do early)
Many markets require fiscal receipts, tax-authority POS integration, or e-invoicing. For each launch market, capture: required receipt fields, sequential numbering rules, authority integration/API, offline rules, record retention periods. This can change receipt numbering, sync, and printing architecture, so record findings in `08-decisions.md` before building receipts/invoicing. Status: __TBD per market__.

## Retention
Financial records retained per local law (default 7 years; configurable per market). Archival keeps them retrievable; deletion of customer PII does not break financial integrity.

## Security checks in CI
`cargo audit`/`cargo deny` (advisories + licenses), `pnpm audit`, secret scanning, dependency pinning, SBOM per release, signed artifacts. Pen-test before general availability.
