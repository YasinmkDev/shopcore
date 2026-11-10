# Spec 005 — Two-node sync spike (outbox → server → pull) and snapshot download

Status: Draft (needs human approval)   Phase: 0   Depends on: 000, 002 (reuses the dataset/harness)
Decides: confirms ADR-009 (custom outbox/inbox sync) or triggers a revision.

## Goal
Prove idempotent, resumable, scoped sync with chaos conditions, a fast first-sync via snapshot, and no impact on checkout latency.

## In scope
- `crates/protocol`: wire types (batch, ack, cursor, tombstone) with versioning.
- Till side (SQLite): `outbox` written in the same transaction as the source write; sync worker pushes batches (≤ 200 rows / ≤ 256 KB) with per-row idempotency keys; pull by per-stream cursor applied in short transactions (≤ 50 ms write-lock each).
- Server side (`crates/server`, Axum + Postgres via CI service container): `POST /sync/push`, `GET /sync/pull?stream&cursor`, tenant_id + branch scoping, per-tenant monotonic sequence, idempotent apply.
- Streams for the spike: `sale` + `sale_line` + `stock_movement` (push, append-only); `price_list_entry` (pull, per-field last-write-wins by server time).
- **Revalidation stub:** server recomputes `sum(line totals) == sale total` using a minimal `domain::Money` with checked ops; mismatches flagged, never altered. (Full pricing/tax comes in Phase 1.)
- **Snapshot:** server produces a compressed (zstd) SQLite snapshot of the branch's catalog (500k SKUs from spec 002), served with HTTP Range for resume, SHA-256 checksum verified on the till, then incremental pull continues from the snapshot's cursor.

## Out of scope
Hub/LAN mode, auth beyond a static dev token, all other entities, UI.

## Chaos tests (must pass, automated)
1. Duplicate delivery of the same batch → no duplicate rows.
2. Out-of-order batches → converge to the same state.
3. Connection dropped mid-batch → resume without loss or duplication.
4. Server restart during push → client retries safely.
5. Clock skew of ±10 min on the till → ordering uses server time; both timestamps stored.
6. Two tills sell the last unit simultaneously → both sales accepted, balance negative, item flagged.
7. Upgrade (new schema version) with unsent outbox rows → rows still delivered.
8. Snapshot download interrupted at random points → resumes, checksum passes, cursor handoff has no gap or overlap.
9. Replaying an entire push history twice yields byte-identical server state.

## Performance
Measure rows/s for push and pull, snapshot size and download+apply time for 500k SKUs on LAN-like and throttled links, and checkout latency (spec 002 scenario 4) while sync runs. Sync must not move checkout p95 outside budget.

## Dependencies to propose
`axum`, `tokio`, `sqlx` (Postgres), `zstd`, `sha2`, `reqwest`, `serde`. Justify each.

## Acceptance
All chaos tests green in CI; numbers recorded in `08-decisions.md`; written assessment of ADR-009 with any protocol changes needed before Phase 5.

## Risks
Cursor design mistakes cause silent gaps (add a gap detector test); snapshot/incremental handoff races; Postgres service availability in Windows CI (use Linux runner for server tests).
