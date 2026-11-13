// SPDX-License-Identifier: MPL-2.0

import 'dart:async';
import 'dart:convert';
import 'dart:io';

import 'package:crypto/crypto.dart';
import 'package:http/http.dart' as http;
import 'package:path/path.dart' as p;
import 'package:sqlite3/sqlite3.dart';

import 'db.dart';
import 'models.dart';
import 'music_tags.dart';
import 'parser/filename_parser.dart';
import 'scanner.dart';
import 'smb.dart';
import 'tmdb.dart';

/// Lower / upper bound of [Library.continueWatching].
const double continueWatchingMin = 0.02;
const double continueWatchingMax = 0.95;

/// The media library: sources, scanning, metadata, progress.
abstract class Library {
  /// Opens (or creates) the library database at [dbPath] (`:memory:` for an
  /// in-memory database). [httpClient], [smbResolver] and [tmdbBaseUri] are
  /// for tests and advanced use.
  static Future<Library> open(
    String dbPath, {
    http.Client? httpClient,
    SmbResolver? smbResolver,
    Uri? tmdbBaseUri,
  }) async => SqliteLibrary.open(
    dbPath,
    httpClient: httpClient,
    smbResolver: smbResolver,
    tmdbBaseUri: tmdbBaseUri,
  );

  Future<List<MediaSource>> sources();

  /// Adds a source (an empty [MediaSource.id] gets a generated one) or
  /// updates an existing one with the same id.
  Future<MediaSource> addSource(MediaSource s);

  /// Removes a source and all of its items.
  Future<void> removeSource(String id);

  /// Scans one source (or all of them): finds files, parses names and
  /// fetches metadata. Emits progress and completes with a [ScanPhase.done]
  /// event. Scans are serialised.
  Stream<ScanProgress> scan({String? sourceId, String? smbPassword});

  /// Sets (or clears) the TMDB API key: a v3 key or a v4 read access token.
  /// Without a key the library works with recognised titles only.
  Future<void> setTmdbApiKey(String? key);

  /// Current TMDB key (null if none).
  Future<String?> tmdbApiKey();

  /// TMDB language, e.g. `ru-RU` (null → TMDB default, en-US).
  Future<void> setMetadataLanguage(String? language);

  Stream<List<LibraryItem>> movies();
  Stream<List<LibraryItem>> shows();
  Stream<List<LibraryItem>> episodes(String showId);
  Stream<List<LibraryItem>> tracks();

  /// Items with 2%..95% progress, most recently played first.
  Stream<List<(LibraryItem, Progress)>> continueWatching();

  /// Movies or shows, newest first.
  Stream<List<LibraryItem>> recentlyAdded({
    ItemKind kind = ItemKind.movie,
    int limit = 20,
  });

  Future<LibraryItem?> item(String id);
  Future<Progress?> progress(String itemId);

  /// Case-insensitive search over movies, shows, episodes and tracks.
  Future<List<LibraryItem>> search(String query, {int limit = 100});

  Future<void> saveProgress(Progress p);

  /// Manual match fix. For an episode the fix applies to its show.
  /// With [tmdbId] the TMDB entry is used; with [title]/[year] the item is
  /// renamed and searched again. The item is locked against re-matching.
  Future<void> fixMatch(String itemId, {int? tmdbId, String? title, int? year});

  Future<void> close();
}

String _hashId(String prefix, String s) =>
    '$prefix${sha1.convert(utf8.encode(s)).toString().substring(0, 16)}';

String showIdFor(String title) => 'show:${normalizeTitle(title)}';

class SqliteLibrary implements Library {
  final Database _db;
  final http.Client _http;
  final bool _ownsHttp;
  final SmbResolver _smb;
  final Uri? _tmdbBaseUri;
  final _changes = StreamController<void>.broadcast();
  Future<void> _scanQueue = Future.value();
  bool _closed = false;

  SqliteLibrary._(
    this._db,
    this._http,
    this._ownsHttp,
    this._smb,
    this._tmdbBaseUri,
  );

