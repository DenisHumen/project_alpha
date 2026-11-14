// SPDX-License-Identifier: MPL-2.0

import 'dart:async';
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary, Int64List;
import 'package:pa_core/pa_core.dart';
import 'package:path/path.dart' as p;

import 'library.dart';
import 'library_lists.dart';
import 'library_query.dart';
import 'models.dart';
import 'music.dart';
import 'music_tags.dart';
import 'open_library.dart';
import 'playback_session.dart';
import 'search_groups.dart';
import 'smb.dart' show SmbUrl;
import 'subtitles.dart';

/// [Library] on the Rust core (UI-024): `pa_core`'s [CoreLibrary] over the
/// `pa-library` schema v1 — `pa-scanner` with music tags, the
/// `pa-recognition` parser, FTS5 search, progress with "Continue watching"
/// and Up Next. Screens see the same [Library] interface as with the Dart
/// v0 implementation ([SqliteLibrary]).
///
/// After every scan the core fetches provider metadata (META-013): TMDB
/// titles, overviews, posters, backdrops and episode data, AniList and
/// Shikimori for anime; a poster from TMDB wins over a local image next to
/// the files, as in v0. The TMDB key and the metadata language are kept in
/// memory: the app sets them on start.
///
/// Network sources (SMB, WebDAV) are read by the core through `pa-vfs`
/// (LIB-018): nothing is mounted and the app never holds their passwords.
/// Their files play through the core's loopback HTTP proxy
/// ([LibraryItem.streamUri]).
class RustLibrary
    implements Library, LibraryLists, LibrarySubtitles, LibraryMusic {
  final CoreLibrary _core;
  final String? _tempDir;
  final String _metadataCacheDir;
  final _changes = StreamController<void>.broadcast();

  /// Plays and audio playlists changed (MUS-004): only the music views
  /// reload.
  final _musicChanges = StreamController<void>.broadcast();
  Future<void> _scanQueue = Future.value();
  bool _closed = false;
  String? _tmdbKey;
  String _language = defaultMetadataLanguage;

  /// Metadata language until [setMetadataLanguage] says otherwise.
  static const defaultMetadataLanguage = 'ru-RU';

  /// Addresses of the sources (by id), for the display path of network files.
  final Map<int, String> _sourceUris = {};

  /// Local roots of the `file://` sources (by id), to find the source of a
  /// local file (sidecar subtitles, PLAY-007).
  final Map<int, String> _localRoots = {};

  /// Base URL of the core's stream proxy (`http://127.0.0.1:<port>/<token>`);
  /// null if it could not start (network files are then not playable).
  String? _streamBase;

  /// Why network sources cannot be reached (the secret store failed to
  /// open), or null.
  Object? get networkSourcesError => _networkError;
  Object? _networkError;

  /// Local images per folder: `findLocalImage` results by `(path, mode)`.
  final Map<String, String?> _images = {};

  RustLibrary._(this._core, this._tempDir, this._metadataCacheDir);

  /// The core library, for services on the same database (device sync,
  /// UI-019).
  CoreLibrary get core => _core;

  /// Refreshes the views after something outside this adapter changed the
  /// database (progress from another device, UI-019).
  void notifyExternalChange() => _notify();

  static Future<void>? _init;

  /// Loads the native core once. [externalLibrary] is for tests that run
  /// outside an app bundle (`dart test`).
  static Future<void> ensureInitialized({ExternalLibrary? externalLibrary}) =>
      _init ??= RustLib.init(externalLibrary: externalLibrary);

  /// Opens (or creates) the v1 database at [dbPath] (`:memory:` for a
  /// temporary one).
  ///
  /// If [legacyV0Path] is a `pa_library` v0 database, it is imported (only
  /// what is new: the import is idempotent). With [renameLegacyV0] the v0
  /// file is renamed to `*.v0.bak` (or a free `*.v0.bak.N`) after a
  /// successful import (docs/dev/db.md); nothing is deleted. The app opens
  /// the library with [openAppLibrary].
  ///
  /// Provider responses are cached in [metadataCacheDir] (default: a
  /// `provider-cache` folder next to the database).
  ///
  /// With [secretsDir] (the app data folder, as for `CoreSources.open`) the
  /// core reaches SMB and WebDAV sources at once ([connectSources]); without
  /// it only local folders are scanned until [connectSources] is called.
  static Future<RustLibrary> open(
    String dbPath, {
    String? legacyV0Path,
    bool renameLegacyV0 = true,
    String? secretsDir,
    String? metadataCacheDir,
    ExternalLibrary? externalLibrary,
  }) async {
    await ensureInitialized(externalLibrary: externalLibrary);
    String? tempDir;
    var path = dbPath;
    if (dbPath == ':memory:') {
      tempDir = Directory.systemTemp.createTempSync('kinolune_core_').path;
      path = p.join(tempDir, 'library.db');
    } else {
      Directory(p.dirname(dbPath)).createSync(recursive: true);
    }
    final core = await CoreLibrary.open(path: path);
    final lib = RustLibrary._(
      core,
      tempDir,
      metadataCacheDir ?? p.join(p.dirname(path), 'provider-cache'),
    );
    if (secretsDir != null) await lib.connectSources(secretsDir);
    try {
      lib._streamBase = await core.streamBaseUrl();
    } catch (e) {
      lib._networkError ??= e;
    }
    if (legacyV0Path != null) {
      await lib.importLegacyV0(legacyV0Path, rename: renameLegacyV0);
    }
    await lib.sources();
    return lib;
  }

  /// Lets the core reach SMB and WebDAV sources with the passwords of its
  /// secret store: the one the app opened (`openPlatformSecrets`, CORE-015),
  /// else the default store with its fallback file in [dataDir]. Call it
  /// after opening the platform store. A failure is kept in
  /// [networkSourcesError]; local folders keep working.
  Future<void> connectSources(String dataDir) async {
    try {
      await _core.connectSources(dataDir: dataDir);
      _networkError = null;
    } catch (e) {
      _networkError = e;
    }
  }

  /// Imports a v0 database if [path] is one. Returns the report, or null if
  /// there is nothing to import.
  Future<CoreImportReport?> importLegacyV0(
    String path, {
    bool rename = true,
  }) async {
    if (!File(path).existsSync()) return null;
    if (!await isLegacyV0Library(path: path)) return null;
    final report = await _core.importV0(path: path);
    if (rename) {
      // Never over an older backup: a free `.v0.bak[.N]` name. A failed
      // rename leaves the file in place; the next start imports it again
      // (idempotent) and retries.
      final backup = freeLegacyV0BackupPath(path);
      try {
        for (final suffix in ['', '-wal', '-shm']) {
          final f = File('$path$suffix');
          if (f.existsSync()) f.renameSync('$backup$suffix');
        }
      } on FileSystemException {
        // Keep going on the imported data.
      }
    }
    _notify();
    return report;
  }

  // -------------------------------------------------------------------------
  // settings

  @override
  Future<void> setTmdbApiKey(String? key) async {
    final k = key?.trim();
    final old = _tmdbKey;
    _tmdbKey = (k == null || k.isEmpty) ? null : k;
    // Another key: ask again for what the old one did not find.
    if (old != null && old != _tmdbKey) await _core.resetMetadata(all: false);
  }

  @override
  Future<String?> tmdbApiKey() async => _tmdbKey;

  @override
  Future<void> setMetadataLanguage(String? language) async {
    final l = (language == null || language.trim().isEmpty)
        ? defaultMetadataLanguage
        : language.trim();
    // No reset: the core fetches again every work whose stored metadata is in
    // another language (`work_metadata.language`), on the next metadata run.
    _language = l;
  }

  /// The language of the next metadata run.
  String get metadataLanguage => _language;

  // -------------------------------------------------------------------------
  // sources

  MediaSource _source(CoreSource s) => MediaSource(
    id: '${s.id}',
    kind: switch (s.kind) {
      'smb' => SourceKind.smb,
      'webdav' => SourceKind.webdav,
      _ => SourceKind.localFolder,
    },
    name: s.name,
    path: s.localRoot ?? s.uri,
    username: s.username,
  );

  @override
  Future<List<MediaSource>> sources() async {
    final all = await _core.sources();
    _sourceUris
      ..clear()
      ..addEntries(all.map((s) => MapEntry(s.id.toInt(), s.uri)));
    _localRoots
      ..clear()
      ..addEntries([
        for (final s in all)
          if (s.localRoot != null) MapEntry(s.id.toInt(), s.localRoot!),
      ]);
    return all.map(_source).toList();
  }

  @override
  Future<MediaSource> addSource(MediaSource s) async {
    final String location;
    if (s.kind == SourceKind.smb) {
      SmbUrl.parse(s.path); // validates
      location = s.path;
    } else {
      location = p.normalize(p.absolute(s.path));
    }
    final added = await _core.addSource(
      id: int.tryParse(s.id),
      kind: s.kind == SourceKind.smb ? 'smb' : 'local',
      name: s.name,
      location: location,
      username: s.username,
    );
    _notify();
    return _source(added);
  }

  @override
  Future<void> removeSource(String id) async {
    final n = int.tryParse(id);
    if (n == null) return;
    await _core.removeSource(id: n);
    _sourceUris.remove(n);
    _localRoots.remove(n);
    _notify();
  }

  // -------------------------------------------------------------------------
  // scanning

  @override
  Stream<ScanProgress> scan({String? sourceId}) {
    final c = StreamController<ScanProgress>();
    final prev = _scanQueue;
    final done = Completer<void>();
    _scanQueue = done.future;
    () async {
      await prev;
      try {
        await _runScan(c, sourceId);
      } catch (e) {
        c.add(ScanProgress(phase: ScanPhase.done, error: '$e', errors: ['$e']));
      } finally {
        done.complete();
        _images.clear();
        _notify();
        await c.close();
      }
    }();
    return c.stream;
  }

  Future<void> _runScan(
    StreamController<ScanProgress> c,
    String? sourceId,
  ) async {
    final errors = <String>[];
    void emit(ScanProgress e) {
      if (e.error != null) errors.add(e.error!);
      if (!c.isClosed) c.add(e);
    }

    final id = sourceId == null ? null : int.tryParse(sourceId);
    if (sourceId != null && id == null) {
      final msg = 'Unknown source $sourceId';
      emit(
        ScanProgress(
          phase: ScanPhase.done,
          error: msg,
          errors: List.unmodifiable([...errors, msg]),
        ),
      );
      return;
    }

    var lastNotify = DateTime.now();
    ScanProgress? finished;
    await sources(); // addresses of sources added through CoreSources
    await for (final u in _core.scan(sourceId: id)) {
      final phase = switch (u.phase) {
        CoreScanPhase.connecting => ScanPhase.connecting,
        CoreScanPhase.scanning => ScanPhase.scanning,
        CoreScanPhase.analyzing => ScanPhase.metadata,
        CoreScanPhase.done => ScanPhase.done,
      };
      final progress = ScanProgress(
        phase: phase,
        sourceId: u.sourceId?.toString(),
        filesFound: u.filesFound,
        processed: u.processed,
        total: u.total,
        currentPath: u.currentPath,
        error: u.error,
        errors: List.unmodifiable([...errors, ?u.error]),
      );
      if (phase == ScanPhase.done) {
        // Held back until the metadata run is over.
        finished = progress;
        continue;
      }
      emit(progress);
      final now = DateTime.now();
      if (now.difference(lastNotify) > const Duration(seconds: 1)) {
        lastNotify = now;
        _images.clear();
        _notify();
      }
    }
    _notify();
    await _fetchMetadata(emit, errors, finished?.filesFound ?? 0);
    emit(
      ScanProgress(
        phase: ScanPhase.done,
        filesFound: finished?.filesFound ?? 0,
        processed: finished?.processed ?? 0,
        total: finished?.total,
        error: finished?.error,
        errors: List.unmodifiable([...errors, ?finished?.error]),
      ),
    );
  }

  /// Provider metadata for new and stale works (ScanPhase.metadata).
  Future<void> _fetchMetadata(
    void Function(ScanProgress) emit,
    List<String> errors,
    int filesFound,
  ) async {
    var lastNotify = DateTime.now();
    final settings = CoreMetadataSettings(
      tmdbApiKey: _tmdbKey,
      language: _language,
      cacheDir: _metadataCacheDir,
    );
    await for (final u in _core.fetchMetadata(settings: settings)) {
      final error = u.error == null ? null : 'Metadata: ${u.error}';
      emit(
        ScanProgress(
          phase: ScanPhase.metadata,
          filesFound: filesFound,
          processed: u.processed,
          total: u.total,
          error: error,
          errors: List.unmodifiable([...errors, ?error]),
        ),
      );
      final now = DateTime.now();
      if (u.done || now.difference(lastNotify) > const Duration(seconds: 1)) {
        lastNotify = now;
        _notify();
      }
    }
  }

  // -------------------------------------------------------------------------
  // items

  String _filePath(CoreItem i) =>
      _displayPath(i.localPath, i.sourceId, i.relPath);

  /// Local path, else the source address and [relPath] (network files).
  String _displayPath(String? local, int sourceId, String relPath) {
    if (local != null) return local;
    final uri = _sourceUris[sourceId];
    if (uri == null) return relPath;
    final root = uri.endsWith('/') ? uri.substring(0, uri.length - 1) : uri;
    return relPath.isEmpty ? root : '$root/$relPath';
  }

  /// Stream URL of a file on a network source; null for local files.
  String? _streamUri(CoreItem i) =>
      _fileStreamUri(i.localPath, i.sourceId, i.relPath);

  String? _fileStreamUri(String? local, int sourceId, String relPath) {
    final base = _streamBase;
    if (local != null || base == null) return null;
    return streamUrl(base, sourceId, relPath);
  }

  /// Proxy URL of the local poster of a network item, if the core has one.
  String? _localPosterUri(CoreItem i) {
    final base = _streamBase;
    final rel = i.localPoster;
    if (base == null || rel == null || rel.isEmpty) return null;
    return streamUrl(base, i.sourceId.toInt(), rel);
  }

  /// URL of [relPath] of source [sourceId] on the core's stream proxy at
  /// [base]: every path segment percent-encoded.
  static String streamUrl(String base, int sourceId, String relPath) {
    final segments = relPath
        .split('/')
        .where((s) => s.isNotEmpty)
        .map(Uri.encodeComponent);
    return [base, '$sourceId', ...segments].join('/');
  }

  String? _image(String mediaPath, {bool video = false, bool generic = true}) {
    final key = '$mediaPath|$video|$generic';
    return _images.putIfAbsent(
      key,
      () => findLocalImage(mediaPath, video: video, allowGeneric: generic),
    );
  }

  LibraryItem _item(CoreItem i) {
    final path = _filePath(i);
    final network = i.localPath == null;
    final kind = switch (i.kind) {
      CoreItemKind.movie => ItemKind.movie,
      CoreItemKind.series => ItemKind.show,
      CoreItemKind.episode => ItemKind.episode,
      CoreItemKind.track => ItemKind.track,
    };
    final poster = switch (kind) {
      // Network sources: the poster or cover the core found next to the
      // files (LIB-007 groups), through the stream proxy (LIB-019).
      _ when network => _localPosterUri(i),
      // Generic names (`poster.jpg`) only for a movie in its own folder.
      ItemKind.movie => _image(
        path,
        video: true,
        generic: i.relPath.contains('/'),
      ),
      ItemKind.show => _image(
        p.join(path, 'x.mkv'),
        video: true,
        generic: i.relPath.isNotEmpty,
      ),
      ItemKind.episode => null,
      ItemKind.track => _image(path),
    };
    // Provider art first (as v0: TMDB poster, else the local image).
    final posterUrl = i.posterUrl ?? poster;
    final title = i.title.isNotEmpty
        ? i.title
        : (kind == ItemKind.episode
              ? 'Episode ${i.episode ?? ''}'
              : p.basename(path));
    return LibraryItem(
      id: i.id,
      kind: kind,
      title: title,
      originalTitle: i.originalTitle,
      year: i.year,
      overview: i.overview,
      posterUrl: posterUrl,
      backdropUrl: i.backdropUrl,
      showId: i.showId,
      season: i.season,
      episode: i.episode,
      artist: i.artist,
      album: i.album,
      trackNumber: i.trackNumber,
      filePath: path,
      streamUri: kind == ItemKind.show ? null : _streamUri(i),
      durationMs: i.durationMs,
      matchConfidence: i.matchConfidence,
      tmdbId: i.tmdbId,
      addedAt: i.addedAtMs == null
          ? null
          : DateTime.fromMillisecondsSinceEpoch(i.addedAtMs!),
      versionCount: i.versionCount,
    );
  }

  Progress _progress(CoreProgress pr) => Progress(
    itemId: pr.itemId,
    positionMs: pr.positionMs,
    durationMs: pr.durationMs,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(pr.updatedAtMs),
    watched: pr.watched,
  );

  void _notify() {
    if (!_closed && _changes.hasListener) _changes.add(null);
  }

  /// Emits [query] now and after every change. A result is dropped if a
  /// newer query finished first, so a slow query never overwrites a fresh
  /// one.
  Stream<T> _watch<T>(Future<T> Function() query, {Stream<void>? also}) =>
      Stream.multi((c) {
    var started = 0;
    var shown = 0;
    Future<void> emit() async {
      if (_closed) return;
      final n = ++started;
      try {
        final v = await query();
        if (n > shown && !c.isClosed) {
          shown = n;
          c.add(v);
        }
      } catch (e, st) {
        if (!_closed && !c.isClosed) c.addError(e, st);
      }
    }

    emit();
    final sub = _changes.stream.listen((_) => emit());
    final other = also?.listen((_) => emit());
    c.onCancel = () async {
      await sub.cancel();
      await other?.cancel();
    };
  });

  Future<List<LibraryItem>> _items(Future<List<CoreItem>> f) async =>
      (await f).map(_item).toList();

  @override
  Stream<List<LibraryItem>> movies() => _watch(() => _items(_core.movies()));

  @override
  Stream<List<LibraryItem>> shows() => _watch(() => _items(_core.shows()));

  @override
  Stream<List<LibraryItem>> episodes(String showId) =>
      _watch(() => _items(_core.episodes(showId: showId)));

  @override
  Stream<List<LibraryItem>> tracks() => _watch(() => _items(_core.tracks()));

  @override
  Stream<List<LibraryItem>> recentlyAdded({
    ItemKind kind = ItemKind.movie,
    int limit = 20,
  }) => _watch(
    () => _items(
      _core.recentlyAdded(
        kind: switch (kind) {
          ItemKind.movie => CoreItemKind.movie,
          ItemKind.show => CoreItemKind.series,
          ItemKind.episode => CoreItemKind.episode,
          ItemKind.track => CoreItemKind.track,
        },
        limit: limit,
      ),
    ),
  );

  @override
  Stream<List<(LibraryItem, Progress)>> continueWatching() => _watch(() async {
    final rows = await _core.continueWatching(limit: 50);
    return [for (final r in rows) (_item(r.item), _progress(r.progress))];
  });

  @override
  Future<LibraryItem?> item(String id) async {
    final i = await _core.item(id: id);
    return i == null ? null : _item(i);
  }

  @override
  Future<LibraryItem?> nextEpisode(String itemId) async {
    final current = await item(itemId);
    final showId = current?.showId;
    if (current == null || current.kind != ItemKind.episode || showId == null) {
      return null;
    }
    return nextEpisodeIn(await _items(_core.episodes(showId: showId)), current);
  }

  @override
  Future<Progress?> progress(String itemId) async {
    final pr = await _core.progress(itemId: itemId);
    return pr == null ? null : _progress(pr);
  }

  @override
  Future<List<LibraryItem>> search(String query, {int limit = 100}) async {
    if (query.trim().isEmpty) return const [];
    return _items(_core.search(query: query, limit: limit.clamp(1, 500)));
  }

  @override
  Future<List<SearchGroup>> searchGrouped(
    String query, {
    int perGroup = searchPerGroupDefault,
  }) async {
    if (query.trim().isEmpty) return const [];
    final groups = await _core.searchGrouped(
      query: query,
      perGroup: perGroup.clamp(1, 500),
    );
    return [
      for (final g in groups)
        SearchGroup(_kindOf(g.kind), g.items.map(_item).toList()),
    ];
  }

  @override
  Future<void> saveProgress(Progress pr) async {
    await _core.saveProgress(
      itemId: pr.itemId,
      positionMs: pr.positionMs,
      durationMs: pr.durationMs,
      updatedAtMs: pr.updatedAt.millisecondsSinceEpoch,
      watched: pr.watched,
    );
    _notify();
  }

  @override
  Future<void> fixMatch(
    String itemId, {
    int? tmdbId,
    String? title,
    int? year,
  }) async {
    if (await _core.item(id: itemId) == null) {
      throw ArgumentError.value(itemId, 'itemId', 'no such item');
    }
    await _core.fixMatch(
      itemId: itemId,
      title: title,
      year: year,
      tmdbId: tmdbId,
    );
    _notify();
  }

  // -------------------------------------------------------------------------
  // library queries (LIB-009)

  static CoreItemKind _workKind(ItemKind kind) => switch (kind) {
    ItemKind.movie => CoreItemKind.movie,
    ItemKind.show => CoreItemKind.series,
    _ => throw ArgumentError.value(
      kind,
      'kind',
      'queries list movies or shows',
    ),
  };

  static ItemKind _kindOf(CoreItemKind kind) => switch (kind) {
    CoreItemKind.movie => ItemKind.movie,
    CoreItemKind.series => ItemKind.show,
    CoreItemKind.episode => ItemKind.episode,
    CoreItemKind.track => ItemKind.track,
  };

  static CoreLibrarySort _coreSort(LibrarySort s) => CoreLibrarySort(
    field: CoreSortField.values[s.field.index],
    descending: s.descending,
  );

  static LibrarySort _sort(CoreLibrarySort s) => LibrarySort(
    field: SortField.values[s.field.index],
    descending: s.descending,
  );

  static CoreLibraryFilter _coreFilter(LibraryFilter f) => CoreLibraryFilter(
    genres: f.genres,
    yearMin: f.yearMin,
    yearMax: f.yearMax,
    watched: [for (final w in f.watched) CoreWatchState.values[w.index]],
    resolutions: [
      for (final r in f.resolutions) CoreResolution.values[r.index],
    ],
    hdr: [for (final h in f.hdr) CoreHdrFilter.values[h.index]],
    audioFormats: f.audioFormats,
    audioLanguages: f.audioLanguages,
    sources: Int64List.fromList([
      for (final id in f.sources) ?int.tryParse(id),
    ]),
  );

  static LibraryFilter _filter(CoreLibraryFilter f) => LibraryFilter(
    genres: f.genres,
    yearMin: f.yearMin,
    yearMax: f.yearMax,
    watched: {for (final w in f.watched) WatchState.values[w.index]},
    resolutions: {
      for (final r in f.resolutions) VideoResolution.values[r.index],
    },
    hdr: {for (final h in f.hdr) HdrFilter.values[h.index]},
    audioFormats: f.audioFormats,
    audioLanguages: f.audioLanguages,
    sources: [for (final id in f.sources) '$id'],
  );

  static CoreLibraryQuery _coreQuery(LibraryQuery q) => CoreLibraryQuery(
    kind: _workKind(q.kind),
    filter: _coreFilter(q.filter),
    sort: _coreSort(q.sort),
  );

  static LibraryQuery _query(CoreLibraryQuery q) => LibraryQuery(
    _kindOf(q.kind),
    filter: _filter(q.filter),
    sort: _sort(q.sort),
  );

  LibraryPage _page(CoreItemPage p) => LibraryPage(
    items: p.items.map(_item).toList(),
    total: p.total,
    offset: p.offset,
  );

  SmartShelf _shelf(CoreSmartShelf s) => SmartShelf(
    id: '${s.id}',
    name: s.name,
    query: _query(s.query),
    pinnedHome: s.pinnedHome,
    position: s.position,
  );

  /// One page of [query]: [limit] items from [offset], and the total.
  Future<LibraryPage> query(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) async => _page(
    await _core.query(
      query: _coreQuery(query),
      offset: offset.clamp(0, 0xffffffff),
      limit: limit.clamp(0, 0xffffffff),
    ),
  );

  /// [query] now and after every library change.
  Stream<LibraryPage> watchQuery(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) => _watch(() => this.query(query, offset: offset, limit: limit));

  /// Alphabet index of [query] in title order (descending if the query
  /// sorts by title descending).
  Future<List<AlphabetEntry>> alphabetIndex(LibraryQuery query) async => [
    for (final e in await _core.alphabetIndex(query: _coreQuery(query)))
      AlphabetEntry(letter: e.letter, offset: e.offset, count: e.count),
  ];

  /// Filter values present among movies or shows, for filter chips.
  Future<LibraryFacets> facets(ItemKind kind) async {
    final f = await _core.facets(kind: _workKind(kind));
    List<FacetCount<String>> text(List<CoreTextFacet> v) => [
      for (final x in v) FacetCount(x.value, x.count),
    ];
    return LibraryFacets(
      genres: text(f.genres),
      yearMin: f.yearMin,
      yearMax: f.yearMax,
      resolutions: [
        for (final r in f.resolutions)
          FacetCount(VideoResolution.values[r.value.index], r.count),
      ],
      hdr: [
        for (final h in f.hdr)
          FacetCount(HdrFilter.values[h.value.index], h.count),
      ],
      audioFormats: text(f.audioFormats),
      audioLanguages: text(f.audioLanguages),
      sources: [
        for (final s in f.sources) FacetCount('${s.sourceId}', s.count),
      ],
    );
  }

  /// Entries of folder [dir] (relative to the source root, `''` = root) of
  /// a source: subfolders and media files, sorted by [sort] (title = name),
  /// folders first if [foldersFirst].
  Future<List<FolderEntry>> browse(
    String sourceId, {
    String dir = '',
    LibrarySort sort = const LibrarySort(),
    bool foldersFirst = true,
  }) async {
    final id = int.tryParse(sourceId);
    if (id == null) return const [];
    return [
      for (final e in await _core.browse(
        sourceId: id,
        dir: dir,
        sort: _coreSort(sort),
        foldersFirst: foldersFirst,
      ))
        FolderEntry(
          name: e.name,
          relPath: e.relPath,
          isFolder: e.isFolder,
          itemId: e.itemId,
          sizeBytes: e.sizeBytes,
          fileCount: e.fileCount,
          addedAt: DateTime.fromMillisecondsSinceEpoch(e.addedAtMs),
          durationMs: e.durationMs,
        ),
    ];
  }

  /// Saved preferences of [view] (`movies`, `shows`, `folder:<source>`…),
  /// or its defaults: by title, folders first, no filter.
  Future<ViewPrefs> viewPrefs(String view) async {
    final p = await _core.viewPrefs(view: view);
    final filter = p.filter;
    return ViewPrefs(
      p.view,
      sort: _sort(p.sort),
      foldersFirst: p.foldersFirst,
      filter: filter == null ? null : _filter(filter),
    );
  }

  /// Saves the preferences of [prefs.view]; each view keeps its own.
  Future<void> setViewPrefs(ViewPrefs prefs) async {
    final filter = prefs.filter;
    await _core.setViewPrefs(
      prefs: CoreViewPrefs(
        view: prefs.view,
        sort: _coreSort(prefs.sort),
        foldersFirst: prefs.foldersFirst,
        filter: filter == null ? null : _coreFilter(filter),
      ),
    );
  }

  /// Smart shelves by position; [pinnedOnly]: those pinned to Home.
  Future<List<SmartShelf>> shelves({bool pinnedOnly = false}) async => [
    for (final s in await _core.shelves(pinnedOnly: pinnedOnly)) _shelf(s),
  ];

  /// Saves [query] as a new shelf at the end of the list.
  Future<SmartShelf> createShelf(
    String name,
    LibraryQuery query, {
    bool pinnedHome = false,
  }) async {
    final s = await _core.createShelf(
      name: name,
      query: _coreQuery(query),
      pinnedHome: pinnedHome,
    );
    _notify();
    return _shelf(s);
  }

  Future<void> updateShelf(
    String id,
    String name,
    LibraryQuery query, {
    bool pinnedHome = false,
  }) async {
    await _core.updateShelf(
      id: _shelfId(id),
      name: name,
      query: _coreQuery(query),
      pinnedHome: pinnedHome,
    );
    _notify();
  }

  Future<void> deleteShelf(String id) async {
    final n = int.tryParse(id);
    if (n == null) return;
    await _core.deleteShelf(id: n);
    _notify();
  }

  /// Puts the shelves in the order of [ids]; unlisted ones follow.
  Future<void> reorderShelves(List<String> ids) async {
    await _core.reorderShelves(
      ids: Int64List.fromList([for (final id in ids) ?int.tryParse(id)]),
    );
    _notify();
  }

  /// One page of a shelf's query.
  Future<LibraryPage> shelfItems(
    String id, {
    int offset = 0,
    int limit = 100,
  }) async => _page(
    await _core.shelfItems(
      id: _shelfId(id),
      offset: offset.clamp(0, 0xffffffff),
      limit: limit.clamp(0, 0xffffffff),
    ),
  );

  /// Genres of a movie or show.
  Future<List<String>> genres(String itemId) => _core.genres(itemId: itemId);

  static int _shelfId(String id) =>
      int.tryParse(id) ??
      (throw ArgumentError.value(id, 'id', 'no such shelf'));

  // -------------------------------------------------------------------------
  // versions, favorites, collections, playlists (LIB-010)

  static int _listId(String id, String what) =>
      int.tryParse(id) ??
      (throw ArgumentError.value(id, 'id', 'no such $what'));

  MediaVersion _version(CoreVersion v) => MediaVersion(
    id: '${v.fileId}',
    label: v.label,
    resolution: v.resolution,
    hdr: v.hdr,
    edition: v.edition,
    filePath: _displayPath(v.localPath, v.sourceId, v.relPath),
    streamUri: _fileStreamUri(v.localPath, v.sourceId, v.relPath),
    sizeBytes: v.sizeBytes,
    durationMs: v.durationMs,
    isDefault: v.isDefault,
  );

  static MediaCollection _collection(CoreCollection c) => MediaCollection(
    id: '${c.id}',
    name: c.name,
    external: c.isExternal,
    itemCount: c.itemCount,
  );

  static VideoPlaylist _playlist(CorePlaylist p) => VideoPlaylist(
    id: '${p.id}',
    name: p.name,
    itemCount: p.itemCount,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(p.updatedAtMs),
  );

  @override
  Future<List<MediaVersion>> versions(String itemId) async => [
    for (final v in await _core.versions(itemId: itemId)) _version(v),
  ];

  @override
  Future<void> setDefaultVersion(String itemId, String? versionId) async {
    await _core.setDefaultVersion(
      itemId: itemId,
      fileId: versionId == null ? null : _listId(versionId, 'version'),
    );
    _notify();
  }

  @override
  Stream<List<LibraryItem>> favorites({bool pinnedOnly = false}) =>
      _watch(() => _items(_core.favorites(pinnedOnly: pinnedOnly)));

  @override
  Future<bool> isFavorite(String itemId) => _core.isFavorite(itemId: itemId);

  @override
  Future<void> setFavorite(String itemId, bool favorite) async {
    await _core.setFavorite(itemId: itemId, favorite: favorite);
    _notify();
  }

  @override
  Future<void> setFavoritePinned(String itemId, bool pinned) async {
    await _core.setFavoritePinned(itemId: itemId, pinned: pinned);
    _notify();
  }

  @override
  Future<List<MediaCollection>> collections() async => [
    for (final c in await _core.collections()) _collection(c),
  ];

  @override
  Future<List<MediaCollection>> itemCollections(String itemId) async => [
    for (final c in await _core.itemCollections(itemId: itemId)) _collection(c),
  ];

  @override
  Future<List<LibraryItem>> collectionItems(String id) =>
      _items(_core.collectionItems(id: _listId(id, 'collection')));

  @override
  Future<MediaCollection> createCollection(String name) async {
    final c = await _core.createCollection(name: name);
    _notify();
    return _collection(c);
  }

  @override
  Future<void> renameCollection(String id, String name) async {
    await _core.renameCollection(id: _listId(id, 'collection'), name: name);
    _notify();
  }

  @override
  Future<void> deleteCollection(String id) async {
    await _core.deleteCollection(id: _listId(id, 'collection'));
    _notify();
  }

  @override
  Future<bool> addToCollection(String id, String itemId) async {
    final added = await _core.addToCollection(
      id: _listId(id, 'collection'),
      itemId: itemId,
    );
    _notify();
    return added;
  }

  @override
  Future<bool> removeFromCollection(String id, String itemId) async {
    final removed = await _core.removeFromCollection(
      id: _listId(id, 'collection'),
      itemId: itemId,
    );
    _notify();
    return removed;
  }

  @override
  Future<List<VideoPlaylist>> playlists() async => [
    for (final p in await _core.playlists()) _playlist(p),
  ];

  @override
  Future<VideoPlaylist> createPlaylist(String name) async {
    final p = await _core.createPlaylist(name: name);
    _notify();
    return _playlist(p);
  }

  @override
  Future<void> renamePlaylist(String id, String name) async {
    await _core.renamePlaylist(id: _listId(id, 'playlist'), name: name);
    _notify();
  }

  @override
  Future<void> deletePlaylist(String id) async {
    await _core.deletePlaylist(id: _listId(id, 'playlist'));
    _notify();
  }

  @override
  Future<List<LibraryItem>> playlistItems(String id) =>
      _items(_core.playlistItems(id: _listId(id, 'playlist')));

  @override
  Future<int> addToPlaylist(String id, String itemId) async {
    final n = await _core.addToPlaylist(
      id: _listId(id, 'playlist'),
      itemId: itemId,
    );
    _notify();
    return n;
  }

  @override
  Future<int> removeFromPlaylist(String id, String itemId) async {
    final n = await _core.removeFromPlaylist(
      id: _listId(id, 'playlist'),
      itemId: itemId,
    );
    _notify();
    return n;
  }

  @override
  Future<void> reorderPlaylist(String id, List<String> itemIds) async {
    await _core.reorderPlaylist(id: _listId(id, 'playlist'), itemIds: itemIds);
    _notify();
  }

  // -------------------------------------------------------------------------
  // music views, plays, audio playlists (MUS-004)

  MusicTrack _musicTrack(CoreMusicTrack t) => MusicTrack(
    item: _item(t.item),
    albumId: t.albumId == null ? null : '${t.albumId}',
    albumArtists: t.albumArtists,
    albumYear: t.albumYear,
    compilation: t.compilation,
    artists: t.artists,
    composers: t.composers,
    genres: t.genres,
    discNumber: t.discNumber,
    discTotal: t.discTotal,
    codec: t.codec,
    lossless: t.lossless,
    sampleRateHz: t.sampleRateHz,
    bitDepth: t.bitDepth,
    channels: t.channels,
    bitrateKbps: t.bitrateKbps,
    local: t.local,
    playCount: t.playCount,
    lastPlayedAt: t.lastPlayedAtMs == null
        ? null
        : DateTime.fromMillisecondsSinceEpoch(t.lastPlayedAtMs!),
  );

  static MusicPlaylist _musicPlaylist(CorePlaylist p) => MusicPlaylist(
    id: '${p.id}',
    name: p.name,
    trackCount: p.itemCount,
    updatedAt: DateTime.fromMillisecondsSinceEpoch(p.updatedAtMs),
  );

  void _notifyMusic() {
    if (!_closed && _musicChanges.hasListener) _musicChanges.add(null);
  }

  @override
  Stream<List<MusicTrack>> musicTracks() => _watch(
    () async => [for (final t in await _core.musicTracks()) _musicTrack(t)],
    also: _musicChanges.stream,
  );

  @override
  Future<void> recordTrackPlay(String trackId) async {
    await _core.recordTrackPlay(itemId: trackId);
    _notifyMusic();
  }

  @override
  bool get hasMusicPlaylists => true;

  @override
  Stream<List<MusicPlaylist>> musicPlaylists() => _watch(
    () async => [for (final p in await _core.musicPlaylists()) _musicPlaylist(p)],
    also: _musicChanges.stream,
  );

  @override
  Future<MusicPlaylist> createMusicPlaylist(String name) async {
    final p = await _core.createMusicPlaylist(name: name);
    _notifyMusic();
    return _musicPlaylist(p);
  }

  @override
  Future<void> renameMusicPlaylist(String id, String name) async {
    await _core.renameMusicPlaylist(id: _listId(id, 'playlist'), name: name);
    _notifyMusic();
  }

  @override
  Future<void> deleteMusicPlaylist(String id) async {
    await _core.deleteMusicPlaylist(id: _listId(id, 'playlist'));
    _notifyMusic();
  }

  @override
  Future<List<LibraryItem>> musicPlaylistItems(String id) =>
      _items(_core.musicPlaylistItems(id: _listId(id, 'playlist')));

  @override
  Future<bool> addToMusicPlaylist(String id, String trackId) async {
    final added = await _core.addToMusicPlaylist(
      id: _listId(id, 'playlist'),
      itemId: trackId,
    );
    _notifyMusic();
    return added;
  }

  @override
  Future<bool> removeFromMusicPlaylist(String id, String trackId) async {
    final removed = await _core.removeFromMusicPlaylist(
      id: _listId(id, 'playlist'),
      itemId: trackId,
    );
    _notifyMusic();
    return removed;
  }

  // -------------------------------------------------------------------------
  // sidecar subtitles (PLAY-007)

  @override
  Future<List<SidecarSubtitle>> sidecarSubtitles(LibraryItem item) async {
    final file = fileOf(item.playbackUri);
    if (file == null) return const [];
    final (sourceId, relPath) = file;
    final subs = await _core.sidecarSubtitles(
      sourceId: sourceId,
      relPath: relPath,
    );
    final base = _streamBase;
    return [
      for (final s in subs)
        if (s.localPath != null || base != null)
          SidecarSubtitle(
            uri: s.localPath ?? streamUrl(base!, sourceId, s.relPath),
            name: s.relPath.split('/').last,
            language: s.lang,
            forced: s.forced,
            hearingImpaired: s.hearingImpaired,
            isDefault: s.isDefault,
          ),
    ];
  }

  /// The source and the path in it of a file the player is given: a stream
  /// URL of this library's proxy, or a local path under a `file://`
  /// source; null for anything else.
  (int, String)? fileOf(String playbackUri) {
    final base = _streamBase;
    if (base != null && playbackUri.startsWith('$base/')) {
      return parseStreamUrl(base, playbackUri);
    }
    String path = playbackUri;
    if (path.startsWith('file://')) path = Uri.parse(path).toFilePath();
    if (!p.isAbsolute(path)) return null;
    (int, String)? best;
    var bestLength = -1;
    for (final e in _localRoots.entries) {
      final root = p.normalize(e.value);
      if (root.length > bestLength && p.isWithin(root, path)) {
        best = (e.key, p.relative(path, from: root).replaceAll(r'\', '/'));
        bestLength = root.length;
      }
    }
    return best;
  }

  /// Inverse of [streamUrl]: the source id and the decoded path of a stream
  /// URL under [base]; null if it is not one.
  static (int, String)? parseStreamUrl(String base, String url) {
    if (!url.startsWith('$base/')) return null;
    final segments = url.substring(base.length + 1).split('/');
    final id = int.tryParse(segments.first);
    if (id == null || segments.length < 2) return null;
    try {
      return (id, segments.skip(1).map(Uri.decodeComponent).join('/'));
    } on ArgumentError {
      return null;
    }
  }

  // -------------------------------------------------------------------------
  // change watcher (LIB-005)

  StreamSubscription<CoreWatchEvent>? _watchSub;
  bool _metadataQueued = false;

  /// Rescans done by the watcher, for diagnostics and tests.
  Stream<CoreWatchEvent> get watchEvents => _watchEvents.stream;
  final _watchEvents = StreamController<CoreWatchEvent>.broadcast();

  /// Starts the core watcher: local auto-scan folders through OS change
  /// notifications with incremental rescans, network sources (read by the
  /// core through `pa-vfs`) on a [networkInterval] timer.
  @override
  Future<void> startWatching({
    Duration networkInterval = const Duration(minutes: 15),
    bool scanOnStart = true,
  }) async {
    if (_closed) return;
    await stopWatching();
    await sources();
    if (_closed) return;
    _watchSub = _core
        .startWatcher(
          settings: CoreWatchSettings(
            networkIntervalSecs: networkInterval.inSeconds,
            debounceMs: 1000,
            scanOnStart: scanOnStart,
          ),
        )
        .listen(_onWatchEvent, onError: (Object _) {});
  }

  void _onWatchEvent(CoreWatchEvent e) {
    if (_closed) return;
    _watchEvents.add(e);
    if (!e.mayHaveChanged) return;
    _images.clear();
    _notify();
    _queueMetadata();
  }

  /// Provider metadata for what the watcher found, after the scans queued
  /// before it; one run at a time.
  void _queueMetadata() {
    if (_metadataQueued || _closed) return;
    _metadataQueued = true;
    final prev = _scanQueue;
    final done = Completer<void>();
    _scanQueue = done.future;
    () async {
      await prev;
      _metadataQueued = false;
      try {
        if (!_closed) await _fetchMetadata((_) {}, [], 0);
      } catch (_) {
        // Retried after the next change or scan.
      } finally {
        done.complete();
        _images.clear();
        _notify();
      }
    }();
  }

  @override
  Future<void> stopWatching() async {
    final sub = _watchSub;
    _watchSub = null;
    if (sub == null) return;
    await _core.stopWatcher();
    await sub.cancel();
  }

  @override
  Future<void> close() async {
    if (_closed) return;
    await stopWatching();
    await _scanQueue;
    _closed = true;
    await _watchEvents.close();
    await _changes.close();
    await _musicChanges.close();
    _core.dispose();
    final tmp = _tempDir;
    if (tmp != null) {
      try {
        Directory(tmp).deleteSync(recursive: true);
      } on FileSystemException {
        // Best effort: the OS cleans its temp folder.
      }
    }
  }
}
