// SPDX-License-Identifier: MPL-2.0

import 'dart:async';
import 'dart:io';

import 'package:flutter_rust_bridge/flutter_rust_bridge_for_generated.dart'
    show ExternalLibrary;
import 'package:pa_core/pa_core.dart';
import 'package:path/path.dart' as p;

import 'library.dart';
import 'models.dart';
import 'music_tags.dart';
import 'smb.dart';

/// [Library] on the Rust core (UI-024): `pa_core`'s [CoreLibrary] over the
/// `pa-library` schema v1 — `pa-scanner` with music tags, the
/// `pa-recognition` parser, FTS5 search, progress with "Continue watching"
/// and Up Next. Screens see the same [Library] interface as with the Dart
/// v0 implementation ([SqliteLibrary]).
///
/// Differences from v0 for now: no TMDB metadata yet (the key is kept and
/// returned, matching against TMDB comes with META-013), so titles are the
/// recognised ones and posters are local images next to the files.
class RustLibrary implements Library {
  final CoreLibrary _core;
  final SmbResolver _smb;
  final String? _tempDir;
  final _changes = StreamController<void>.broadcast();
  Future<void> _scanQueue = Future.value();
  bool _closed = false;
  String? _tmdbKey;

  /// Local mount paths of SMB sources (by source id), from the last scan or
  /// an existing mount.
  final Map<int, String> _smbRoots = {};

  /// Local images per folder: `findLocalImage` results by `(path, mode)`.
  final Map<String, String?> _images = {};

  RustLibrary._(this._core, this._smb, this._tempDir);

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
  /// file is renamed to `*.v0.bak` after a successful import
  /// (docs/dev/db.md); keep it while the Rust core is behind a flag, so the
  /// app can still go back to v0.
  static Future<RustLibrary> open(
    String dbPath, {
    String? legacyV0Path,
    bool renameLegacyV0 = true,
    SmbResolver? smbResolver,
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
    final lib = RustLibrary._(core, smbResolver ?? SmbResolver(), tempDir);
    if (legacyV0Path != null) {
      await lib.importLegacyV0(legacyV0Path, rename: renameLegacyV0);
    }
    await lib._findMountedShares();
    return lib;
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
      for (final suffix in ['', '-wal', '-shm']) {
        final f = File('$path$suffix');
        if (f.existsSync()) f.renameSync('$path.v0.bak$suffix');
      }
    }
    _notify();
    return report;
  }

  Future<void> _findMountedShares() async {
    for (final s in await _core.sources()) {
      if (s.kind != 'smb') continue;
      final root = await _smb.mounted(s.uri);
      if (root != null) _smbRoots[s.id] = root;
    }
  }

  // -------------------------------------------------------------------------
  // settings

  @override
  Future<void> setTmdbApiKey(String? key) async {
    final k = key?.trim();
    _tmdbKey = (k == null || k.isEmpty) ? null : k;
  }

  @override
  Future<String?> tmdbApiKey() async => _tmdbKey;

  @override
  Future<void> setMetadataLanguage(String? language) async {}

  // -------------------------------------------------------------------------
  // sources

  MediaSource _source(CoreSource s) => MediaSource(
    id: '${s.id}',
    kind: s.kind == 'smb' ? SourceKind.smb : SourceKind.localFolder,
    name: s.name,
    path: s.localRoot ?? s.uri,
    username: s.username,
  );

  @override
  Future<List<MediaSource>> sources() async =>
      (await _core.sources()).map(_source).toList();

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
    _smbRoots.remove(n);
    _notify();
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
    String? smbPassword,
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

    // Network shares are mounted here (gio) and scanned as local folders.
    final roots = <CoreSourceRoot>[];
    for (final s in await _core.sources()) {
      if (s.kind != 'smb' || (id != null && s.id != id)) continue;
      emit(
        ScanProgress(
          phase: ScanPhase.connecting,
          sourceId: '${s.id}',
          currentPath: s.uri,
          errors: List.unmodifiable(errors),
        ),
      );
      try {
        final root = await _smb.resolve(
          s.uri,
          username: s.username,
          password: smbPassword,
        );
        _smbRoots[s.id] = root;
        roots.add(CoreSourceRoot(sourceId: s.id, path: root));
      } catch (e) {
        final msg = '${s.name}: $e';
        emit(
          ScanProgress(
            phase: ScanPhase.connecting,
            sourceId: '${s.id}',
            error: msg,
            errors: List.unmodifiable([...errors, msg]),
          ),
        );
      }
    }

    var lastNotify = DateTime.now();
    await for (final u in _core.scan(sourceId: id, roots: roots)) {
      final phase = switch (u.phase) {
        CoreScanPhase.connecting => ScanPhase.connecting,
        CoreScanPhase.scanning => ScanPhase.scanning,
        CoreScanPhase.analyzing => ScanPhase.metadata,
        CoreScanPhase.done => ScanPhase.done,
      };
      emit(
        ScanProgress(
          phase: phase,
          sourceId: u.sourceId?.toString(),
          filesFound: u.filesFound,
          processed: u.processed,
          total: u.total,
          currentPath: u.currentPath,
          error: u.error,
          errors: List.unmodifiable([...errors, ?u.error]),
        ),
      );
      final now = DateTime.now();
      if (now.difference(lastNotify) > const Duration(seconds: 1)) {
        lastNotify = now;
        _images.clear();
        _notify();
      }
    }
  }

  // -------------------------------------------------------------------------
  // items

  String _filePath(CoreItem i) {
    final local = i.localPath;
    if (local != null) return local;
    final root = _smbRoots[i.sourceId];
    return root == null ? i.relPath : p.join(root, i.relPath);
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
    final kind = switch (i.kind) {
      CoreItemKind.movie => ItemKind.movie,
      CoreItemKind.series => ItemKind.show,
      CoreItemKind.episode => ItemKind.episode,
      CoreItemKind.track => ItemKind.track,
    };
    final poster = switch (kind) {
      // Generic names (`poster.jpg`) only for a movie in its own folder.
      ItemKind.movie => _image(path, video: true, generic: i.relPath.contains('/')),
      ItemKind.show => _image(
        p.join(path, 'x.mkv'),
        video: true,
        generic: i.relPath.isNotEmpty,
      ),
      ItemKind.episode => null,
      ItemKind.track => _image(path),
    };
    final title = i.title.isNotEmpty
        ? i.title
        : (kind == ItemKind.episode ? 'Episode ${i.episode ?? ''}' : p.basename(path));
    return LibraryItem(
      id: i.id,
      kind: kind,
      title: title,
      originalTitle: i.originalTitle,
      year: i.year,
      overview: i.overview,
      posterUrl: poster,
      showId: i.showId,
      season: i.season,
      episode: i.episode,
      artist: i.artist,
      album: i.album,
      trackNumber: i.trackNumber,
      filePath: path,
      durationMs: i.durationMs,
      matchConfidence: i.matchConfidence,
      tmdbId: i.tmdbId,
      addedAt: i.addedAtMs == null
          ? null
          : DateTime.fromMillisecondsSinceEpoch(i.addedAtMs!),
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
  Stream<T> _watch<T>(Future<T> Function() query) => Stream.multi((c) {
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
    c.onCancel = sub.cancel;
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

  @override
  Future<void> close() async {
    if (_closed) return;
    await _scanQueue;
    _closed = true;
    await _changes.close();
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