  static SqliteLibrary open(
    String dbPath, {
    http.Client? httpClient,
    SmbResolver? smbResolver,
    Uri? tmdbBaseUri,
  }) {
    final Database db;
    if (dbPath == ':memory:') {
      db = sqlite3.openInMemory();
    } else {
      Directory(p.dirname(dbPath)).createSync(recursive: true);
      db = sqlite3.open(dbPath);
    }
    migrate(db);
    purgeExpiredTmdbData(db, DateTime.now());
    return SqliteLibrary._(
      db,
      httpClient ?? http.Client(),
      httpClient == null,
      smbResolver ?? SmbResolver(),
      tmdbBaseUri,
    );
  }

  // -------------------------------------------------------------------------
  // settings

  String? _setting(String key) {
    final r = _db.select('SELECT value FROM settings WHERE key = ?', [key]);
    return r.isEmpty ? null : r.first['value'] as String?;
  }

  void _setSetting(String key, String? value) {
    if (value == null) {
      _db.execute('DELETE FROM settings WHERE key = ?', [key]);
    } else {
      _db.execute('INSERT OR REPLACE INTO settings(key, value) VALUES (?, ?)', [
        key,
        value,
      ]);
    }
  }

  @override
  Future<void> setTmdbApiKey(String? key) async {
    final k = key?.trim();
    final old = _setting('tmdb_api_key');
    _setSetting('tmdb_api_key', (k == null || k.isEmpty) ? null : k);
    if (old != k) {
      // Retry unmatched items with the new key on the next scan.
      _db.execute(
        'UPDATE items SET meta_fetched_at = NULL WHERE tmdb_id IS NULL',
      );
    }
  }

  @override
  Future<String?> tmdbApiKey() async => _setting('tmdb_api_key');

  @override
  Future<void> setMetadataLanguage(String? language) async {
    final old = _setting('tmdb_language');
    _setSetting('tmdb_language', language);
    if (old != language) {
      _db.execute('UPDATE items SET meta_fetched_at = NULL');
    }
  }

  TmdbClient? _tmdb() {
    final key = _setting('tmdb_api_key');
    if (key == null) return null;
    return TmdbClient(
      _http,
      apiKey: key,
      language: _setting('tmdb_language'),
      cache: SqliteTmdbCache(_db),
      baseUri: _tmdbBaseUri,
    );
  }

  // -------------------------------------------------------------------------
  // sources

  @override
  Future<List<MediaSource>> sources() async => _db
      .select('SELECT * FROM sources ORDER BY position, name')
      .map(sourceFromRow)
      .toList();

  @override
  Future<MediaSource> addSource(MediaSource s) async {
    var src = s;
    if (src.kind == SourceKind.smb) {
      SmbUrl.parse(src.path); // validates
    } else {
      src = src.copyWith(path: p.normalize(p.absolute(src.path)));
    }
    if (src.id.isEmpty) {
      src = src.copyWith(
        id: _hashId(
          'src_',
          '${src.kind.name}:${src.path}:${DateTime.now().microsecondsSinceEpoch}',
        ),
      );
    }
    final pos = _db
        .select('SELECT COALESCE(MAX(position), 0) + 1 AS p FROM sources')
        .first['p'];
    _db.execute(
      '''
INSERT INTO sources(id, kind, name, path, username, position) VALUES (?, ?, ?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET kind = excluded.kind, name = excluded.name,
  path = excluded.path, username = excluded.username''',
      [src.id, src.kind.name, src.name, src.path, src.username, pos],
    );
    _notify();
    return src;
  }

  @override
  Future<void> removeSource(String id) async {
    _db.execute('BEGIN');
    try {
      _db.execute('DELETE FROM items WHERE source_id = ? AND kind != ?', [
        id,
        'show',
      ]);
      _deleteEmptyShows();
      _db.execute('DELETE FROM sources WHERE id = ?', [id]);
      _db.execute('COMMIT');
    } catch (_) {
      _db.execute('ROLLBACK');
      rethrow;
    }
    _notify();
  }

