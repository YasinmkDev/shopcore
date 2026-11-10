# Spec 005 — Two-node sync spike (outbox → server → pull) and snapshot download

Status: Draft (needs human approval)   Phase: 0   Depends on: 000, 002 (reuses the dataset/harness)
Decides: confirms ADR-009 (custom outbox/inbox sync) or triggers a revision.

**Toolchain (amendment 1):** Rust pinned to **1.99.1** in `rust-toolchain.toml` (ADR-020). sqlx 0.9.0 requires Rust ≥ 1.94.0; the pin satisfies it, so no toolchain change is needed for this spec.

## Goal
Prove idempotent, resumable, scoped sync with chaos conditions, a fast first-sync via snapshot, and no impact on checkout latency.

## Sync client separation (amendment 2)
The sync client (the till-side code that pushes/pulls over HTTP, using `reqwest` and therefore `aws-lc-rs`) lives in a **separate crate, `crates/sync-client`**, and is **NOT a dependency of the desktop crate** (`crates/desktop/src-tauri`). The shipped desktop binary therefore contains no `reqwest` / `aws-lc-rs` code. The session summary reports the measured installer-size delta and the Windows CI build-time delta for the added crates, and justifies `aws-lc-rs` vs `ring` (see below). `aws-lc-rs` is reqwest 0.13's default rustls crypto provider; `ring` would require the `rustls-no-provider` feature plus a manual `CryptoProvider` install, and `ring` is itself native (C/assembly) code — so `ring` buys no pure-Rust advantage, only more wiring. Both are native crypto, neither is OpenSSL.

## In scope
- `crates/protocol`: wire types (batch, ack, cursor, tombstone) with versioning.
- Till side (SQLite): `outbox` written in the same transaction as the source write; sync worker pushes batches (≤ 200 rows / ≤ 256 KB) with per-row idempotency keys; pull by per-stream cursor applied in short transactions (≤ 50 ms write-lock each).
- Server side (`crates/server`, Axum + Postgres via CI service container): `POST /sync/push`, `GET /sync/pull?stream&cursor`, tenant_id + branch scoping, per-tenant monotonic sequence, idempotent apply.
- Streams for the spike: `sale` + `sale_line` + `stock_movement` (push, append-only); `price_list_entry` (pull, per-field last-write-wins by server time).
- **Money first (amendment 2 / fix 4):** before the revalidation stub, `crates/domain` gets ONLY `Money` — i64 minor units (per `02-domain-model.md`), checked add/sub (overflow → error, never wrap), ordering, serde, and property tests. No pricing, no tax. `Money` is Phase 1's foundation, so it must be correct. `crates/domain/src/lib.rs` is currently empty (doc comment only); this is its first real type.
- **Revalidation stub:** server recomputes `sum(line totals) == sale total` using the real `crates/domain` `Money` with checked ops — the stub only checks line-total sums, it is not a copy of the domain logic. Mismatches flagged, never altered. (Full pricing/tax comes in Phase 1.)
- **Snapshot:** server produces a compressed (zstd) SQLite snapshot of the branch's **catalog + stock balances** (500k SKUs from spec 002), served with HTTP Range for resume (see fix 5), SHA-256 checksum verified on the till, then incremental pull continues from the snapshot's cursor. The balances-to-cursor handoff is the risky part, so it is in scope for the spike. **Customers and settings are deferred** — they reuse the same snapshot mechanism and are added later; this is a deliberate narrowing against `context/04-data-and-sync.md` line 51 (which lists catalog + customers + settings + balances for initial sync). Recorded here and noted against context/04.
- **HTTP Range (amendment 3 / fix 5):** use `tower-http` (`ServeFile`, `fs` feature) for Range serving. No manual Range parsing.

## Out of scope
Hub/LAN mode, auth beyond a static dev token, all other entities, UI. (The dev token is a dev-only placeholder such as `dev-only`, never a real secret, and is replaced by real auth before Phase 5.) Customers and settings snapshots are deferred (see above).

