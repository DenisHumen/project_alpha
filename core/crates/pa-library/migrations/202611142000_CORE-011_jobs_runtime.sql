-- SPDX-License-Identifier: MPL-2.0
--
-- CORE-011: runtime columns of the persistent job queue (pa-jobs).
-- Expand-only migration: new nullable or defaulted columns.
--
-- max_attempts      runs before a retryable error becomes `failed`.
-- network           what the job needs from the network: 'none', 'any' or
--                   'unmetered' (Wi-Fi only).
-- allow_on_battery  0: the job waits while the device runs on battery.
-- pause_reason      why a job is `paused`: 'user' stays paused until resumed;
--                   'network', 'battery' and 'resources' are set by the
--                   scheduler and cleared when the condition is back.
-- wait_reason       why a job is `waiting_resources` (diagnostics, UI).
-- checkpoint_json   handler state for resuming after a restart (downloaded
--                   segments, Range offset…), written while running.

ALTER TABLE jobs ADD COLUMN max_attempts INTEGER NOT NULL DEFAULT 5 CHECK (max_attempts >= 1);
ALTER TABLE jobs ADD COLUMN network TEXT NOT NULL DEFAULT 'none'
    CHECK (network IN ('none', 'any', 'unmetered'));
ALTER TABLE jobs ADD COLUMN allow_on_battery INTEGER NOT NULL DEFAULT 1
    CHECK (allow_on_battery IN (0, 1));
ALTER TABLE jobs ADD COLUMN pause_reason TEXT
    CHECK (pause_reason IS NULL OR pause_reason IN ('user', 'network', 'battery', 'resources'));
ALTER TABLE jobs ADD COLUMN wait_reason TEXT;
ALTER TABLE jobs ADD COLUMN checkpoint_json TEXT
    CHECK (checkpoint_json IS NULL OR json_valid(checkpoint_json));