  void _deleteEmptyShows() {
    _db.execute('''
DELETE FROM items WHERE kind = 'show' AND NOT EXISTS
  (SELECT 1 FROM items e WHERE e.kind = 'episode' AND e.show_id = items.id)''');
  }

  // -------------------------------------------------------------------------
  // scanning

  @override
  Stream<ScanProgress> scan({String? sourceId, String? smbPassword}) {
    final c = StreamController<ScanProgress>();
    final prev = _scanQueue;
    final done = Completer<void>();
    _scanQueue = done.future;
    () async {
      await prev;
      try {
        await _runScan(c, sourceId, smbPassword);
      } catch (e) {
        c.add(ScanProgress(phase: ScanPhase.done, error: '$e', errors: ['$e']));
      } finally {
        done.complete();
        _notify();
        await c.close();
      }
    }();
    return c.stream;
  }

  Future<void> _runScan(
    StreamController<ScanProgress> c,
    String? sourceId,
    String? smbPassword,
  ) async {
    final errors = <String>[];
    var found = 0;
    void emit(
      ScanPhase phase, {
      String? src,
      int processed = 0,
      int? total,
      String? path,
      String? error,
    }) {
      if (error != null) errors.add(error);
      if (!c.isClosed) {
        c.add(
          ScanProgress(
            phase: phase,
            sourceId: src,
            filesFound: found,
            processed: processed,
            total: total,
            currentPath: path,
            error: error,
            errors: List.unmodifiable(errors),
          ),
        );
      }
    }

    final all = await sources();
    final selected = sourceId == null
        ? all
        : all.where((s) => s.id == sourceId).toList();
    if (sourceId != null && selected.isEmpty) {
      emit(ScanPhase.done, error: 'Unknown source $sourceId');
      return;
    }
    final scanId = DateTime.now().microsecondsSinceEpoch;

    for (final src in selected) {
      emit(ScanPhase.connecting, src: src.id, path: src.path);
      String root;
      try {
        root = src.kind == SourceKind.smb
            ? await _smb.resolve(
                src.path,
                username: src.username,
                password: smbPassword,
              )
            : src.path;
      } catch (e) {
        emit(ScanPhase.connecting, src: src.id, error: '${src.name}: $e');
        continue;
      }
      if (!await Directory(root).exists()) {
        // Offline drive / unmounted share: keep the existing items.
        emit(
          ScanPhase.connecting,
          src: src.id,
          error: '${src.name}: folder not found: $root',
        );
        continue;
      }

      final files = <ScannedFile>[];
      await for (final f in scanFolder(root)) {
        files.add(f);
        found++;
        if (found % 50 == 0) {
          emit(ScanPhase.scanning, src: src.id, path: f.path);
        }
      }
      final videosPerDir = <String, int>{};
      for (final f in files.where((f) => f.type == MediaType.video)) {
        videosPerDir.update(p.dirname(f.path), (v) => v + 1, ifAbsent: () => 1);
      }

      var processed = 0;
      const batch = 200;
      for (var i = 0; i < files.length; i += batch) {
        _db.execute('BEGIN');
        try {
          for (final f in files.skip(i).take(batch)) {
            _upsertFile(src, root, f, scanId, videosPerDir);
            processed++;
          }
          _db.execute('COMMIT');
        } catch (e) {
          _db.execute('ROLLBACK');
          emit(ScanPhase.scanning, src: src.id, error: '${src.name}: $e');
        }
        emit(
          ScanPhase.scanning,
          src: src.id,
          processed: processed,
          total: files.length,
        );
        _notify();
        await Future<void>.delayed(Duration.zero);
      }

      // Remove files that disappeared from this (reachable) source.
      _db.execute(
        '''
DELETE FROM items WHERE source_id = ? AND kind != 'show'
  AND (seen_scan IS NULL OR seen_scan != ?)''',
        [src.id, scanId],
      );
      _deleteEmptyShows();
      _notify();
    }

    await _fetchMetadata((processed, total, error) {
      emit(
        ScanPhase.metadata,
        processed: processed,
        total: total,
        error: error,
      );
    });
    emit(ScanPhase.done, processed: found, total: found);
  }

