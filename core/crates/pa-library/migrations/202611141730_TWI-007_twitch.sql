-- SPDX-License-Identifier: MPL-2.0
--
-- TWI-007: Twitch in the library (docs/dev/db.md "Twitch", docs/research/06
-- §5). Expand-only migration.
--
-- Twitch ids are opaque strings, numeric today; like pa-model keys we keep
-- them as text in canonical decimal form. Metadata from Helix is a cache:
-- the Developer Agreement allows at most 24 h (docs/legal/providers.md
-- §2.4), so every metadata row has `fetched_at_ms` and
-- `pa_library::twitch::purge_expired` deletes stale rows unless the user
-- saved them (favourite channel, downloaded file, recording, record rule).
-- User data (favourites, record rules, downloads, chat logs) is kept.

CREATE TABLE twitch_streamers (
    id                   INTEGER PRIMARY KEY,
    user_id              TEXT    NOT NULL UNIQUE
                                 CHECK (user_id <> '' AND user_id NOT GLOB '*[^0-9]*' AND user_id NOT GLOB '0*'),
    -- Logins can be renamed and reused, so not unique.
    login                TEXT    NOT NULL CHECK (login <> ''),
    display_name         TEXT    NOT NULL CHECK (display_name <> ''),
    broadcaster_type     TEXT    NOT NULL DEFAULT 'normal'
                                 CHECK (broadcaster_type IN ('partner', 'affiliate', 'normal')),
    description          TEXT,
    avatar_url           TEXT,
    -- Avatar copied to the app cache (path on this device, not synced).
    avatar_local         TEXT,
    offline_banner_url   TEXT,
    last_seen_live_at_ms INTEGER,
    -- When the metadata last came from Twitch: base of the 24 h cache cap.
    fetched_at_ms        INTEGER NOT NULL,
    updated_at_ms        INTEGER NOT NULL
) STRICT;

CREATE INDEX twitch_streamers_login ON twitch_streamers (login);

CREATE TABLE twitch_categories (
    id            INTEGER PRIMARY KEY,
    game_id       TEXT    NOT NULL UNIQUE
                          CHECK (game_id <> '' AND game_id NOT GLOB '*[^0-9]*' AND game_id NOT GLOB '0*'),
    name          TEXT    NOT NULL CHECK (name <> ''),
    box_art_url   TEXT,
    igdb_id       TEXT,
    fetched_at_ms INTEGER NOT NULL
) STRICT;

-- Favourite channels of a profile: local favourites (no Twitch login) and
-- imported Twitch follows. Synced as `twitch_follow.set`. Not a foreign key
-- to `twitch_streamers`: a synced favourite may arrive before its metadata.
CREATE TABLE twitch_favorites (
    profile_id    INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    user_id       TEXT    NOT NULL
                          CHECK (user_id <> '' AND user_id NOT GLOB '*[^0-9]*' AND user_id NOT GLOB '0*'),
    source        TEXT    NOT NULL DEFAULT 'local' CHECK (source IN ('local', 'twitch_follow')),
    -- "X is live" notifications (TWI-009).
    notify        INTEGER NOT NULL DEFAULT 1 CHECK (notify IN (0, 1)),
    added_at_ms   INTEGER NOT NULL,
    PRIMARY KEY (profile_id, user_id)
) STRICT;

CREATE INDEX twitch_favorites_user ON twitch_favorites (user_id);

-- A broadcast. `video_id` is its archive VOD once known.
CREATE TABLE twitch_live_sessions (
    id            INTEGER PRIMARY KEY,
    stream_id     TEXT    NOT NULL UNIQUE
                          CHECK (stream_id <> '' AND stream_id NOT GLOB '*[^0-9]*' AND stream_id NOT GLOB '0*'),
    user_id       TEXT    NOT NULL CHECK (user_id <> ''),
    started_at_ms INTEGER NOT NULL,
    ended_at_ms   INTEGER CHECK (ended_at_ms IS NULL OR ended_at_ms >= started_at_ms),
    title_last    TEXT,
    peak_viewers  INTEGER CHECK (peak_viewers >= 0),
    video_id      TEXT,
    fetched_at_ms INTEGER NOT NULL
) STRICT;

