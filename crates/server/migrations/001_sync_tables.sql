-- Spec 005 sync spike: server-side sync tables.
--
-- Times are BIGINT milliseconds since epoch (not TIMESTAMPTZ) so the
-- server needs no chrono dependency; the wire format is already epoch ms.
--
-- Idempotency is enforced by the DATABASE, not by application code
-- (amendment 5): `applied_push` has PRIMARY KEY (tenant_id, idempotency_key),
-- so a duplicate row is rejected by Postgres itself. The apply path uses
-- INSERT ... ON CONFLICT DO NOTHING and the replay test proves the
-- constraint fires.

CREATE TABLE IF NOT EXISTS applied_push (
    tenant_id UUID NOT NULL,
    idempotency_key UUID NOT NULL,
    entity TEXT NOT NULL,
    stream TEXT NOT NULL,
    payload JSONB NOT NULL,
    server_sequence BIGINT NOT NULL,
    applied_at_ms BIGINT NOT NULL,
    PRIMARY KEY (tenant_id, idempotency_key)
);

CREATE TABLE IF NOT EXISTS stock_movement (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sku_id UUID NOT NULL,
    qty_milli BIGINT NOT NULL,
    occurred_at_ms BIGINT NOT NULL,
    server_sequence BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_movement_branch_sku
    ON stock_movement (tenant_id, branch_id, sku_id);

-- Stock balances are derived from movements, never written directly
-- (amendment 8): the apply transaction inserts movements and upserts the
-- affected balances atomically, with the same rule as the till
-- (balance = sum of qty_milli over movements per branch+sku).
CREATE TABLE IF NOT EXISTS stock_balance (
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sku_id UUID NOT NULL,
    qty_milli BIGINT NOT NULL DEFAULT 0,
    PRIMARY KEY (tenant_id, branch_id, sku_id)
);

-- Price-list entries (pull stream). Per-field LWW (amendment 4): each
-- mutable field carries its own update timestamp, so two edits to
-- different fields merge instead of clobbering each other.
CREATE TABLE IF NOT EXISTS price_list_entry (
    id UUID PRIMARY KEY,
    tenant_id UUID NOT NULL,
    branch_id UUID NOT NULL,
    sku_id UUID NOT NULL,
    price_minor BIGINT NOT NULL,
    price_updated_at_ms BIGINT NOT NULL,
    currency TEXT NOT NULL,
    effective_from_ms BIGINT NOT NULL,
    effective_updated_at_ms BIGINT NOT NULL,
    version BIGINT NOT NULL DEFAULT 1,
    deleted_at_ms BIGINT,
    server_sequence BIGINT NOT NULL,
    updated_at_ms BIGINT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_price_stream
    ON price_list_entry (tenant_id, branch_id, server_sequence);

-- Per-tenant monotonic sequence. The row is locked (SELECT ... FOR UPDATE)
-- for the duration of a batch apply, serializing batches per tenant.
CREATE TABLE IF NOT EXISTS server_sequence (
    tenant_id UUID PRIMARY KEY,
    last_sequence BIGINT NOT NULL DEFAULT 0
);
