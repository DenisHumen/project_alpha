-- SPDX-License-Identifier: MPL-2.0
--
-- LIB-001: library schema v1 (docs/dev/db.md, ADR-0013).
--
-- One schema for clients and the media server. Conventions:
-- - STRICT tables, `INTEGER PRIMARY KEY` row ids (local, never synced);
--   sync uses the stable `media_key` text (pa-model `MediaKey`).
-- - Times are Unix milliseconds in `*_at_ms`, durations in `*_ms`.
-- - Booleans are INTEGER 0/1. JSON columns are checked with json_valid().
-- - Closed enums (mirroring pa-model) have a CHECK list; open sets
--   (providers, source kinds, job types) only a lowercase-slug CHECK, so a
--   new value needs no table rebuild.
-- - Applied migrations are never edited: change the schema with a new file
--   (expand -> contract, LIB-002).

-- Sources ------------------------------------------------------------------

CREATE TABLE sources (
    id             INTEGER PRIMARY KEY,
    kind           TEXT    NOT NULL CHECK (kind <> '' AND kind NOT GLOB '*[^a-z0-9_]*'),
    name           TEXT    NOT NULL,
    -- Root URI without credentials (file:///…, smb://host/share/dir).
    uri            TEXT    NOT NULL CHECK (uri <> ''),
    username       TEXT,
    -- Key of the secret in the OS secure storage. Never a password.
    credential_ref TEXT,
    position       INTEGER NOT NULL DEFAULT 0,
    enabled        INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at_ms  INTEGER NOT NULL
) STRICT;

-- Files and groups ---------------------------------------------------------

CREATE TABLE files (
    id                 INTEGER PRIMARY KEY,
    source_id          INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    -- POSIX path relative to the source root. Never synced.
    path               TEXT    NOT NULL CHECK (path <> ''),
    size_bytes         INTEGER NOT NULL CHECK (size_bytes >= 0),
    mtime_ms           INTEGER,
    inode              INTEGER,
    container          TEXT,
    duration_ms        INTEGER CHECK (duration_ms >= 0),
    video_json         TEXT    CHECK (video_json IS NULL OR json_valid(video_json)),
    audio_langs        TEXT    CHECK (audio_langs IS NULL OR json_valid(audio_langs)),
    sub_langs          TEXT    CHECK (sub_langs IS NULL OR json_valid(sub_langs)),
    -- Content fingerprint, lowercase hex; basis of the `file:` media key.
    fingerprint        TEXT    CHECK (fingerprint IS NULL
                                      OR (fingerprint <> '' AND fingerprint NOT GLOB '*[^0-9a-f]*')),
    oshash             TEXT    CHECK (oshash IS NULL
                                      OR (length(oshash) = 16 AND oshash NOT GLOB '*[^0-9a-f]*')),
    ed2k               TEXT    CHECK (ed2k IS NULL
                                      OR (length(ed2k) = 32 AND ed2k NOT GLOB '*[^0-9a-f]*')),
    crc32              TEXT    CHECK (crc32 IS NULL
                                      OR (length(crc32) = 8 AND crc32 NOT GLOB '*[^0-9a-f]*')),
    crc32_in_name      TEXT,
    embedded_tags_json TEXT    CHECK (embedded_tags_json IS NULL OR json_valid(embedded_tags_json)),
    added_at_ms        INTEGER NOT NULL,
    scanned_at_ms      INTEGER,
    probed_at_ms       INTEGER,
    hashed_at_ms       INTEGER,
    -- Soft delete: set when a scan does not see the file (grace period, LIB-003).
    missing_since_ms   INTEGER,
    UNIQUE (source_id, path)
) STRICT;

CREATE INDEX files_fingerprint ON files (fingerprint) WHERE fingerprint IS NOT NULL;
CREATE INDEX files_oshash ON files (oshash, size_bytes) WHERE oshash IS NOT NULL;
CREATE INDEX files_ed2k ON files (ed2k) WHERE ed2k IS NOT NULL;

CREATE TABLE file_groups (
    id        INTEGER PRIMARY KEY,
    source_id INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    root_path TEXT    NOT NULL,
    kind      TEXT    NOT NULL CHECK (kind IN ('series_folder', 'movie_folder', 'disc', 'loose')),
    -- Normalized release-name skeleton (03 §4.8).
    skeleton  TEXT,
    UNIQUE (source_id, root_path)
) STRICT;

CREATE TABLE file_group_members (
    group_id   INTEGER NOT NULL REFERENCES file_groups (id) ON DELETE CASCADE,
    file_id    INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    role       TEXT    NOT NULL
               CHECK (role IN ('main', 'part', 'extra', 'sample', 'subtitle', 'nfo', 'art')),
    part_no    INTEGER CHECK (part_no > 0),
    extra_type TEXT,
    PRIMARY KEY (group_id, file_id)
) STRICT;

CREATE INDEX file_group_members_file ON file_group_members (file_id);

CREATE TABLE parse_results (
    file_id         INTEGER PRIMARY KEY REFERENCES files (id) ON DELETE CASCADE,
    parser_version  TEXT    NOT NULL,
    -- N-best hypotheses with weights (META-005).
    hypotheses_json TEXT    NOT NULL CHECK (json_valid(hypotheses_json)),
    parsed_at_ms    INTEGER NOT NULL
) STRICT;

-- Catalog: canonical works -------------------------------------------------

CREATE TABLE works (
    id             INTEGER PRIMARY KEY,
    kind           TEXT    NOT NULL CHECK (kind IN ('movie', 'show', 'anime')),
    anime_type     TEXT    CHECK (anime_type IN ('tv', 'ova', 'ona', 'movie', 'special', 'music_video')),
    title          TEXT    NOT NULL CHECK (title <> ''),
    original_title TEXT,
    -- ISO 639 code of the original language.
    original_lang  TEXT,
    year           INTEGER CHECK (year BETWEEN 1800 AND 3000),
    runtime_ms     INTEGER CHECK (runtime_ms >= 0),
    status         TEXT,
    overview       TEXT,
    created_at_ms  INTEGER NOT NULL,
    updated_at_ms  INTEGER NOT NULL,
    CHECK (anime_type IS NULL OR kind = 'anime')
) STRICT;

CREATE TABLE external_ids (
    work_id  INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    -- tmdb, tvdb, imdb, anidb, anilist, mal, kitsu, shikimori, kinopoisk, tvmaze…
    provider TEXT    NOT NULL CHECK (provider <> '' AND provider NOT GLOB '*[^a-z0-9_]*'),
    -- Entity type inside the provider: tmdb `movie`/`tv`, tvdb `series`; '' if none.
    ext_kind TEXT    NOT NULL DEFAULT '',
    ext_id   TEXT    NOT NULL CHECK (ext_id <> ''),
    PRIMARY KEY (provider, ext_kind, ext_id),
    UNIQUE (work_id, provider, ext_kind)
) STRICT;

CREATE TABLE titles (
    id      INTEGER PRIMARY KEY,
    work_id INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    -- ISO 639 language and ISO 15924 script; '' if unknown.
    lang    TEXT    NOT NULL DEFAULT '',
    script  TEXT    NOT NULL DEFAULT '',
    text    TEXT    NOT NULL CHECK (text <> ''),
    kind    TEXT    NOT NULL CHECK (kind IN ('main', 'official', 'synonym', 'short', 'romaji')),
    -- Provider the title came from; '' if user or unknown.
    source  TEXT    NOT NULL DEFAULT '',
    UNIQUE (work_id, kind, lang, script, text)
) STRICT;

CREATE TABLE seasons (
    id         INTEGER PRIMARY KEY,
    work_id    INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    scheme     TEXT    NOT NULL CHECK (scheme IN ('tmdb', 'tvdb_aired', 'tvdb_dvd', 'tvdb_absolute',
                                                  'anidb', 'tmdb_group')),
    -- TMDB episode-group id for scheme `tmdb_group`, '' otherwise.
    scheme_ref TEXT    NOT NULL DEFAULT '',
    -- 0 = specials.
    number     INTEGER NOT NULL CHECK (number >= 0),
    title      TEXT,
    year       INTEGER CHECK (year BETWEEN 1800 AND 3000),
    CHECK ((scheme = 'tmdb_group') = (scheme_ref <> '')),
    UNIQUE (work_id, scheme, scheme_ref, number)
) STRICT;

CREATE TABLE episodes (
    id              INTEGER PRIMARY KEY,
    work_id         INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    season_id       INTEGER REFERENCES seasons (id) ON DELETE SET NULL,
    -- Id in the canonical space: AniDB eid or TMDB episode id.
    canonical_ref   TEXT,
    episode_type    TEXT    NOT NULL DEFAULT 'regular'
                    CHECK (episode_type IN ('regular', 'special', 'credit', 'trailer', 'parody',
                                            'recap', 'other')),
    -- Numbers in TMDB order when known; other orders are in episode_mappings.
    season_number   INTEGER CHECK (season_number >= 0),
    episode_number  INTEGER CHECK (episode_number >= 0),
    absolute_number INTEGER CHECK (absolute_number >= 0),
    -- YYYY-MM-DD.
    air_date        TEXT    CHECK (air_date IS NULL
                                   OR air_date GLOB '[0-9][0-9][0-9][0-9]-[01][0-9]-[0-3][0-9]'),
    title           TEXT,
    runtime_ms      INTEGER CHECK (runtime_ms >= 0),
    UNIQUE (work_id, canonical_ref)
) STRICT;

CREATE INDEX episodes_number ON episodes (work_id, season_number, episode_number);
CREATE INDEX episodes_season ON episodes (season_id) WHERE season_id IS NOT NULL;

CREATE TABLE episode_mappings (
    id             INTEGER PRIMARY KEY,
    episode_id     INTEGER NOT NULL REFERENCES episodes (id) ON DELETE CASCADE,
    scheme         TEXT    NOT NULL CHECK (scheme IN ('tmdb', 'tvdb_aired', 'tvdb_dvd', 'tvdb_absolute',
                                                      'anidb', 'tmdb_group')),
    scheme_ref     TEXT    NOT NULL DEFAULT '',
    -- NULL for absolute schemes.
    season_number  INTEGER CHECK (season_number >= 0),
    episode_number INTEGER NOT NULL CHECK (episode_number >= 0),
    -- Part of a split episode; 0 = whole episode.
    part           INTEGER NOT NULL DEFAULT 0 CHECK (part >= 0),
    -- anime_lists, tvdb, tmdb_group, thexem, user…
    source         TEXT    NOT NULL CHECK (source <> '' AND source NOT GLOB '*[^a-z0-9_]*'),
    confidence     REAL    NOT NULL DEFAULT 1.0 CHECK (confidence BETWEEN 0.0 AND 1.0),
    CHECK ((scheme = 'tmdb_group') = (scheme_ref <> ''))
) STRICT;

CREATE UNIQUE INDEX episode_mappings_unique
    ON episode_mappings (episode_id, scheme, scheme_ref, ifnull(season_number, -1), episode_number, part);
CREATE INDEX episode_mappings_lookup
    ON episode_mappings (scheme, scheme_ref, season_number, episode_number);

CREATE TABLE anime_entries (
    work_id       INTEGER PRIMARY KEY REFERENCES works (id) ON DELETE CASCADE,
    anidb_aid     INTEGER UNIQUE CHECK (anidb_aid > 0),
    -- Links S1/S2/movies of one franchise.
    franchise_id  INTEGER,
    relation_prev INTEGER REFERENCES works (id) ON DELETE SET NULL,
    relation_next INTEGER REFERENCES works (id) ON DELETE SET NULL
) STRICT;

CREATE INDEX anime_entries_franchise ON anime_entries (franchise_id) WHERE franchise_id IS NOT NULL;
CREATE INDEX anime_entries_prev ON anime_entries (relation_prev) WHERE relation_prev IS NOT NULL;
CREATE INDEX anime_entries_next ON anime_entries (relation_next) WHERE relation_next IS NOT NULL;

-- Matches: file -> work ----------------------------------------------------

CREATE TABLE matches (
    id               INTEGER PRIMARY KEY,
    file_id          INTEGER NOT NULL REFERENCES files (id) ON DELETE CASCADE,
    work_id          INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    scheme_used      TEXT    CHECK (scheme_used IN ('tmdb', 'tvdb_aired', 'tvdb_dvd', 'tvdb_absolute',
                                                    'anidb', 'tmdb_group')),
    -- Calibrated probability.
    confidence       REAL    NOT NULL CHECK (confidence BETWEEN 0.0 AND 1.0),
    state            TEXT    NOT NULL
                     CHECK (state IN ('auto', 'auto_review', 'user_confirmed', 'user_set', 'rejected')),
    decided_by       TEXT    NOT NULL
                     CHECK (decided_by IN ('rule', 'hash_crowd', 'hash_anidb', 'hash_os', 'nfo',
                                           'embedded', 'scorer', 'llm', 'user')),
    features_json    TEXT    CHECK (features_json IS NULL OR json_valid(features_json)),
    explanation_json TEXT    CHECK (explanation_json IS NULL OR json_valid(explanation_json)),
    model_version    TEXT,
    created_at_ms    INTEGER NOT NULL,
    updated_at_ms    INTEGER NOT NULL
) STRICT;

-- At most one live (not rejected) match per file; rejected ones are history.
CREATE UNIQUE INDEX matches_live_file ON matches (file_id) WHERE state <> 'rejected';
CREATE INDEX matches_file ON matches (file_id);
CREATE INDEX matches_work ON matches (work_id);

-- Episodes of a match in file order (several for a multi-episode file).
CREATE TABLE match_episodes (
    match_id   INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
    position   INTEGER NOT NULL CHECK (position >= 0),
    episode_id INTEGER NOT NULL REFERENCES episodes (id) ON DELETE CASCADE,
    PRIMARY KEY (match_id, position),
    UNIQUE (match_id, episode_id)
) STRICT;

CREATE INDEX match_episodes_episode ON match_episodes (episode_id);

-- Alternatives shown in the "fix match" UI.
CREATE TABLE match_candidates (
    match_id INTEGER NOT NULL REFERENCES matches (id) ON DELETE CASCADE,
    rank     INTEGER NOT NULL CHECK (rank >= 1),
    work_id  INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    score    REAL    NOT NULL,
    PRIMARY KEY (match_id, rank)
) STRICT;

CREATE INDEX match_candidates_work ON match_candidates (work_id);

-- A user fix that applies to a whole folder.
CREATE TABLE folder_rules (
    id                    INTEGER PRIMARY KEY,
    source_id             INTEGER NOT NULL REFERENCES sources (id) ON DELETE CASCADE,
    path_prefix           TEXT    NOT NULL,
    work_id               INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    scheme                TEXT    CHECK (scheme IN ('tmdb', 'tvdb_aired', 'tvdb_dvd', 'tvdb_absolute',
                                                    'anidb', 'tmdb_group')),
    scheme_ref            TEXT    NOT NULL DEFAULT '',
    episode_offset        INTEGER NOT NULL DEFAULT 0,
    season_override       INTEGER CHECK (season_override >= 0),
    kind_override         TEXT    CHECK (kind_override IN ('movie', 'show', 'anime')),
    created_from_match_id INTEGER REFERENCES matches (id) ON DELETE SET NULL,
    created_at_ms         INTEGER NOT NULL,
    UNIQUE (source_id, path_prefix)
) STRICT;

CREATE INDEX folder_rules_work ON folder_rules (work_id);
CREATE INDEX folder_rules_match ON folder_rules (created_from_match_id)
    WHERE created_from_match_id IS NOT NULL;

-- Provider cache -----------------------------------------------------------

CREATE TABLE provider_cache (
    provider      TEXT    NOT NULL,
    endpoint      TEXT    NOT NULL,
    key           TEXT    NOT NULL,
    payload       BLOB    NOT NULL,
    etag          TEXT,
    fetched_at_ms INTEGER NOT NULL,
    ttl_ms        INTEGER NOT NULL CHECK (ttl_ms >= 0),
    PRIMARY KEY (provider, endpoint, key)
) STRICT;

CREATE INDEX provider_cache_fetched ON provider_cache (fetched_at_ms);

-- Profiles and user state --------------------------------------------------

CREATE TABLE profiles (
    id            INTEGER PRIMARY KEY,
    -- Stable id shared by the devices of one household (sync).
    sync_id       TEXT    NOT NULL UNIQUE CHECK (length(sync_id) BETWEEN 1 AND 64),
    -- '' = default name, localized by the UI.
    name          TEXT    NOT NULL,
    avatar        TEXT,
    is_kid        INTEGER NOT NULL DEFAULT 0 CHECK (is_kid IN (0, 1)),
    position      INTEGER NOT NULL DEFAULT 0,
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

-- The default profile always exists, so progress works without setup.
INSERT INTO profiles (id, sync_id, name, created_at_ms, updated_at_ms)
VALUES (1, 'default', '', CAST(strftime('%s', 'now') AS INTEGER) * 1000,
        CAST(strftime('%s', 'now') AS INTEGER) * 1000);

-- Which episode order a profile sees for a work.
CREATE TABLE display_preferences (
    profile_id INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    work_id    INTEGER NOT NULL REFERENCES works (id) ON DELETE CASCADE,
    scheme     TEXT    NOT NULL CHECK (scheme IN ('tmdb', 'tvdb_aired', 'tvdb_dvd', 'tvdb_absolute',
                                                  'anidb', 'tmdb_group')),
    scheme_ref TEXT    NOT NULL DEFAULT '',
    PRIMARY KEY (profile_id, work_id)
) STRICT;

CREATE INDEX display_preferences_work ON display_preferences (work_id);

-- Playback state (pa-model PlaybackItem), keyed by the stable media key.
CREATE TABLE progress (
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    media_key     TEXT    NOT NULL CHECK (length(media_key) BETWEEN 3 AND 256 AND media_key GLOB '?*:?*'),
    source_type   TEXT    NOT NULL
                  CHECK (source_type IN ('movie', 'episode', 'track', 'twitch_vod', 'twitch_recording',
                                         'twitch_clip', 'video')),
    position_ms   INTEGER NOT NULL CHECK (position_ms >= 0),
    duration_ms   INTEGER CHECK (duration_ms >= 0),
    watched       INTEGER NOT NULL DEFAULT 0 CHECK (watched IN (0, 1)),
    updated_at_ms INTEGER NOT NULL,
    device_id     TEXT    NOT NULL,
    PRIMARY KEY (profile_id, media_key)
) STRICT;

-- "Continue watching": latest first.
CREATE INDEX progress_recent ON progress (profile_id, updated_at_ms DESC);

-- A favorite is either a playable item (media_key) or a work (show, movie).
CREATE TABLE favorites (
    id            INTEGER PRIMARY KEY,
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    media_key     TEXT    CHECK (length(media_key) BETWEEN 3 AND 256 AND media_key GLOB '?*:?*'),
    work_id       INTEGER REFERENCES works (id) ON DELETE CASCADE,
    created_at_ms INTEGER NOT NULL,
    CHECK ((media_key IS NULL) <> (work_id IS NULL))
) STRICT;

CREATE UNIQUE INDEX favorites_media ON favorites (profile_id, media_key) WHERE media_key IS NOT NULL;
CREATE UNIQUE INDEX favorites_work ON favorites (profile_id, work_id) WHERE work_id IS NOT NULL;
CREATE INDEX favorites_work_ref ON favorites (work_id) WHERE work_id IS NOT NULL;
CREATE INDEX favorites_profile ON favorites (profile_id, created_at_ms DESC);

-- Collections: manual, smart (saved filter) or imported (TMDB collection).
CREATE TABLE collections (
    id            INTEGER PRIMARY KEY,
    -- NULL = visible to every profile.
    profile_id    INTEGER REFERENCES profiles (id) ON DELETE CASCADE,
    name          TEXT    NOT NULL CHECK (name <> ''),
    kind          TEXT    NOT NULL CHECK (kind IN ('manual', 'smart', 'external')),
    -- Source of an external collection, e.g. `tmdb:collection:10`.
    external_ref  TEXT,
    rule_json     TEXT    CHECK (rule_json IS NULL OR json_valid(rule_json)),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL,
    CHECK ((kind = 'smart') = (rule_json IS NOT NULL)),
    CHECK ((kind = 'external') = (external_ref IS NOT NULL))
) STRICT;

CREATE INDEX collections_profile ON collections (profile_id) WHERE profile_id IS NOT NULL;
CREATE UNIQUE INDEX collections_external ON collections (external_ref) WHERE external_ref IS NOT NULL;

CREATE TABLE collection_items (
    id            INTEGER PRIMARY KEY,
    collection_id INTEGER NOT NULL REFERENCES collections (id) ON DELETE CASCADE,
    position      INTEGER NOT NULL,
    work_id       INTEGER REFERENCES works (id) ON DELETE CASCADE,
    media_key     TEXT    CHECK (length(media_key) BETWEEN 3 AND 256 AND media_key GLOB '?*:?*'),
    added_at_ms   INTEGER NOT NULL,
    CHECK ((media_key IS NULL) <> (work_id IS NULL))
) STRICT;

CREATE INDEX collection_items_order ON collection_items (collection_id, position);
CREATE UNIQUE INDEX collection_items_work ON collection_items (collection_id, work_id)
    WHERE work_id IS NOT NULL;
CREATE UNIQUE INDEX collection_items_media ON collection_items (collection_id, media_key)
    WHERE media_key IS NOT NULL;
CREATE INDEX collection_items_work_ref ON collection_items (work_id) WHERE work_id IS NOT NULL;

-- Playlists of playable items (music first, video P1).
CREATE TABLE playlists (
    id            INTEGER PRIMARY KEY,
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    name          TEXT    NOT NULL CHECK (name <> ''),
    kind          TEXT    NOT NULL CHECK (kind IN ('audio', 'video')),
    created_at_ms INTEGER NOT NULL,
    updated_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX playlists_profile ON playlists (profile_id);

CREATE TABLE playlist_items (
    id          INTEGER PRIMARY KEY,
    playlist_id INTEGER NOT NULL REFERENCES playlists (id) ON DELETE CASCADE,
    position    INTEGER NOT NULL,
    media_key   TEXT    NOT NULL CHECK (length(media_key) BETWEEN 3 AND 256 AND media_key GLOB '?*:?*'),
    added_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX playlist_items_order ON playlist_items (playlist_id, position);

-- Persistent job queue (pa-jobs, CORE-011; 04 §1.10) ----------------------

CREATE TABLE jobs (
    id                INTEGER PRIMARY KEY,
    -- library_scan, metadata_refresh, offline_copy, transcode_optimize, cleanup, twitch_*…
    type              TEXT    NOT NULL CHECK (type <> '' AND type NOT GLOB '*[^a-z0-9_]*'),
    owner_profile_id  INTEGER REFERENCES profiles (id) ON DELETE SET NULL,
    target_storage    TEXT,
    priority          INTEGER NOT NULL DEFAULT 0,
    state             TEXT    NOT NULL DEFAULT 'queued'
                      CHECK (state IN ('queued', 'waiting_resources', 'running', 'paused', 'retrying',
                                       'done', 'failed', 'cancelled')),
    progress          REAL    CHECK (progress BETWEEN 0.0 AND 1.0),
    bytes_done        INTEGER NOT NULL DEFAULT 0 CHECK (bytes_done >= 0),
    bytes_total       INTEGER CHECK (bytes_total >= 0),
    attempts          INTEGER NOT NULL DEFAULT 0 CHECK (attempts >= 0),
    next_run_at_ms    INTEGER,
    params_json       TEXT    NOT NULL DEFAULT '{}' CHECK (json_valid(params_json)),
    result_json       TEXT    CHECK (result_json IS NULL OR json_valid(result_json)),
    error             TEXT,
    created_by_device TEXT,
    created_at_ms     INTEGER NOT NULL,
    updated_at_ms     INTEGER NOT NULL
) STRICT;

CREATE INDEX jobs_pending ON jobs (state, priority DESC, next_run_at_ms)
    WHERE state IN ('queued', 'waiting_resources', 'retrying', 'paused', 'running');
CREATE INDEX jobs_owner ON jobs (owner_profile_id) WHERE owner_profile_id IS NOT NULL;
