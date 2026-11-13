-- SPDX-License-Identifier: MPL-2.0
--
-- LIB-008: library search (FTS5). Expand-only migration.
--
-- Search keys are computed in Rust (pa_recognition::title::normalize: case
-- and diacritics folding, roman numerals, Cyrillic transliteration into one
-- loose Latin space), so SQL triggers cannot fill the index themselves.
-- Instead they put the changed entity into `search_queue`;
-- `pa_library::search::sync` (also called by `search::search`) rebuilds the
-- documents of queued entities. Any connection may write the catalogue
-- without knowing about search: the index catches up on the next sync.

-- One row per searchable string (a title, an alias, a file name) of an
-- entity. `entity` + `ref_id` point at the source row (polymorphic, so no
-- foreign key); `kind` is the result group. Open sets: lowercase slugs.
CREATE TABLE search_docs (
    id        INTEGER PRIMARY KEY,
    entity    TEXT    NOT NULL CHECK (entity <> '' AND entity NOT GLOB '*[^a-z0-9_]*'),
    ref_id    INTEGER NOT NULL,
    kind      TEXT    NOT NULL CHECK (kind <> '' AND kind NOT GLOB '*[^a-z0-9_]*'),
    -- The original string, as shown when it is the matched alias.
    text      TEXT    NOT NULL,
    -- Folded tokens (numerals arabic), space separated.
    plain_key TEXT    NOT NULL,
    -- Loose Latin tokens, space separated.
    loose_key TEXT    NOT NULL,
    -- Relative weight of the string within its entity (main title 1.0).
    weight    REAL    NOT NULL CHECK (weight > 0.0 AND weight <= 1.0)
) STRICT;

CREATE INDEX search_docs_entity ON search_docs (entity, ref_id);
CREATE INDEX search_docs_plain ON search_docs (plain_key);
CREATE INDEX search_docs_loose ON search_docs (loose_key);

-- Full-text index over search_docs (rowid = search_docs.id). Contentless:
-- the text lives in search_docs. Tokens are already folded by Rust, so the
-- tokenizer only splits on spaces and keeps marks inside tokens.
CREATE VIRTUAL TABLE search_fts USING fts5(
    plain,
    loose,
    content = '',
    contentless_delete = 1,
    tokenize = "unicode61 remove_diacritics 0 categories 'L* N* Co M*'",
    prefix = '2 3'
);

-- Entities whose documents must be rebuilt.
CREATE TABLE search_queue (
    entity TEXT    NOT NULL CHECK (entity <> '' AND entity NOT GLOB '*[^a-z0-9_]*'),
    ref_id INTEGER NOT NULL,
    PRIMARY KEY (entity, ref_id)
) STRICT, WITHOUT ROWID;

-- Works and their titles ------------------------------------------------------

CREATE TRIGGER search_works_ins AFTER INSERT ON works BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('work', new.id);
END;
CREATE TRIGGER search_works_upd AFTER UPDATE OF id, kind, title, original_title ON works BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('work', old.id), ('work', new.id);
END;
CREATE TRIGGER search_works_del AFTER DELETE ON works BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('work', old.id);
END;

CREATE TRIGGER search_titles_ins AFTER INSERT ON titles BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('work', new.work_id);
END;
CREATE TRIGGER search_titles_upd AFTER UPDATE ON titles BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('work', old.work_id), ('work', new.work_id);
END;
CREATE TRIGGER search_titles_del AFTER DELETE ON titles BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('work', old.work_id);
END;

-- Episodes ----------------------------------------------------------------------

CREATE TRIGGER search_episodes_ins AFTER INSERT ON episodes BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('episode', new.id);
END;
CREATE TRIGGER search_episodes_upd AFTER UPDATE OF id, title ON episodes BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('episode', old.id), ('episode', new.id);
END;
CREATE TRIGGER search_episodes_del AFTER DELETE ON episodes BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('episode', old.id);
END;

-- Unrecognised video files: indexed while they have no live match and no
-- music track. ------------------------------------------------------------------

