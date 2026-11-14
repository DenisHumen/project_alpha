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
import '../library/library_queries.dart';
import 'demo_data.dart';

class FakeLibrary implements Library, LibraryQueriesProvider {
  FakeLibrary({this.scanStepDelay = const Duration(milliseconds: 60)});

  /// A library of exactly [items] (tests: large grids), with [progress].
  FakeLibrary.items(
    List<LibraryItem> items, {
    List<Progress> progress = const [],
    this.scanStepDelay = Duration.zero,
  }) {
    _sources.add(
      const MediaSource(
        id: 'src-demo',
        kind: SourceKind.localFolder,
        name: 'Demo library',
        path: '/media',
      ),
    );
    _items.value = List.of(items);
    _progress.value = {for (final p in progress) p.itemId: p};
  }

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
  Stream<ScanProgress> scan({String? sourceId}) async* {
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

  /// Genres of the demo titles (the demo data has none): one or two from a
  /// fixed list, picked from the id.
  static List<String> demoGenres(LibraryItem item) {
    const genres = [
      'Drama', // l10n-ignore
      'Sci-Fi', // l10n-ignore
      'Comedy', // l10n-ignore
      'Thriller', // l10n-ignore
      'Adventure', // l10n-ignore
      'Mystery', // l10n-ignore
    ];
    var h = 0;
    for (final c in item.id.codeUnits) {
      h = (h * 31 + c) & 0x7fffffff;
    }
    final a = genres[h % genres.length];
    final b = genres[(h ~/ 7) % genres.length];
    return a == b ? [a] : [a, b];
  }

  @override
  late final LibraryQueries queries = MemoryLibraryQueries(
    this,
    supportsFilters: true,
    defaultSort: recentlyAddedFirst,
    genresOf: demoGenres,
    badgeSource: badges,
  );

  /// Watch badges from the stored progress: a movie by its own, a show
  /// watched when all its episodes are, in progress when one is started.
  Stream<Map<String, ItemBadge>> badges(ItemKind kind) =>
      combineLatest2(_items.stream, _progress.stream, (items, progress) {
        final c = _badgeMaps[kind];
        if (c != null && identical(c.$1, items) && identical(c.$2, progress)) {
          return c.$3;
        }
        final out = _badgesOf(kind, items, progress);
        _badgeMaps[kind] = (items, progress, out);
        return out;
      });

  final _badgeMaps =
      <
        ItemKind,
        (List<LibraryItem>, Map<String, Progress>, Map<String, ItemBadge>)
      >{};

  static Map<String, ItemBadge> _badgesOf(
    ItemKind kind,
    List<LibraryItem> items,
    Map<String, Progress> progress,
  ) {
    final out = <String, ItemBadge>{};
    if (kind == ItemKind.show) {
      final episodes = <String, List<String>>{};
      for (final i in items) {
        if (i.kind == ItemKind.episode && i.showId != null) {
          (episodes[i.showId!] ??= []).add(i.id);
        }
      }
      for (final MapEntry(key: show, value: ids) in episodes.entries) {
        final ps = [for (final id in ids) progress[id]];
        if (ps.every((p) => p?.watched ?? false)) {
          out[show] = const ItemBadge(WatchState.watched);
        } else if (ps.any((p) => p != null && p.positionMs > 0)) {
          out[show] = const ItemBadge(WatchState.inProgress);
        }
      }
      return out;
    }
    for (final i in items) {
      if (i.kind != kind) continue;
      final p = progress[i.id];
      if (p == null) continue;
      if (p.watched || p.fraction > .95) {
        out[i.id] = const ItemBadge(WatchState.watched);
      } else if (p.fraction >= .02) {
        out[i.id] = ItemBadge(WatchState.inProgress, p.fraction);
      }
    }
    return out;
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
    (all) => where == null
        ? _kindList(all, kind)
        : all.where((i) => i.kind == kind && where(i)).toList(),
  );

  // The same list for the same library state, so in-memory queries can
  // reuse their sorted result between pages.
  final _kindLists = <ItemKind, (List<LibraryItem>, List<LibraryItem>)>{};

  List<LibraryItem> _kindList(List<LibraryItem> all, ItemKind kind) {
    final c = _kindLists[kind];
    if (c != null && identical(c.$1, all)) return c.$2;
    final out = List<LibraryItem>.unmodifiable(
      all.where((i) => i.kind == kind),
    );
    _kindLists[kind] = (all, out);
    return out;
  }

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
  Future<List<SearchGroup>> searchGrouped(
    String query, {
    int perGroup = searchPerGroupDefault,
  }) async {
    final q = query.trim().toLowerCase();
    if (q.isEmpty) return const [];
    bool has(String? s) => s?.toLowerCase().contains(q) ?? false;
    final hits = _items.value.where(
      (i) =>
          has(i.title) ||
          has(i.originalTitle) ||
          has(i.artist) ||
          has(i.album) ||
          i.year?.toString() == q,
    );
    // Titles that start with the query first, as the core ranks them.
    final ranked = [
      ...hits.where((i) => i.title.toLowerCase().startsWith(q)),
      ...hits.where((i) => !i.title.toLowerCase().startsWith(q)),
    ];
    return groupSearchResults(ranked, perGroup: perGroup);
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