  void _upsertFile(
    MediaSource src,
    String root,
    ScannedFile f,
    int scanId,
    Map<String, int> videosPerDir,
  ) {
    final path = f.path;
    final mtime = f.modified.millisecondsSinceEpoch;
    final existing = _db.select(
      'SELECT id, file_size, file_mtime FROM items WHERE file_path = ? AND kind != ?',
      [path, 'show'],
    );
    if (existing.isNotEmpty) {
      final e = existing.first;
      if (e['file_size'] == f.size && e['file_mtime'] == mtime) {
        _db.execute(
          'UPDATE items SET seen_scan = ?, source_id = ? WHERE id = ?',
          [scanId, src.id, e['id']],
        );
        return;
      }
    }
    final now = DateTime.now().millisecondsSinceEpoch;

    if (f.type == MediaType.audio) {
      final t = trackInfo(path, root: root);
      final cover = findLocalImage(path);
      final title = t.title.isNotEmpty
          ? t.title
          : p.basenameWithoutExtension(path);
      _db.execute(
        '''
INSERT INTO items(id, kind, source_id, title, year, artist, album, track_number,
  file_path, file_size, file_mtime, duration_ms, poster_url, local_poster,
  match_confidence, parsed_title, added_at, seen_scan, meta_fetched_at)
VALUES (?, 'track', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET source_id = excluded.source_id, title = excluded.title,
  year = excluded.year, artist = excluded.artist, album = excluded.album,
  track_number = excluded.track_number, file_size = excluded.file_size,
  file_mtime = excluded.file_mtime, duration_ms = excluded.duration_ms,
  poster_url = excluded.poster_url, local_poster = excluded.local_poster,
  seen_scan = excluded.seen_scan''',
        [
          _hashId('a', path),
          src.id,
          title,
          t.year,
          t.artist,
          t.album,
          t.trackNumber,
          path,
          f.size,
          mtime,
          t.durationMs,
          cover,
          cover,
          1.0,
          title,
          now,
          scanId,
          now,
        ],
      );
      return;
    }

    final parsed = parsePath(path, root: root);
    final id = _hashId('v', path);
    final locked = _db.select('SELECT locked FROM items WHERE id = ?', [id]);
    if (locked.isNotEmpty && locked.first['locked'] == 1) {
      _db.execute(
        'UPDATE items SET file_size = ?, file_mtime = ?, seen_scan = ?, source_id = ? WHERE id = ?',
        [f.size, mtime, scanId, src.id, id],
      );
      return;
    }

    if (parsed.isEpisode) {
      var showTitle = parsed.title.isNotEmpty ? parsed.title : 'Unknown';
      final showId = _ensureShow(
        src,
        root,
        path,
        parsed,
        showTitle,
        now,
        scanId,
      );
      final epTitle = parsed.episodeTitle ?? 'Episode ${parsed.episode}';
      _db.execute(
        '''
INSERT INTO items(id, kind, source_id, title, show_id, season, episode, episode_end,
  file_path, file_size, file_mtime, match_confidence, parsed_title, parsed_year,
  added_at, seen_scan)
VALUES (?, 'episode', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET kind = 'episode', source_id = excluded.source_id,
  title = excluded.title, show_id = excluded.show_id, season = excluded.season,
  episode = excluded.episode, episode_end = excluded.episode_end,
  file_size = excluded.file_size, file_mtime = excluded.file_mtime,
  match_confidence = excluded.match_confidence, parsed_title = excluded.parsed_title,
  seen_scan = excluded.seen_scan, meta_fetched_at = NULL''',
        [
          id,
          src.id,
          epTitle,
          showId,
          parsed.season ?? 1,
          parsed.episode,
          parsed.episodeEnd,
          path,
          f.size,
          mtime,
          parsed.confidence,
          parsed.episodeTitle,
          parsed.year,
          now,
          scanId,
        ],
      );
    } else {
      final single =
          (videosPerDir[p.dirname(path)] ?? 0) == 1 &&
          !p.equals(p.dirname(path), root);
      final poster = findLocalImage(path, video: true, allowGeneric: single);
      final title = parsed.title.isNotEmpty
          ? parsed.title
          : p.basenameWithoutExtension(path);
      _db.execute(
        '''
INSERT INTO items(id, kind, source_id, title, year, file_path, file_size, file_mtime,
  match_confidence, parsed_title, parsed_alt_title, parsed_year, poster_url,
  local_poster, added_at, seen_scan)
VALUES (?, 'movie', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)
ON CONFLICT(id) DO UPDATE SET kind = 'movie', source_id = excluded.source_id,
  title = excluded.title, year = excluded.year, show_id = NULL, season = NULL,
  episode = NULL, file_size = excluded.file_size, file_mtime = excluded.file_mtime,
  match_confidence = excluded.match_confidence, parsed_title = excluded.parsed_title,
  parsed_alt_title = excluded.parsed_alt_title, parsed_year = excluded.parsed_year,
  poster_url = excluded.poster_url, local_poster = excluded.local_poster,
  seen_scan = excluded.seen_scan, meta_fetched_at = NULL, tmdb_id = NULL''',
        [
          id,
          src.id,
          title,
          parsed.year,
          path,
          f.size,
          mtime,
          parsed.confidence,
          title,
          parsed.altTitle,
          parsed.year,
          poster,
          poster,
          now,
          scanId,
        ],
      );
    }
  }

