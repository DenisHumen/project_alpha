-- SPDX-License-Identifier: MPL-2.0
--
-- Outbox of pa-sync (SYNC-001, docs/research/04 §1.8.1): local sync ops
-- waiting for an ack from a peer or hub. Expand-only migration.

CREATE TABLE sync_outbox (
    seq                INTEGER PRIMARY KEY,
    -- UUIDv7 of the op, hyphenated; receivers deduplicate by it.
    op_id              TEXT    NOT NULL UNIQUE CHECK (length(op_id) = 36),
    -- Op kind (`progress.upsert`, `watched`…), for diagnostics.
    op                 TEXT    NOT NULL CHECK (op <> ''),
    -- pa_model::Versioned<pa_sync::Op> as JSON.
    payload            TEXT    NOT NULL CHECK (json_valid(payload)),
    -- HLC of the op as `wall.counter@device`, for diagnostics.
    hlc                TEXT    NOT NULL,
    attempts           INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_attempt_at_ms INTEGER NOT NULL,
    last_error         TEXT,
    created_at_ms      INTEGER NOT NULL
) STRICT;

CREATE INDEX sync_outbox_due ON sync_outbox (next_attempt_at_ms, seq);
