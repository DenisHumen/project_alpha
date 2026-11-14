-- SPDX-License-Identifier: MPL-2.0
--
-- MUS-004: play counts of music tracks for the music Home ("Recently
-- played", "Frequently played"). Expand-only migration.
--
-- One row per profile and track, by the stable media key of the track
-- (`file:<fingerprint>`, as progress), so counts survive rescans. A play is
-- counted when the track starts in the player (`music::record_play`).

CREATE TABLE music_plays (
    profile_id        INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    media_key         TEXT    NOT NULL CHECK (length(media_key) BETWEEN 3 AND 256 AND media_key GLOB '?*:?*'),
    play_count        INTEGER NOT NULL CHECK (play_count > 0),
    last_played_at_ms INTEGER NOT NULL,
    PRIMARY KEY (profile_id, media_key)
) STRICT;

-- "Recently played": latest first.
CREATE INDEX music_plays_recent ON music_plays (profile_id, last_played_at_ms DESC);