CREATE INDEX twitch_live_sessions_user ON twitch_live_sessions (user_id, started_at_ms);
CREATE INDEX twitch_live_sessions_video ON twitch_live_sessions (video_id) WHERE video_id IS NOT NULL;

-- VOD: archive, highlight or upload (pa_model::Video). Sync key
-- `twitch:vod:<video_id>`, the same online and for a downloaded copy.
CREATE TABLE twitch_videos (
    id                  INTEGER PRIMARY KEY,
    video_id            TEXT    NOT NULL UNIQUE
                                CHECK (video_id <> '' AND video_id NOT GLOB '*[^0-9]*' AND video_id NOT GLOB '0*'),
    user_id             TEXT    NOT NULL CHECK (user_id <> ''),
    stream_id           TEXT,
    video_type          TEXT    NOT NULL CHECK (video_type IN ('archive', 'highlight', 'upload')),
    title               TEXT    NOT NULL,
    published_at_ms     INTEGER NOT NULL,
    duration_ms         INTEGER NOT NULL CHECK (duration_ms >= 0),
    view_count          INTEGER NOT NULL DEFAULT 0 CHECK (view_count >= 0),
    thumbnail_url       TEXT,
    -- [{"offset_ms":…,"duration_ms":…}]
    muted_segments_json TEXT    NOT NULL DEFAULT '[]' CHECK (json_valid(muted_segments_json)),
    is_sub_only         INTEGER CHECK (is_sub_only IN (0, 1)),
    availability        TEXT    NOT NULL DEFAULT 'unknown'
                                CHECK (availability IN ('online', 'deleted', 'unknown')),
    last_checked_at_ms  INTEGER,
    fetched_at_ms       INTEGER NOT NULL
) STRICT;

CREATE INDEX twitch_videos_user ON twitch_videos (user_id, published_at_ms);
CREATE INDEX twitch_videos_stream ON twitch_videos (stream_id) WHERE stream_id IS NOT NULL;

-- Chapters: which category was on air when, for a VOD or a live session.
CREATE TABLE twitch_category_segments (
    id            INTEGER PRIMARY KEY,
    video_id      TEXT    REFERENCES twitch_videos (video_id) ON DELETE CASCADE,
    stream_id     TEXT    REFERENCES twitch_live_sessions (stream_id) ON DELETE CASCADE,
    game_id       TEXT    NOT NULL CHECK (game_id <> ''),
    title_at_time TEXT,
    start_ms      INTEGER NOT NULL CHECK (start_ms >= 0),
    -- NULL while the segment is still on air.
    end_ms        INTEGER CHECK (end_ms IS NULL OR end_ms >= start_ms),
    -- eventsub, gql_moments, fallback (≈ current category)…
    source        TEXT    NOT NULL CHECK (source <> '' AND source NOT GLOB '*[^a-z0-9_]*'),
    CHECK ((video_id IS NULL) <> (stream_id IS NULL))
) STRICT;

CREATE INDEX twitch_category_segments_video ON twitch_category_segments (video_id, start_ms)
    WHERE video_id IS NOT NULL;
CREATE INDEX twitch_category_segments_stream ON twitch_category_segments (stream_id, start_ms)
    WHERE stream_id IS NOT NULL;
CREATE INDEX twitch_category_segments_game ON twitch_category_segments (game_id);

CREATE TABLE twitch_clips (
    id              INTEGER PRIMARY KEY,
    slug            TEXT    NOT NULL UNIQUE CHECK (slug <> '' AND slug NOT GLOB '*[^A-Za-z0-9_-]*'),
    broadcaster_id  TEXT    NOT NULL CHECK (broadcaster_id <> ''),
    creator_name    TEXT,
    video_id        TEXT,
    vod_offset_ms   INTEGER CHECK (vod_offset_ms >= 0),
    game_id         TEXT,
    title           TEXT    NOT NULL,
    created_at_ms   INTEGER NOT NULL,
    duration_ms     INTEGER NOT NULL CHECK (duration_ms >= 0),
    thumbnail_url   TEXT,
    view_count      INTEGER NOT NULL DEFAULT 0 CHECK (view_count >= 0),
    fetched_at_ms   INTEGER NOT NULL
) STRICT;

