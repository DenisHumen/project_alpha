// SPDX-License-Identifier: MPL-2.0
//
// In-memory implementation of the pa_library `Library` interface for the demo
// mode, screenshots and widget tests.
//
// * `FakeLibrary()` starts empty (onboarding is shown); a scan "finds" the
//   demo library so the whole flow can be tried without media files.
// * `FakeLibrary.demo()` starts with the demo library
//   (`--dart-define=KINOLUNE_DEMO=true`).

import 'dart:async';

import '../integration/library.dart';
import 'demo_data.dart';

class FakeLibrary implements Library {
  FakeLibrary({this.scanStepDelay = const Duration(milliseconds: 60)});

  FakeLibrary.demo({this.scanStepDelay = const Duration(milliseconds: 60)}) {
    _sources.add(
      const MediaSource(
        id: 'src-demo',
        kind: SourceKind.localFolder,
        name: 'Demo library',
        path: '/media',
      ),
    );
    _load(buildDemoLibraryData());
  }

  final Duration scanStepDelay;
  final List<MediaSource> _sources = [];
  final _items = _Live<List<LibraryItem>>(const []);
  final _progress = _Live<Map<String, Progress>>(const {});
  String? _tmdbApiKey;
  int _nextId = 1;

  void _load(DemoLibraryData d) {
    _items.value = [...d.movies, ...d.shows, ...d.episodes, ...d.tracks];
    _progress.value = {for (final p in d.progress) p.itemId: p};
  }

  @override
  Future<List<MediaSource>> sources() async => List.unmodifiable(_sources);

  @override
  Future<MediaSource> addSource(MediaSource s) async {
    final src = s.id.isEmpty ? s.copyWith(id: 'src-${_nextId++}') : s;
    // An existing id updates the source, as in the real libraries.
    final i = _sources.indexWhere((x) => x.id == src.id);
    if (i >= 0) {
      _sources[i] = src;
    } else {
      _sources.add(src);
    }
    return src;
  }

  @override
  Future<void> removeSource(String id) async {
    _sources.removeWhere((s) => s.id == id);
    if (_sources.isEmpty) {
      _items.value = const [];
      _progress.value = const {};
    }
  }

  @override
  Stream<ScanProgress> scan({String? sourceId, String? smbPassword}) async* {
    if (_sources.isEmpty) {
      yield const ScanProgress(phase: ScanPhase.done, error: 'No sources');
      return;
    }
    yield ScanProgress(phase: ScanPhase.connecting, sourceId: sourceId);
    final data = buildDemoLibraryData();
    final all = [...data.movies, ...data.episodes, ...data.tracks];
    final total = all.length;
    for (var i = 0; i < total; i += 7) {
      await Future<void>.delayed(scanStepDelay);
      yield ScanProgress(
        phase: ScanPhase.scanning,
        sourceId: sourceId,
        filesFound: total,
        processed: i,
        total: total,
        currentPath: all[i].filePath,
      );
    }
    _load(data);
    yield ScanProgress(
      phase: ScanPhase.done,
      sourceId: sourceId,
      filesFound: total,
      processed: total,
      total: total,
    );
  }

  @override
  Future<void> setTmdbApiKey(String? key) async => _tmdbApiKey = key;

  @override
  Future<String?> tmdbApiKey() async => _tmdbApiKey;

  /// The last [setMetadataLanguage] value (tests).
  String? metadataLanguage;

  @override
  Future<void> setMetadataLanguage(String? language) async =>
      metadataLanguage = language;

  Stream<List<LibraryItem>> _ofKind(
    ItemKind kind, [
    bool Function(LibraryItem)? where,
  ]) => _items.stream.map(
    (all) =>
        all.where((i) => i.kind == kind && (where?.call(i) ?? true)).toList(),
  );

  @override
  Stream<List<LibraryItem>> movies() => _ofKind(ItemKind.movie);

  @override
  Stream<List<LibraryItem>> shows() => _ofKind(ItemKind.show);

  @override
  Stream<List<LibraryItem>> episodes(String showId) =>
      _ofKind(ItemKind.episode, (i) => i.showId == showId).map(
        (l) => l
          ..sort(
            (a, b) => ((a.season ?? 0) * 1000 + (a.episode ?? 0)).compareTo(
              (b.season ?? 0) * 1000 + (b.episode ?? 0),
            ),
          ),
      );

  @override
  Stream<List<LibraryItem>> tracks() => _ofKind(ItemKind.track);