## Data changes
- **Outbox (fix 1):** the spike's `outbox` table carries the full `context/04` column set, not the short list the draft had. Columns: `id` (UUIDv7), `tenant_id`, `branch_id`, `entity`, `stream`, `payload`, `idempotency_key`, `created_at`, `updated_at`, `deleted_at` (nullable), `version` (int, optimistic concurrency), `sent_at` (nullable), `attempts`. Partial index `outbox(sent_at) WHERE sent_at IS NULL`.
- **Idempotency key (amendment 7):** the idempotency key per pushed row IS the outbox row's own UUIDv7 (`id`). It is stable across retries by construction — a retry re-sends the same row with the same key, so the server's dedupe is a single-column lookup on a value the till already owns.
- **Idempotency enforced by the database (amendment 5):** on the server, the table that stores applied push rows carries `UNIQUE (tenant_id, idempotency_key)`. The server's apply is an insert that relies on this constraint — a duplicate row is rejected by Postgres itself, not by application-level dedupe. The replay test (chaos test 9) must prove the constraint fires (a second apply of the same batch fails on the constraint and leaves state unchanged).
- **`sync_state`** per `04-data-and-sync.md` (cursor per stream), in the spike's till DB and the spike server DB.
- **Server stock balances (amendment 8):** the server derives `stock_balance` from pushed movements with the **same rule as the till** (see `04-data-and-sync.md` and the seed aggregation in spec 002): `stock_balance` is the sum of `qty_milli` over `stock_movement` grouped by `(branch_id, sku_id)`, and it is maintained in the **same Postgres transaction** as the movement insert — the apply of a batch inserts its movements and updates the affected balances atomically, so a balance is never visible without its movements (and vice versa). The server never writes `stock_balance` outside that apply transaction, mirroring the till's "never write `stock_balances` outside the posting function" invariant (AGENTS.md).
- **Pull stream `price_list_entry` (fix 9):** a pull-only stream. Each entry: `id` (UUIDv7), `tenant_id`, `branch_id`, `sku_id`, `price_minor` (`Money`, i64 minor units), `currency` (per ADR-017, single currency per tenant in v1), `effective_from` (server time), `version`, `deleted_at` (nullable, tombstone). Conflict rule: per-field last-write-wins by **server time**; the till stores both the server time and its own local receipt time. The server is the only writer for this stream in the spike.
- Wire types (batch, ack, cursor, tombstone) in `crates/protocol`, versioned.

## Security / audit
Static dev token only (placeholder, see above). All server endpoints enforce tenant_id + branch scoping; no cross-tenant access.

## Definition of done
Standard checklist in `AGENTS.md` + all acceptance criteria above met; chaos tests green in CI; the written ADR-009 assessment is recorded (see Stop condition).

## Chaos tests (must pass, automated)
1. Duplicate delivery of the same batch → no duplicate rows.
2. Out-of-order batches → converge to the same state.
3. Connection dropped mid-batch → resume without loss or duplication.
4. Server restart during push → client retries safely.
5. Clock skew of ±10 min on the till → ordering uses server time; both timestamps stored.
6. Two tills sell the last unit simultaneously → both sales accepted, balance negative, item flagged.
7. Upgrade (new schema version) with unsent outbox rows → rows still delivered.
8. Snapshot download interrupted at random points → resumes, checksum passes, cursor handoff has no gap or overlap.
9. Replaying an entire push history twice yields byte-identical server state — and the replay returns the **existing ack / sequence with no new row written** (fix 3: a replay is a no-op read, not a second apply). The replay test proves the `UNIQUE (tenant_id, idempotency_key)` constraint fires: the second apply is rejected by Postgres and the state is unchanged (amendment 5).

## Per-field LWW merge test (amendment 4)
Two server-side edits to **different fields** of one `price_list_entry` must merge correctly: edit A changes `price_minor` only, edit B changes `effective_from` only (both against the same base version). After both are applied, the entry carries A's price AND B's effective date — neither edit clobbers the other's field. This test is part of the spike (it runs against the server's pull/apply path). If the merge cannot be demonstrated, the spec and `08-decisions.md` must state explicitly that per-field LWW remains **unproven until Phase 5**.

## Performance
Measure rows/s for push and pull, snapshot size and download+apply time for 500k SKUs on LAN-like and throttled links, and checkout latency **both at baseline (spec 002 scenario 4) AND under load (spec 002 scenario 5)** while sync runs (fix 8 — scenario 4 alone does not prove sync is harmless under load). Sync must not move checkout p95 outside budget in either case.