  String _ensureShow(
    MediaSource src,
    String root,
    String path,
    ParsedName parsed,
    String title,
    int now,
    int scanId,
  ) {
    final id = showIdFor(title);
    final existing = _db.select(
      'SELECT year, parsed_year FROM items WHERE id = ?',
      [id],
    );
    if (existing.isEmpty) {
      var folder = p.dirname(path);
      if (seasonFromFolder(p.basename(folder)) != null &&
          !p.equals(folder, root)) {
        folder = p.dirname(folder);
      }
      final poster =
          findLocalImage(
            p.join(folder, 'x.mkv'),
            video: true,
            allowGeneric: !p.equals(folder, root),
          ) ??
          findLocalImage(path, video: true, allowGeneric: false);
      _db.execute(
        '''
INSERT INTO items(id, kind, source_id, title, year, file_path, match_confidence,
  parsed_title, parsed_alt_title, parsed_year, poster_url, local_poster, added_at, seen_scan)
VALUES (?, 'show', ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)''',
        [
          id,
          src.id,
          title,
          parsed.year,
          folder,
          parsed.confidence,
          title,
          parsed.altTitle,
          parsed.year,
          poster,
          poster,
          now,
          scanId,
        ],
      );
    } else if (existing.first['parsed_year'] == null && parsed.year != null) {
      _db.execute(
        'UPDATE items SET parsed_year = ?, year = COALESCE(year, ?) WHERE id = ?',
        [parsed.year, parsed.year, id],
      );
    }
    return id;
  }

  // -------------------------------------------------------------------------
  // metadata