  @override
  Stream<List<LibraryItem>> recentlyAdded({
    ItemKind kind = ItemKind.movie,
    int limit = 20,
  }) => _ofKind(kind).map((l) => l.take(limit).toList());

  @override
  Future<LibraryItem?> item(String id) async {
    for (final i in _items.value) {
      if (i.id == id) return i;
    }
    return null;
  }

  @override
  Future<Progress?> progress(String itemId) async => _progress.value[itemId];

  @override
  Future<LibraryItem?> nextEpisode(String itemId) async {
    final current = await item(itemId);
    if (current == null || current.kind != ItemKind.episode) return null;
    return nextEpisodeIn(
      _items.value.where((i) => i.showId == current.showId),
      current,
    );
  }

  @override
  Future<List<LibraryItem>> search(String query, {int limit = 100}) async {
    final q = query.trim().toLowerCase();
    if (q.isEmpty) return const [];
    return _items.value
        .where(
          (i) =>
              i.kind != ItemKind.episode &&
              (i.title.toLowerCase().contains(q) ||
                  (i.artist?.toLowerCase().contains(q) ?? false) ||
                  (i.album?.toLowerCase().contains(q) ?? false)),
        )
        .take(limit)
        .toList();
  }

  @override
  Stream<List<(LibraryItem, Progress)>> continueWatching() {
    final ctrl = StreamController<List<(LibraryItem, Progress)>>();
    List<LibraryItem> items = _items.value;
    Map<String, Progress> progress = _progress.value;
    void emit() {
      final byId = {for (final i in items) i.id: i};
      final list = <(LibraryItem, Progress)>[];
      for (final p in progress.values) {
        final item = byId[p.itemId];
        if (item == null || p.durationMs <= 0) continue;
        final f = p.positionMs / p.durationMs;
        if (f >= .02 && f <= .95 && !p.watched) list.add((item, p));
      }
      list.sort((a, b) => b.$2.updatedAt.compareTo(a.$2.updatedAt));
      ctrl.add(list);
    }

    StreamSubscription<List<LibraryItem>>? s1;
    StreamSubscription<Map<String, Progress>>? s2;
    ctrl.onListen = () {
      s1 = _items.stream.listen((v) {
        items = v;
        emit();
      });
      s2 = _progress.stream.listen((v) {
        progress = v;
        emit();
      });
    };
    ctrl.onCancel = () async {
      await s1?.cancel();
      await s2?.cancel();
    };
    return ctrl.stream;
  }

  @override
  Future<void> saveProgress(Progress p) async {
    final watched = p.durationMs > 0 && p.positionMs / p.durationMs > .95;
    _progress.value = {
      ..._progress.value,
      p.itemId: Progress(
        itemId: p.itemId,
        positionMs: p.positionMs,
        durationMs: p.durationMs,
        updatedAt: p.updatedAt,
        watched: p.watched || watched,
      ),
    };
  }

  @override
  Future<void> fixMatch(
    String itemId, {
    int? tmdbId,
    String? title,
    int? year,
  }) async {
    _items.value = [
      for (final i in _items.value)
        if (i.id == itemId)
          LibraryItem(
            id: i.id,
            kind: i.kind,
            title: (title != null && title.isNotEmpty) ? title : i.title,
            filePath: i.filePath,
            originalTitle: i.originalTitle,
            year: year ?? i.year,
            overview: i.overview,
            posterUrl: i.posterUrl,
            backdropUrl: i.backdropUrl,
            showId: i.showId,
            season: i.season,
            episode: i.episode,
            artist: i.artist,
            album: i.album,
            trackNumber: i.trackNumber,
            durationMs: i.durationMs,
            matchConfidence: 1.0,
            tmdbId: tmdbId ?? i.tmdbId,
            addedAt: i.addedAt,
          )
        else
          i,
    ];
  }

  /// Watcher calls, for tests.
  int watchStarts = 0;
  bool watching = false;

  @override
  Future<void> startWatching({
    Map<String, String> smbPasswords = const {},
    Duration networkInterval = const Duration(minutes: 15),
    bool scanOnStart = true,
  }) async {
    watchStarts++;
    watching = true;
  }

  @override
  Future<void> stopWatching() async => watching = false;

  @override
  Future<void> close() async {}
}

/// A tiny "behaviour subject": new listeners get the current value first.
class _Live<T> {
  _Live(this._value);

  T _value;
  final _ctrl = StreamController<T>.broadcast();

  T get value => _value;
  set value(T v) {
    _value = v;
    _ctrl.add(v);
  }

  Stream<T> get stream async* {
    yield _value;
    yield* _ctrl.stream;
  }
}