## Dependencies (pinned, proof in the approval reply)
New crates, all rustls (default features off where possible), no new OpenSSL:

| Crate | Pinned | Features | Why |
|---|---|---|---|
| `axum` | 0.8.9 | default | HTTP server: routing for `POST /sync/push`, `GET /sync/pull`, snapshot download. |
| `sqlx` | 0.9.0 | `postgres`, `runtime-tokio`, `tls-rustls`, `migrate` (default-features off) | Postgres driver for the server; rustls TLS (not native-tls/OpenSSL); migrations for the server schema. |
| `zstd` | 0.14.3 | default | Snapshot compression (500k-SKU SQLite file). |
| `sha2` | 0.11.1 | default | SHA-256 checksum for the snapshot (till verifies before applying). |
| `reqwest` | 0.13.5 | `rustls`, `json` (default-features off) | Till-side HTTP client: push/pull + snapshot download with Range resume. rustls, not native-tls. |
| `tower-http` | 0.6.11 | `fs` | HTTP Range serving for the snapshot (`ServeFile`). Pinned to 0.6.11 to match axum 0.8.9's own tower-http (avoids two versions in the tree). Range support verified in the 0.6.11 source (`try_parse_range`). |
| `tokio` | 1.54.1 | already in workspace | Async runtime (shared with the rest of the workspace). |
| `serde` | 1.0.229 | already in workspace | Wire-type serialisation. |

**License flag (from the proof):** `cargo deny check licenses` on the scratch tree passes except one crate — `webpki-roots` 1.0.11 (Mozilla root-cert bundle, pulled by sqlx's `tls-rustls`) is licensed `CDLA-Permissive-2.0`, which is NOT in `deny.toml`'s allow-list. Decision needed with the table: either (a) add `"CDLA-Permissive-2.0"` to the allow-list (one line, with a comment — it is a permissive *data* licence for the cert bundle, not a code licence), or (b) drop `tls-rustls` from sqlx (plaintext local Postgres only). Recommended: (a). **No OpenSSL is in the tree** — no `openssl`, `openssl-sys`, or `native-tls` crate. The only OpenSSL-named crate is `openssl-probe` 0.2.2, which only locates the system cert directory (no linking, no OpenSSL code). Note: reqwest's `rustls` feature uses `aws-lc-rs` as its crypto provider (a C library, AWS-LC — not OpenSSL); it builds on Windows CI (cmake is preinstalled on `windows-latest`).

## Local run + CI (fixes 6, 7; amendment 5)
- **docker-compose (fix 6):** no `docker-compose.yml` exists yet. The implementation creates one at the repo root with a Postgres service for local runs (Postgres version pinned in the file; data in a named volume, never committed).
- **Linux `server-tests` CI job (fix 7):** no such job exists yet. It lands with spec 005, per ADR-015: the server/Postgres chaos tests run on a Linux CI job (Postgres service container); till-side unit tests also run on the Windows job.
- **Postgres tests never skip silently (amendment 5):** a local run skips the Postgres-backed tests ONLY when the explicit env flag `SHOPCORE_SKIP_POSTGRES_TESTS=1` is set, and then it prints a loud message naming the skipped tests. Without the flag and without Postgres, the tests FAIL (they do not skip). The Linux `server-tests` CI job fails if Postgres is unavailable.

## Stop condition (amendment 6)
If a chaos test reveals a **protocol design flaw** (not a bug in the test or the code), STOP. Do not patch around it. Report it with evidence: the failing test, the observed state, and the exact protocol property that is broken. The spec ends with a **written assessment of ADR-009** — keep, or revise (with the concrete protocol change needed before Phase 5). A revision is a valid outcome of this spike.

## Acceptance
All chaos tests green in CI; numbers recorded in `08-decisions.md`; written assessment of ADR-009 with any protocol changes needed before Phase 5.

## Risks
Cursor design mistakes cause silent gaps (add a gap detector test); snapshot/incremental handoff races; Postgres service availability in Windows CI (use Linux runner for server tests).