  Future<void> _fetchMetadata(
    void Function(int processed, int total, String? error) report,
  ) async {
    final tmdb = _tmdb();
    if (tmdb == null) return;

    final movies = _db.select('''
SELECT id, parsed_title, parsed_alt_title, parsed_year, tmdb_id, locked FROM items
WHERE kind = 'movie' AND meta_fetched_at IS NULL''');
    final shows = _db.select('''
SELECT id, parsed_title, parsed_alt_title, parsed_year, tmdb_id, locked FROM items
WHERE kind = 'show' AND meta_fetched_at IS NULL''');
    var total = movies.length + shows.length;
    var processed = 0;
    var fatal = false;

    Future<void> guard(Future<void> Function() job) async {
      if (fatal) return;
      try {
        await job();
      } on TmdbException catch (e) {
        if (e.statusCode == 401) {
          fatal = true;
          report(processed, total, 'TMDB: invalid API key');
        } else {
          report(processed, total, 'TMDB: ${e.message}');
        }
      } catch (e) {
        report(processed, total, 'TMDB: $e');
      }
      processed++;
      if (processed % 10 == 0) {
        report(processed, total, null);
        _notify();
      }
    }

    Future<void> pool(List<Future<void> Function()> jobs) async {
      var next = 0;
      Future<void> worker() async {
        while (next < jobs.length && !fatal) {
          final j = jobs[next++];
          await guard(j);
        }
      }

      await Future.wait([for (var i = 0; i < 4; i++) worker()]);
    }

    await pool([
      for (final r in movies) () => _matchItem(tmdb, r, tv: false),
      for (final r in shows) () => _matchItem(tmdb, r, tv: true),
    ]);
    _notify();
    if (fatal) return;

    final eps = _db.select(
      '''
SELECT e.id, e.season, e.episode, s.tmdb_id AS show_tmdb, s.poster_url AS show_poster
FROM items e JOIN items s ON s.id = e.show_id
WHERE e.kind = 'episode' AND e.meta_fetched_at IS NULL AND s.tmdb_id IS NOT NULL''',
    );
    total += eps.length;
    await pool([
      for (final r in eps)
        () async {
          final ep = await tmdb.episode(
            r['show_tmdb'] as int,
            r['season'] as int,
            r['episode'] as int,
          );
          final now = DateTime.now().millisecondsSinceEpoch;
          if (ep == null) {
            _db.execute(
              'UPDATE items SET meta_fetched_at = ?, poster_url = ? WHERE id = ?',
              [now, r['show_poster'], r['id']],
            );
            return;
          }
          _db.execute(
            '''
UPDATE items SET title = COALESCE(?, title), overview = ?, backdrop_url = ?,
  poster_url = ?, meta_fetched_at = ? WHERE id = ?''',
            [ep.name, ep.overview, ep.stillUrl, r['show_poster'], now, r['id']],
          );
        },
    ]);
    report(processed, total, null);
    _notify();
  }

  Future<void> _matchItem(TmdbClient tmdb, Row r, {required bool tv}) async {
    final id = r['id'] as String;
    final now = DateTime.now().millisecondsSinceEpoch;
    final tmdbId = r['tmdb_id'] as int?;
    TmdbSearchResult? res;
    double conf;
    if (tmdbId != null && r['locked'] == 1) {
      res = tv ? await tmdb.tv(tmdbId) : await tmdb.movie(tmdbId);
      conf = 1.0;
    } else {
      final m = await tmdb.findBest(
        r['parsed_title'] as String? ?? '',
        year: r['parsed_year'] as int?,
        altTitle: r['parsed_alt_title'] as String?,
        tv: tv,
      );
      if (m == null) {
        _db.execute('UPDATE items SET meta_fetched_at = ? WHERE id = ?', [
          now,
          id,
        ]);
        return;
      }
      res = m.result;
      conf = m.confidence;
    }
    _applyResult(id, res, conf, now);
  }

  void _applyResult(String id, TmdbSearchResult res, double conf, int now) {
    _db.execute(
      '''
UPDATE items SET title = ?, original_title = ?, year = COALESCE(?, year), overview = ?,
  poster_url = COALESCE(?, local_poster, poster_url), backdrop_url = ?, tmdb_id = ?,
  match_confidence = ?, meta_fetched_at = ? WHERE id = ?''',
      [
        res.title.isNotEmpty ? res.title : _titleOf(id),
        res.originalTitle,
        res.year,
        res.overview,
        res.posterUrl,
        res.backdropUrl,
        res.id,
        conf,
        now,
        id,
      ],
    );
    if (res.isTv) {
      // Episodes pick up the show poster and are re-fetched.
      _db.execute(
        'UPDATE items SET meta_fetched_at = NULL, poster_url = ? WHERE show_id = ?',
        [res.posterUrl, id],
      );
    }
  }

