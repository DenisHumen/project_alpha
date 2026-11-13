-- SPDX-License-Identifier: MPL-2.0
--
-- MUS-001: music library (docs/dev/db.md "Музыка"). Expand-only migration.
--
-- One audio file = one track. Tags are read by pa-scanner (`index_music`)
-- and written through `pa_library::music`. Artists, albums and genres are
-- shared by all sources; rows left without tracks are removed by
-- `music::remove_orphans`. Names are matched by a case- and space-folded key.

CREATE TABLE music_artists (
    id            INTEGER PRIMARY KEY,
    name          TEXT    NOT NULL CHECK (name <> ''),
    sort_name     TEXT,
    -- Folded name (lowercase, single spaces): one artist per key.
    name_key      TEXT    NOT NULL UNIQUE CHECK (name_key <> ''),
    -- MusicBrainz artist id, lowercase UUID.
    mbid          TEXT    CHECK (mbid IS NULL OR (length(mbid) = 36 AND mbid NOT GLOB '*[^0-9a-f-]*')),
    created_at_ms INTEGER NOT NULL
) STRICT;

CREATE TABLE music_albums (
    id                 INTEGER PRIMARY KEY,
    -- Grouping key: `mb:<release mbid>`, `aa:<album artist>|<title>` or,
    -- without an album artist tag, `dir:<source id>:<folder>|<title>`.
    album_key          TEXT    NOT NULL UNIQUE CHECK (album_key <> ''),
    title              TEXT    NOT NULL CHECK (title <> ''),
    year               INTEGER CHECK (year BETWEEN 1000 AND 3000),
    -- Various artists: a compilation tag or several track artists.
    compilation        INTEGER NOT NULL DEFAULT 0 CHECK (compilation IN (0, 1)),
    disc_count         INTEGER CHECK (disc_count > 0),
    release_mbid       TEXT    CHECK (release_mbid IS NULL
                                      OR (length(release_mbid) = 36 AND release_mbid NOT GLOB '*[^0-9a-f-]*')),
    release_group_mbid TEXT    CHECK (release_group_mbid IS NULL
                                      OR (length(release_group_mbid) = 36
                                          AND release_group_mbid NOT GLOB '*[^0-9a-f-]*')),
    -- Cover: the embedded picture of a track file or an image in the
    -- album folder (`cover.jpg`, `folder.jpg`…), by source and path.
    art_kind           TEXT    CHECK (art_kind IN ('embedded', 'folder')),
    art_source_id      INTEGER REFERENCES sources (id) ON DELETE SET NULL,
    art_path           TEXT,
    created_at_ms      INTEGER NOT NULL,
    updated_at_ms      INTEGER NOT NULL,
    CHECK ((art_kind IS NULL) = (art_path IS NULL))
) STRICT;

CREATE INDEX music_albums_art_source ON music_albums (art_source_id) WHERE art_source_id IS NOT NULL;
CREATE INDEX music_albums_title ON music_albums (title COLLATE NOCASE);

CREATE TABLE music_album_artists (
    album_id  INTEGER NOT NULL REFERENCES music_albums (id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES music_artists (id) ON DELETE CASCADE,
    position  INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (album_id, artist_id)
) STRICT;

CREATE INDEX music_album_artists_artist ON music_album_artists (artist_id);

CREATE TABLE music_tracks (
    id                 INTEGER PRIMARY KEY,
    file_id            INTEGER NOT NULL UNIQUE REFERENCES files (id) ON DELETE CASCADE,
    album_id           INTEGER REFERENCES music_albums (id) ON DELETE SET NULL,
    title              TEXT    NOT NULL CHECK (title <> ''),
    disc_number        INTEGER CHECK (disc_number > 0),
    disc_total         INTEGER CHECK (disc_total > 0),
    track_number       INTEGER CHECK (track_number > 0),
    track_total        INTEGER CHECK (track_total > 0),
    year               INTEGER CHECK (year BETWEEN 1000 AND 3000),
    -- Album grouping input as read: album artist tag and compilation flag.
    album_artist_tag   TEXT,
    compilation_tag    INTEGER NOT NULL DEFAULT 0 CHECK (compilation_tag IN (0, 1)),
    -- MusicBrainz recording id (sync key `mb:recording:`), lowercase UUID.
    recording_mbid     TEXT    CHECK (recording_mbid IS NULL
                                      OR (length(recording_mbid) = 36 AND recording_mbid NOT GLOB '*[^0-9a-f-]*')),
    duration_ms        INTEGER CHECK (duration_ms >= 0),
    -- mp3, aac, alac, flac, opus, vorbis, pcm, dsd, ape, wavpack…
    codec              TEXT    CHECK (codec IS NULL OR (codec <> '' AND codec NOT GLOB '*[^a-z0-9_]*')),
    lossless           INTEGER CHECK (lossless IN (0, 1)),
    sample_rate_hz     INTEGER CHECK (sample_rate_hz > 0),
    bit_depth          INTEGER CHECK (bit_depth > 0),
    channels           INTEGER CHECK (channels > 0),
    bitrate_kbps       INTEGER CHECK (bitrate_kbps > 0),
    -- ReplayGain 2.0: gain in dB, peak as a linear sample value. Opus
    -- R128_*_GAIN tags are converted to the ReplayGain reference (-18 LUFS).
    rg_track_gain_db   REAL,
    rg_track_peak      REAL    CHECK (rg_track_peak >= 0),
    rg_album_gain_db   REAL,
    rg_album_peak      REAL    CHECK (rg_album_peak >= 0),
    has_embedded_art   INTEGER NOT NULL DEFAULT 0 CHECK (has_embedded_art IN (0, 1)),
    -- 1: the tags could not be read; title and album come from the path.
    tags_failed        INTEGER NOT NULL DEFAULT 0 CHECK (tags_failed IN (0, 1)),
    -- `files.size_bytes` and `files.mtime_ms` when the tags were read: a
    -- different value means the file changed and is read again.
    tagged_size_bytes  INTEGER NOT NULL CHECK (tagged_size_bytes >= 0),
    tagged_mtime_ms    INTEGER,
    tagged_at_ms       INTEGER NOT NULL
) STRICT;

CREATE INDEX music_tracks_album ON music_tracks (album_id, disc_number, track_number);
CREATE INDEX music_tracks_recording ON music_tracks (recording_mbid) WHERE recording_mbid IS NOT NULL;

CREATE TABLE music_track_artists (
    track_id  INTEGER NOT NULL REFERENCES music_tracks (id) ON DELETE CASCADE,
    artist_id INTEGER NOT NULL REFERENCES music_artists (id) ON DELETE CASCADE,
    role      TEXT    NOT NULL CHECK (role IN ('artist', 'composer')),
    position  INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (track_id, role, artist_id)
) STRICT;

CREATE INDEX music_track_artists_artist ON music_track_artists (artist_id, role);

CREATE TABLE music_genres (
    id       INTEGER PRIMARY KEY,
    name     TEXT    NOT NULL CHECK (name <> ''),
    name_key TEXT    NOT NULL UNIQUE CHECK (name_key <> '')
) STRICT;

CREATE TABLE music_track_genres (
    track_id INTEGER NOT NULL REFERENCES music_tracks (id) ON DELETE CASCADE,
    genre_id INTEGER NOT NULL REFERENCES music_genres (id) ON DELETE CASCADE,
    position INTEGER NOT NULL CHECK (position >= 0),
    PRIMARY KEY (track_id, genre_id)
) STRICT;

CREATE INDEX music_track_genres_genre ON music_track_genres (genre_id);
