// SPDX-License-Identifier: MPL-2.0

import 'package:sqlite3/sqlite3.dart';

import 'models.dart';
import 'tmdb.dart';

const int schemaVersion = 1;

/// TMDB data older than this is dropped (terms: at most 6 months).
const Duration tmdbDataMaxAge = Duration(days: 180);

void migrate(Database db) {
  db.execute('PRAGMA foreign_keys = ON');
  db.execute('PRAGMA journal_mode = WAL');
  if (db.userVersion >= schemaVersion) return;
  db.execute('''
CREATE TABLE IF NOT EXISTS sources (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  name TEXT NOT NULL,
  path TEXT NOT NULL,
  username TEXT,
  position INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS items (
  id TEXT PRIMARY KEY,
  kind TEXT NOT NULL,
  source_id TEXT,
  title TEXT NOT NULL,
  original_title TEXT,
  year INTEGER,
  overview TEXT,
  poster_url TEXT,
  backdrop_url TEXT,
  show_id TEXT,
  season INTEGER,
  episode INTEGER,
  episode_end INTEGER,
  artist TEXT,
  album TEXT,
  track_number INTEGER,
  file_path TEXT NOT NULL,
  file_size INTEGER,
  file_mtime INTEGER,
  duration_ms INTEGER,
  match_confidence REAL,
  parsed_title TEXT,
  parsed_alt_title TEXT,
  parsed_year INTEGER,
  local_poster TEXT,
  tmdb_id INTEGER,
  meta_fetched_at INTEGER,
  locked INTEGER NOT NULL DEFAULT 0,
  added_at INTEGER NOT NULL,
  seen_scan INTEGER
);
CREATE INDEX IF NOT EXISTS items_kind ON items(kind);
CREATE INDEX IF NOT EXISTS items_show ON items(show_id);
CREATE INDEX IF NOT EXISTS items_source ON items(source_id);
CREATE UNIQUE INDEX IF NOT EXISTS items_file ON items(file_path) WHERE kind != 'show';
CREATE TABLE IF NOT EXISTS progress (
  item_id TEXT PRIMARY KEY,
  position_ms INTEGER NOT NULL,
  duration_ms INTEGER NOT NULL,
  updated_at INTEGER NOT NULL,
  watched INTEGER NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS tmdb_cache (
  key TEXT PRIMARY KEY,
  body TEXT NOT NULL,
  fetched_at INTEGER NOT NULL
);
CREATE TABLE IF NOT EXISTS settings (
  key TEXT PRIMARY KEY,
  value TEXT
);
''');
  db.userVersion = schemaVersion;
}

/// Drops expired TMDB cache entries and TMDB-derived fields older than
/// [tmdbDataMaxAge] (they are re-fetched on the next scan).
void purgeExpiredTmdbData(Database db, DateTime now) {
  final cacheCutoff = now
      .subtract(TmdbClient.maxCacheAge)
      .millisecondsSinceEpoch;
  db.execute('DELETE FROM tmdb_cache WHERE fetched_at < ?', [cacheCutoff]);
  final cutoff = now.subtract(tmdbDataMaxAge).millisecondsSinceEpoch;
  db.execute(
    '''
UPDATE items SET
  overview = NULL,
  poster_url = local_poster,
  backdrop_url = NULL,
  original_title = NULL,
  meta_fetched_at = NULL
WHERE meta_fetched_at IS NOT NULL AND meta_fetched_at < ?''',
    [cutoff],
  );
}

class SqliteTmdbCache implements TmdbCache {
  final Database db;
  SqliteTmdbCache(this.db);

  @override
  String? get(String key, Duration maxAge) {
    final cutoff = DateTime.now().subtract(maxAge).millisecondsSinceEpoch;
    final r = db.select(
      'SELECT body FROM tmdb_cache WHERE key = ? AND fetched_at >= ?',
      [key, cutoff],
    );
    return r.isEmpty ? null : r.first['body'] as String;
  }

  @override
  void put(String key, String body) {
    db.execute(
      'INSERT OR REPLACE INTO tmdb_cache(key, body, fetched_at) VALUES (?, ?, ?)',
      [key, body, DateTime.now().millisecondsSinceEpoch],
    );
  }
}

LibraryItem itemFromRow(Row r) => LibraryItem(
  id: r['id'] as String,
  kind: ItemKind.values.byName(r['kind'] as String),
  title: r['title'] as String,
  originalTitle: r['original_title'] as String?,
  year: r['year'] as int?,
  overview: r['overview'] as String?,
  posterUrl: r['poster_url'] as String?,
  backdropUrl: r['backdrop_url'] as String?,
  showId: r['show_id'] as String?,
  season: r['season'] as int?,
  episode: r['episode'] as int?,
  artist: r['artist'] as String?,
  album: r['album'] as String?,
  trackNumber: r['track_number'] as int?,
  filePath: r['file_path'] as String,
  durationMs: r['duration_ms'] as int?,
  matchConfidence: (r['match_confidence'] as num?)?.toDouble(),
  tmdbId: r['tmdb_id'] as int?,
  addedAt: DateTime.fromMillisecondsSinceEpoch(r['added_at'] as int),
);

Progress progressFromRow(Row r, {String prefix = ''}) => Progress(
  itemId: r['${prefix}item_id'] as String,
  positionMs: r['${prefix}position_ms'] as int,
  durationMs: r['${prefix}duration_ms'] as int,
  updatedAt: DateTime.fromMillisecondsSinceEpoch(
    r['${prefix}updated_at'] as int,
  ),
  watched: (r['${prefix}watched'] as int) != 0,
);

MediaSource sourceFromRow(Row r) => MediaSource(
  id: r['id'] as String,
  kind: SourceKind.values.byName(r['kind'] as String),
  name: r['name'] as String,
  path: r['path'] as String,
  username: r['username'] as String?,
);
