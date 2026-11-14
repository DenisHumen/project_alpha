-- SPDX-License-Identifier: MPL-2.0
--
-- LIB-010: versions, favorites, collections and playlists. Expand-only.
--
-- version_prefs          the default version of a work for a profile: the
--                        file that plays when a movie has several versions
--                        (4K, 1080p, Director's Cut). Without a row the
--                        first file of the work plays. Goes away with the
--                        file, so a removed version falls back by itself.
-- favorites.pinned_home  the favorite is shown in the Favorites row of Home
--                        (1 by default; 0 keeps it a favorite but off Home).
-- playlist_items_media   lookup of a playable item in playlists.
--
-- `favorites`, `collections` (manual and TMDB `external` ones) and
-- `playlists` themselves are tables of schema v1.

CREATE TABLE version_prefs (
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    work_id       INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    file_id       INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (profile_id, work_id)
) STRICT;

CREATE INDEX version_prefs_work ON version_prefs (work_id);
CREATE INDEX version_prefs_file ON version_prefs (file_id);

ALTER TABLE favorites ADD COLUMN pinned_home INTEGER NOT NULL DEFAULT 1
    CHECK (pinned_home IN (0, 1));

CREATE INDEX playlist_items_media ON playlist_items (media_key);
