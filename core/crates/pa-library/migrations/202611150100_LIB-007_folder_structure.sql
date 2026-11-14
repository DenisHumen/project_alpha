-- SPDX-License-Identifier: MPL-2.0
--
-- LIB-007: structural folder analysis (pa-scanner `analyze_folders`).
-- Expand-only.
--
-- file_groups.disc_unit       'bdmv', 'video_ts' or 'iso' for kind = 'disc'.
-- file_groups.analysis_hash   hash of the last analysis result of the group;
--                             a rerun with the same result writes nothing.
-- file_groups.analyzed_at_ms  when the group was last written.
--
-- group_assets   entries of a group that `file_group_members` cannot hold:
--                files that are not rows of `files` (sidecar subtitles, NFO,
--                local art, extras and samples the shallow scan skips, disc
--                streams) and files the analysis ignores (leftover cuts).
--                `file_id` links the `files` row when there is one.
--   target_path    the video a sidecar belongs to (NULL: the whole group).
--   lang           subtitle language, ISO 639-2 (`rus`, `eng`), NULL if unknown.
--   forced, hearing_impaired, is_default   subtitle flags from the name.
--   art_kind       local art: poster, fanart, clearlogo, clearart, banner,
--                  thumb, discart, scan (booklet/cover scans); season_number
--                  for `season01-poster.jpg`.
--   ids_json       external ids found in an NFO: {"tmdb":"603","imdb":"tt0133093"}.

ALTER TABLE file_groups ADD COLUMN disc_unit TEXT
    CHECK (disc_unit IS NULL OR disc_unit IN ('bdmv', 'video_ts', 'iso'));
ALTER TABLE file_groups ADD COLUMN analysis_hash TEXT;
ALTER TABLE file_groups ADD COLUMN analyzed_at_ms INTEGER;

CREATE TABLE group_assets (
    id               INTEGER PRIMARY KEY,
    group_id         INTEGER NOT NULL REFERENCES file_groups (id) ON DELETE CASCADE,
    path             TEXT    NOT NULL CHECK (path <> ''),
    file_id          INTEGER REFERENCES files (id) ON DELETE SET NULL,
    role             TEXT    NOT NULL
                     CHECK (role IN ('main', 'part', 'extra', 'sample', 'subtitle', 'nfo', 'art',
                                     'ignore')),
    part_no          INTEGER CHECK (part_no > 0),
    extra_type       TEXT,
    target_path      TEXT,
    lang             TEXT,
    forced           INTEGER NOT NULL DEFAULT 0 CHECK (forced IN (0, 1)),
    hearing_impaired INTEGER NOT NULL DEFAULT 0 CHECK (hearing_impaired IN (0, 1)),
    is_default       INTEGER NOT NULL DEFAULT 0 CHECK (is_default IN (0, 1)),
    art_kind         TEXT    CHECK (art_kind IS NULL OR art_kind IN ('poster', 'fanart', 'clearlogo',
                                                                     'clearart', 'banner', 'thumb',
                                                                     'discart', 'scan')),
    season_number    INTEGER CHECK (season_number >= 0),
    ids_json         TEXT    CHECK (ids_json IS NULL OR json_valid(ids_json)),
    size_bytes       INTEGER NOT NULL DEFAULT 0 CHECK (size_bytes >= 0),
    UNIQUE (group_id, path)
) STRICT;

CREATE INDEX group_assets_file ON group_assets (file_id) WHERE file_id IS NOT NULL;
