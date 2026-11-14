-- SPDX-License-Identifier: MPL-2.0
--
-- LIB-009: library queries — filters, sorts, per-view preferences, smart
-- shelves. Expand-only.
--
-- work_genres    genres of a work by provider (TMDB names in the metadata
--                language, AniList English names); the genre filter and its
--                facet list read them.
-- files.audio_formats
--                audio formats of a probed file (LIB-006 `pa-probe`): a JSON
--                array of slugs (`aac`, `ac3`, `eac3`, `truehd`, `dts`,
--                `dts_hd_ma`, `flac`, `opus`, `pcm`…, plus `atmos` / `dts_x`
--                for object audio). Cleared with the other probe columns: a
--                trigger empties it when a scan resets `probed_at_ms`.
-- view_prefs     per profile and view (`movies`, `shows`, `folder:<source>`…):
--                saved sort, direction, "folders first" and the last filter.
-- smart_shelves  saved queries (filter + sort) of a profile, optionally
--                pinned to Home in `position` order.

CREATE TABLE work_genres (
    work_id  INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    -- tmdb, anilist, shikimori, local…
    provider TEXT    NOT NULL CHECK (provider <> '' AND provider NOT GLOB '*[^a-z0-9_]*'),
    name     TEXT    NOT NULL CHECK (name <> ''),
    PRIMARY KEY (work_id, provider, name)
) STRICT;

CREATE INDEX work_genres_name ON work_genres (name COLLATE NOCASE, work_id);

ALTER TABLE files ADD COLUMN audio_formats TEXT
    CHECK (audio_formats IS NULL OR json_valid(audio_formats));

CREATE TRIGGER files_audio_formats_reset AFTER UPDATE OF probed_at_ms ON files
WHEN NEW.probed_at_ms IS NULL AND NEW.audio_formats IS NOT NULL
BEGIN
    UPDATE files SET audio_formats = NULL WHERE id = NEW.id;
END;

CREATE TABLE view_prefs (
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    view          TEXT    NOT NULL CHECK (length(view) BETWEEN 1 AND 128),
    -- Sort field slug (`title`, `year`, `added`, `duration`, `last_watched`…).
    sort          TEXT    NOT NULL CHECK (sort <> '' AND sort NOT GLOB '*[^a-z0-9_]*'),
    descending    INTEGER NOT NULL DEFAULT 0 CHECK (descending IN (0, 1)),
    folders_first INTEGER NOT NULL DEFAULT 1 CHECK (folders_first IN (0, 1)),
    -- Last filter of the view (pa-catalog `LibraryFilter` as JSON), if kept.
    filter_json   TEXT    CHECK (filter_json IS NULL OR json_valid(filter_json)),
    updated_at_ms INTEGER NOT NULL,
    PRIMARY KEY (profile_id, view)
) STRICT;

CREATE TABLE smart_shelves (
    id            INTEGER PRIMARY KEY,
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    name          TEXT    NOT NULL CHECK (name <> ''),
    -- pa-catalog `LibraryQuery` (kind, filter, sort) as JSON.
    query_json    TEXT    NOT NULL CHECK (json_valid(query_json)),
    pinned_home   INTEGER NOT NULL DEFAULT 0 CHECK (pinned_home IN (0, 1)),
    position      INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX smart_shelves_profile ON smart_shelves (profile_id, position);
