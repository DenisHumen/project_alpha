-- SPDX-License-Identifier: MPL-2.0
--
-- LIB-011: "Remove from Continue watching". Expand-only migration.
--
-- A row is hidden from the "Continue watching" row while
-- hidden_at_ms >= updated_at_ms; the next playback (a newer updated_at_ms)
-- brings it back. Local to the device: there is no sync op for it yet.

ALTER TABLE progress ADD COLUMN hidden_at_ms INTEGER CHECK (hidden_at_ms >= 0);