  String _titleOf(String id) =>
      _db.select('SELECT title FROM items WHERE id = ?', [id]).first['title']
          as String;

  @override
  Future<void> fixMatch(
    String itemId, {
    int? tmdbId,
    String? title,
    int? year,
  }) async {
    final rows = _db.select(
      'SELECT id, kind, show_id FROM items WHERE id = ?',
      [itemId],
    );
    if (rows.isEmpty) {
      throw ArgumentError.value(itemId, 'itemId', 'no such item');
    }
    var targetId = itemId;
    var kind = ItemKind.values.byName(rows.first['kind'] as String);
    if (kind == ItemKind.episode && rows.first['show_id'] != null) {
      targetId = rows.first['show_id'] as String;
      kind = ItemKind.show;
    }
    if (kind == ItemKind.track) {
      _db.execute(
        'UPDATE items SET title = COALESCE(?, title), year = COALESCE(?, year), '
        'locked = 1 WHERE id = ?',
        [title, year, targetId],
      );
      _notify();
      return;
    }
    final tv = kind == ItemKind.show;
    final now = DateTime.now().millisecondsSinceEpoch;

    if (title != null || year != null) {
      _db.execute(
        '''
UPDATE items SET title = COALESCE(?, title), parsed_title = COALESCE(?, parsed_title),
  parsed_alt_title = CASE WHEN ? IS NULL THEN parsed_alt_title ELSE NULL END,
  year = ?, parsed_year = ?, locked = 1, tmdb_id = NULL, meta_fetched_at = NULL,
  overview = NULL, backdrop_url = NULL, original_title = NULL, poster_url = local_poster,
  match_confidence = 1.0
WHERE id = ?''',
        [title, title, title, year, year, targetId],
      );
    }
    if (tmdbId != null) {
      _db.execute(
        'UPDATE items SET tmdb_id = ?, locked = 1, meta_fetched_at = NULL WHERE id = ?',
        [tmdbId, targetId],
      );
    }
    if (tv) {
      _db.execute('UPDATE items SET meta_fetched_at = NULL WHERE show_id = ?', [
        targetId,
      ]);
    }

    final tmdb = _tmdb();
    if (tmdb != null) {
      final r = _db
          .select(
            '''
SELECT id, parsed_title, parsed_alt_title, parsed_year, tmdb_id, locked FROM items
WHERE id = ?''',
            [targetId],
          )
          .first;
      if (tmdbId != null) {
        final res = tv ? await tmdb.tv(tmdbId) : await tmdb.movie(tmdbId);
        _applyResult(targetId, res, 1.0, now);
      } else {
        await _matchItem(tmdb, r, tv: tv);
      }
      if (tv) await _fetchMetadata((_, _, _) {});
    }
    _notify();
  }

  // -------------------------------------------------------------------------
  // queries

  void _notify() {
    if (!_closed && _changes.hasListener) _changes.add(null);
  }

  Stream<T> _watch<T>(T Function() query) => Stream.multi((c) {
    void emit() {
      if (_closed) return;
      try {
        c.add(query());
      } catch (e, st) {
        c.addError(e, st);
      }
    }

    emit();
    final sub = _changes.stream.listen((_) => emit());
    c.onCancel = sub.cancel;
  });

  List<LibraryItem> _items(
    String where, [
    List<Object?> args = const [],
    String order = '',
  ]) => _db
      .select('SELECT * FROM items WHERE $where $order', args)
      .map(itemFromRow)
      .toList();

  @override
  Stream<List<LibraryItem>> movies() => _watch(
    () => _items(
      "kind = 'movie'",
      const [],
      'ORDER BY title COLLATE NOCASE, year',
    ),
  );

  @override
  Stream<List<LibraryItem>> shows() => _watch(
    () => _items(
      "kind = 'show' AND EXISTS (SELECT 1 FROM items e WHERE e.show_id = items.id)",
      const [],
      'ORDER BY title COLLATE NOCASE',
    ),
  );