CREATE TRIGGER search_files_ins AFTER INSERT ON files BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('file', new.id);
END;
CREATE TRIGGER search_files_upd AFTER UPDATE OF id, path, missing_since_ms ON files BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('file', old.id), ('file', new.id);
END;
CREATE TRIGGER search_files_del AFTER DELETE ON files BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('file', old.id);
END;

CREATE TRIGGER search_matches_ins AFTER INSERT ON matches BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('file', new.file_id);
END;
CREATE TRIGGER search_matches_upd AFTER UPDATE OF file_id, state ON matches BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('file', old.file_id), ('file', new.file_id);
END;
CREATE TRIGGER search_matches_del AFTER DELETE ON matches BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('file', old.file_id);
END;

-- Music ---------------------------------------------------------------------------

CREATE TRIGGER search_artists_ins AFTER INSERT ON music_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('artist', new.id);
END;
CREATE TRIGGER search_artists_upd AFTER UPDATE OF id, name, sort_name ON music_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('artist', old.id), ('artist', new.id);
    -- Album and track documents include the artist name.
    INSERT OR IGNORE INTO search_queue
        SELECT 'album', album_id FROM music_album_artists WHERE artist_id = new.id;
    INSERT OR IGNORE INTO search_queue
        SELECT 'track', track_id FROM music_track_artists WHERE artist_id = new.id;
END;
CREATE TRIGGER search_artists_del AFTER DELETE ON music_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('artist', old.id);
END;

CREATE TRIGGER search_albums_ins AFTER INSERT ON music_albums BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('album', new.id);
END;
CREATE TRIGGER search_albums_upd AFTER UPDATE OF id, title ON music_albums BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('album', old.id), ('album', new.id);
END;
CREATE TRIGGER search_albums_del AFTER DELETE ON music_albums BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('album', old.id);
END;

CREATE TRIGGER search_album_artists_ins AFTER INSERT ON music_album_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('album', new.album_id);
END;
CREATE TRIGGER search_album_artists_upd AFTER UPDATE ON music_album_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('album', old.album_id), ('album', new.album_id);
END;
CREATE TRIGGER search_album_artists_del AFTER DELETE ON music_album_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('album', old.album_id);
END;

CREATE TRIGGER search_tracks_ins AFTER INSERT ON music_tracks BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('track', new.id), ('file', new.file_id);
END;
CREATE TRIGGER search_tracks_upd AFTER UPDATE OF id, file_id, title ON music_tracks BEGIN
    INSERT OR IGNORE INTO search_queue
        VALUES ('track', old.id), ('track', new.id), ('file', old.file_id), ('file', new.file_id);
END;
CREATE TRIGGER search_tracks_del AFTER DELETE ON music_tracks BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('track', old.id), ('file', old.file_id);
END;

CREATE TRIGGER search_track_artists_ins AFTER INSERT ON music_track_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('track', new.track_id);
END;
CREATE TRIGGER search_track_artists_upd AFTER UPDATE ON music_track_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('track', old.track_id), ('track', new.track_id);
END;
CREATE TRIGGER search_track_artists_del AFTER DELETE ON music_track_artists BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('track', old.track_id);
END;

-- Collections ---------------------------------------------------------------------

CREATE TRIGGER search_collections_ins AFTER INSERT ON collections BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('collection', new.id);
END;
CREATE TRIGGER search_collections_upd AFTER UPDATE OF id, name ON collections BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('collection', old.id), ('collection', new.id);
END;
CREATE TRIGGER search_collections_del AFTER DELETE ON collections BEGIN
    INSERT OR IGNORE INTO search_queue VALUES ('collection', old.id);
END;

-- Existing rows: index them on the first sync.
INSERT OR IGNORE INTO search_queue
    SELECT 'work', id FROM works
    UNION ALL SELECT 'episode', id FROM episodes
    UNION ALL SELECT 'file', id FROM files
    UNION ALL SELECT 'artist', id FROM music_artists
    UNION ALL SELECT 'album', id FROM music_albums
    UNION ALL SELECT 'track', id FROM music_tracks
    UNION ALL SELECT 'collection', id FROM collections;