CREATE INDEX twitch_clips_broadcaster ON twitch_clips (broadcaster_id, created_at_ms);

-- A saved Twitch file: a downloaded VOD or clip, or our own live recording.
-- The file itself is a row of `files`; its keys: `twitch:vod:<video_id>`,
-- `twitch:rec:<stream_id>`, `twitch:clip:<slug>`.
CREATE TABLE twitch_assets (
    id                   INTEGER PRIMARY KEY,
    file_id              INTEGER NOT NULL UNIQUE REFERENCES files (id) ON DELETE CASCADE,
    kind                 TEXT    NOT NULL CHECK (kind IN ('vod', 'recording', 'clip')),
    user_id              TEXT    NOT NULL CHECK (user_id <> ''),
    video_id             TEXT,
    stream_id            TEXT,
    clip_slug            TEXT,
    -- Title as saved with the file (also in its TWITCH_* tags).
    title                TEXT    NOT NULL,
    recorded_at_ms       INTEGER,
    duration_ms          INTEGER CHECK (duration_ms >= 0),
    quality              TEXT,
    has_ads_marked       INTEGER NOT NULL DEFAULT 0 CHECK (has_ads_marked IN (0, 1)),
    -- Recording → VOD: position in the VOD = position in the recording + offset.
    offset_rec_to_vod_ms INTEGER,
    -- server, desktop, import…
    origin               TEXT    NOT NULL CHECK (origin <> '' AND origin NOT GLOB '*[^a-z0-9_]*'),
    created_at_ms        INTEGER NOT NULL,
    CHECK (kind <> 'vod' OR video_id IS NOT NULL),
    CHECK (kind <> 'recording' OR stream_id IS NOT NULL),
    CHECK (kind <> 'clip' OR clip_slug IS NOT NULL),
    CHECK (offset_rec_to_vod_ms IS NULL OR (kind = 'recording' AND video_id IS NOT NULL))
) STRICT;

CREATE INDEX twitch_assets_video ON twitch_assets (video_id) WHERE video_id IS NOT NULL;
CREATE INDEX twitch_assets_stream ON twitch_assets (stream_id) WHERE stream_id IS NOT NULL;
CREATE INDEX twitch_assets_clip ON twitch_assets (clip_slug) WHERE clip_slug IS NOT NULL;
CREATE INDEX twitch_assets_user ON twitch_assets (user_id);

-- Chat sidecar of a saved file (`.chat.json.gz`, TwitchDownloader JSON).
-- `path` is relative to the root of the source of the asset's file.
CREATE TABLE twitch_chat_logs (
    id              INTEGER PRIMARY KEY,
    asset_id        INTEGER NOT NULL UNIQUE REFERENCES twitch_assets (id) ON DELETE CASCADE,
    path            TEXT    NOT NULL CHECK (path <> ''),
    format          TEXT    NOT NULL DEFAULT 'tdl_json_v1'
                            CHECK (format <> '' AND format NOT GLOB '*[^a-z0-9_]*'),
    message_count   INTEGER NOT NULL DEFAULT 0 CHECK (message_count >= 0),
    first_offset_ms INTEGER CHECK (first_offset_ms >= 0),
    last_offset_ms  INTEGER CHECK (last_offset_ms >= first_offset_ms),
    emote_snapshot  INTEGER NOT NULL DEFAULT 0 CHECK (emote_snapshot IN (0, 1)),
    created_at_ms   INTEGER NOT NULL
) STRICT;