  @override
  Stream<List<LibraryItem>> episodes(String showId) => _watch(
    () => _items("kind = 'episode' AND show_id = ?", [
      showId,
    ], 'ORDER BY season, episode, file_path'),
  );

  @override
  Stream<List<LibraryItem>> tracks() => _watch(
    () => _items(
      "kind = 'track'",
      const [],
      'ORDER BY artist COLLATE NOCASE, album COLLATE NOCASE, track_number, title COLLATE NOCASE',
    ),
  );

  @override
  Stream<List<LibraryItem>> recentlyAdded({
    ItemKind kind = ItemKind.movie,
    int limit = 20,
  }) => _watch(
    () => _items('kind = ?', [
      kind.name,
    ], 'ORDER BY added_at DESC, title LIMIT $limit'),
  );

  @override
  Stream<List<(LibraryItem, Progress)>> continueWatching() => _watch(() {
    final rows = _db.select(
      '''
SELECT i.*, p.item_id AS p_item_id, p.position_ms AS p_position_ms,
  p.duration_ms AS p_duration_ms, p.updated_at AS p_updated_at, p.watched AS p_watched
FROM progress p JOIN items i ON i.id = p.item_id
WHERE p.watched = 0 AND p.duration_ms > 0
  AND CAST(p.position_ms AS REAL) / p.duration_ms >= ?
  AND CAST(p.position_ms AS REAL) / p.duration_ms <= ?
  AND i.kind IN ('movie', 'episode')
ORDER BY p.updated_at DESC LIMIT 50''',
      [continueWatchingMin, continueWatchingMax],
    );
    return [
      for (final r in rows) (itemFromRow(r), progressFromRow(r, prefix: 'p_')),
    ];
  });

  @override
  Future<LibraryItem?> item(String id) async {
    final r = _items('id = ?', [id]);
    return r.isEmpty ? null : r.first;
  }

  @override
  Future<Progress?> progress(String itemId) async {
    final r = _db.select('SELECT * FROM progress WHERE item_id = ?', [itemId]);
    return r.isEmpty ? null : progressFromRow(r.first);
  }

  @override
  Future<List<LibraryItem>> search(String query, {int limit = 100}) async {
    final q = normalizeTitle(query);
    if (q.isEmpty) return const [];
    final out = <LibraryItem>[];
    final rows = _db.select(
      'SELECT * FROM items ORDER BY '
      "CASE kind WHEN 'movie' THEN 0 WHEN 'show' THEN 1 WHEN 'track' THEN 2 ELSE 3 END, "
      'title COLLATE NOCASE',
    );
    for (final r in rows) {
      final hay = [
        r['title'],
        r['original_title'],
        r['artist'],
        r['album'],
      ].whereType<String>().map(normalizeTitle).join(' | ');
      if (hay.contains(q)) {
        out.add(itemFromRow(r));
        if (out.length >= limit) break;
      }
    }
    return out;
  }

  @override
  Future<void> saveProgress(Progress pr) async {
    final watched =
        pr.watched ||
        (pr.durationMs > 0 &&
            pr.positionMs / pr.durationMs > continueWatchingMax);
    _db.execute(
      '''
INSERT OR REPLACE INTO progress(item_id, position_ms, duration_ms, updated_at, watched)
VALUES (?, ?, ?, ?, ?)''',
      [
        pr.itemId,
        pr.positionMs,
        pr.durationMs,
        pr.updatedAt.millisecondsSinceEpoch,
        watched ? 1 : 0,
      ],
    );
    if (pr.durationMs > 0) {
      _db.execute(
        'UPDATE items SET duration_ms = ? WHERE id = ? AND duration_ms IS NULL',
        [pr.durationMs, pr.itemId],
      );
    }
    _notify();
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    await _scanQueue;
    _closed = true;
    await _changes.close();
    _db.close();
    if (_ownsHttp) _http.close();
  }
}
