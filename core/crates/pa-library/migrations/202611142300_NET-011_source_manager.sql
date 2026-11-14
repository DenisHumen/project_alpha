-- SPDX-License-Identifier: MPL-2.0
--
-- NET-011: source manager (pa-sources). Settings and status of a source.
--
-- Settings:
-- options_json       backend options (SMB dialect, signing, encryption,
--                    domain; WebDAV certificate pin; timeout, read-ahead).
--                    Never credentials: the password lives in the OS secret
--                    store under `credential_ref`.
-- scan_into_library  files of the source go to the library (0: browse only).
-- auto_scan          rescan on schedule and on start (0: only by hand).
-- updated_at_ms      last change of the settings.
--
-- Status (device-local, never synced):
-- online                        result of the last check or speed test
--                               (NULL: never checked).
-- checked_at_ms                 when it was checked.
-- last_scan_started_at_ms       start of the last scan.
-- last_scan_finished_at_ms      end of the last scan (NULL while it runs).
-- last_error, last_error_at_ms  last connection or scan error, secrets masked.
-- speed_*                       last speed test.

ALTER TABLE sources ADD COLUMN options_json TEXT NOT NULL DEFAULT '{}'
    CHECK (json_valid(options_json));
ALTER TABLE sources ADD COLUMN scan_into_library INTEGER NOT NULL DEFAULT 1
    CHECK (scan_into_library IN (0, 1));
ALTER TABLE sources ADD COLUMN auto_scan INTEGER NOT NULL DEFAULT 1
    CHECK (auto_scan IN (0, 1));
ALTER TABLE sources ADD COLUMN updated_at_ms INTEGER;
ALTER TABLE sources ADD COLUMN online INTEGER CHECK (online IN (0, 1));
ALTER TABLE sources ADD COLUMN checked_at_ms INTEGER;
ALTER TABLE sources ADD COLUMN last_scan_started_at_ms INTEGER;
ALTER TABLE sources ADD COLUMN last_scan_finished_at_ms INTEGER;
ALTER TABLE sources ADD COLUMN last_error TEXT;
ALTER TABLE sources ADD COLUMN last_error_at_ms INTEGER;
ALTER TABLE sources ADD COLUMN speed_bytes_per_sec INTEGER CHECK (speed_bytes_per_sec >= 0);
ALTER TABLE sources ADD COLUMN speed_latency_ms INTEGER CHECK (speed_latency_ms >= 0);
ALTER TABLE sources ADD COLUMN speed_measured_at_ms INTEGER;