-- Automatic recording of a channel (full builds, TWI-018).
CREATE TABLE twitch_record_rules (
    id                     INTEGER PRIMARY KEY,
    profile_id             INTEGER NOT NULL REFERENCES profiles (id) ON DELETE CASCADE,
    user_id                TEXT    NOT NULL
                                   CHECK (user_id <> '' AND user_id NOT GLOB '*[^0-9]*' AND user_id NOT GLOB '0*'),
    quality                TEXT    NOT NULL DEFAULT 'best' CHECK (quality <> ''),
    with_chat              INTEGER NOT NULL DEFAULT 1 CHECK (with_chat IN (0, 1)),
    retention_days         INTEGER CHECK (retention_days > 0),
    keep_last_n            INTEGER CHECK (keep_last_n > 0),
    -- JSON array of game ids; NULL records every category.
    categories_filter_json TEXT    CHECK (categories_filter_json IS NULL
                                          OR (json_valid(categories_filter_json)
                                              AND json_type(categories_filter_json) = 'array')),
    replace_with_vod       INTEGER NOT NULL DEFAULT 0 CHECK (replace_with_vod IN (0, 1)),
    enabled                INTEGER NOT NULL DEFAULT 1 CHECK (enabled IN (0, 1)),
    created_at_ms          INTEGER NOT NULL,
    updated_at_ms          INTEGER NOT NULL,
    UNIQUE (profile_id, user_id)
) STRICT;

CREATE INDEX twitch_record_rules_user ON twitch_record_rules (user_id);

-- Full-text search over streamers, VODs, categories, clips and live
-- sessions. rowid = local id * 8 + kind (0 streamer, 1 video, 2 category,
-- 3 clip, 4 live session); kept up to date by the triggers below.
CREATE VIRTUAL TABLE twitch_search USING fts5 (
    text,
    tokenize = 'unicode61 remove_diacritics 2',
    prefix = '2 3'
);

CREATE TRIGGER twitch_streamers_ai AFTER INSERT ON twitch_streamers BEGIN
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8, new.display_name || ' ' || new.login);
END;
CREATE TRIGGER twitch_streamers_au AFTER UPDATE OF display_name, login ON twitch_streamers BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8;
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8, new.display_name || ' ' || new.login);
END;
CREATE TRIGGER twitch_streamers_ad AFTER DELETE ON twitch_streamers BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8;
END;

CREATE TRIGGER twitch_videos_ai AFTER INSERT ON twitch_videos BEGIN
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 1, new.title);
END;
CREATE TRIGGER twitch_videos_au AFTER UPDATE OF title ON twitch_videos BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 1;
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 1, new.title);
END;
CREATE TRIGGER twitch_videos_ad AFTER DELETE ON twitch_videos BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 1;
END;

CREATE TRIGGER twitch_categories_ai AFTER INSERT ON twitch_categories BEGIN
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 2, new.name);
END;
CREATE TRIGGER twitch_categories_au AFTER UPDATE OF name ON twitch_categories BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 2;
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 2, new.name);
END;
CREATE TRIGGER twitch_categories_ad AFTER DELETE ON twitch_categories BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 2;
END;

CREATE TRIGGER twitch_clips_ai AFTER INSERT ON twitch_clips BEGIN
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 3, new.title);
END;
CREATE TRIGGER twitch_clips_au AFTER UPDATE OF title ON twitch_clips BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 3;
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 3, new.title);
END;
CREATE TRIGGER twitch_clips_ad AFTER DELETE ON twitch_clips BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 3;
END;

CREATE TRIGGER twitch_live_sessions_ai AFTER INSERT ON twitch_live_sessions
    WHEN new.title_last IS NOT NULL BEGIN
    INSERT INTO twitch_search (rowid, text) VALUES (new.id * 8 + 4, new.title_last);
END;
CREATE TRIGGER twitch_live_sessions_au AFTER UPDATE OF title_last ON twitch_live_sessions BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 4;
    INSERT INTO twitch_search (rowid, text)
        SELECT new.id * 8 + 4, new.title_last WHERE new.title_last IS NOT NULL;
END;
CREATE TRIGGER twitch_live_sessions_ad AFTER DELETE ON twitch_live_sessions BEGIN
    DELETE FROM twitch_search WHERE rowid = old.id * 8 + 4;
END;
