// SPDX-License-Identifier: MPL-2.0
//
// Data of the Home screen (UI-007): the shared Continue row, Up Next and the
// library rows (recently added, smart shelves pinned to Home, genres). Every
// stream follows library changes (scans, the LIB-005 watcher, saved
// progress), so rows update in place without reloading the screen.
//
// The LIB-009 parts (genres from facets, smart shelves, "recently watched"
// order) need the Rust core ([RustLibrary]). On the v0 fallback library
// Home shows Continue, recently added and sources only.

import 'dart:async';

import '../fakes/fake_home_catalog.dart';
import '../fakes/fake_library.dart';
import '../integration/library.dart';

/// What a card of the Continue row is; shown as a type badge.
enum ContinueKind { movie, episode, twitchVod }

/// A card of the Continue row: a library item in progress or an entry from
/// another source (a Twitch VOD), with its position.
class ContinueEntry {
  const ContinueEntry({
    required this.kind,
    required this.id,
    required this.title,
    required this.progress,
    this.item,
    this.show,
    this.imageUrl,
    this.subtitle,
  });

  final ContinueKind kind;
  final String id;

  /// The show's title for an episode.
  final String title;
  final Progress progress;

  /// The library item; null for entries from outside the library.
  final LibraryItem? item;

  /// The show of an episode, if found.
  final LibraryItem? show;
  final String? imageUrl;

  /// Caption of a non-library entry (e.g. channel · category).
  final String? subtitle;

  DateTime get updatedAt => progress.updatedAt;
}

/// Entries for the Continue row from outside the library, merged by
/// [ContinueEntry.updatedAt]. TODO(TWI): Twitch VODs with a saved position
/// plug in here once the Twitch section is wired into the app.
typedef ContinueSource = Stream<List<ContinueEntry>>;

/// Up Next: the episode after the last finished one of a show.
class UpNextEntry {
  const UpNextEntry({
    required this.episode,
    required this.show,
    required this.after,
  });

  final LibraryItem episode;
  final LibraryItem show;

  /// When the previous episode was finished.
  final DateTime after;
}

enum HomeShelfKind { recentMovies, recentShows, smart, genre }

/// A poster row of Home.
class HomeShelf {
  const HomeShelf({required this.kind, required this.items, this.name});

  final HomeShelfKind kind;

  /// Shelf or genre name (user and metadata data, not translated).
  final String? name;
  final List<LibraryItem> items;

  /// Stable key of the row.
  String get key => '${kind.name}:${name ?? ''}';
}

/// LIB-009 queries Home needs, per library implementation.
abstract interface class HomeCatalog {
  /// Most common genres of movies and shows.
  Future<List<String>> topGenres(int count);

  /// Movies and shows of [genre], recently added first.
  Future<List<LibraryItem>> genreItems(String genre, int limit);

  /// Smart shelves pinned to Home with their first items.
  Future<List<(SmartShelf, List<LibraryItem>)>> pinnedShelves(int limit);

  /// Recently added movies or shows.
  Future<List<LibraryItem>> recentlyAdded(ItemKind kind, int limit);

  /// Shows with watched episodes, most recently watched first.
  Future<List<LibraryItem>> recentlyWatchedShows(int limit);
}

/// The catalog for [library]: the Rust core, the demo data or the reduced
/// v0 fallback.
HomeCatalog homeCatalogFor(Library library) => switch (library) {
  final RustLibrary l => RustHomeCatalog(l),
  final FakeLibrary l => FakeHomeCatalog(l),
  _ => BasicHomeCatalog(library),
};

/// Home data of one library.
class HomeFeed {
  HomeFeed(
    this.library, {
    HomeCatalog? catalog,
    this.continueSources = const [],
  }) : catalog = catalog ?? homeCatalogFor(library);

  final Library library;
  final HomeCatalog catalog;

  /// Extra Continue entries (Twitch VODs…).
  final List<ContinueSource> continueSources;

  static const rowLimit = 20;
  static const genreRows = 6;
  static const upNextShows = 12;

  /// Emits once now and after every library change.
  Stream<void> _changes() => library.continueWatching().map((_) {});

  /// The shared Continue row, most recently played first.
  Stream<List<ContinueEntry>> continueRow() => combineLatestLists([
    library.continueWatching().asyncMap(_continueEntries),
    ...continueSources,
  ]).map((all) => all..sort((a, b) => b.updatedAt.compareTo(a.updatedAt)));

  Future<List<ContinueEntry>> _continueEntries(
    List<(LibraryItem, Progress)> rows,
  ) async {
    final shows = <String, LibraryItem?>{};
    final out = <ContinueEntry>[];
    for (final (item, p) in rows) {
      final showId = item.showId;
      final episode = item.kind == ItemKind.episode;
      final show = episode && showId != null
          ? shows[showId] ??= await _safe(() => library.item(showId), null)
          : null;
      out.add(
        ContinueEntry(
          kind: episode ? ContinueKind.episode : ContinueKind.movie,
          id: item.id,
          title: show?.title ?? item.title,
          progress: p,
          item: item,
          show: show,
          imageUrl: item.backdropUrl ?? show?.backdropUrl,
        ),
      );
    }
    return out;
  }

  /// Up Next, most recently finished first.
  Stream<List<UpNextEntry>> upNext() => latestOnChange(_changes(), _upNext);

  Future<List<UpNextEntry>> _upNext() async {
    final out = <UpNextEntry>[];
    final shows = await _safe(
      () => catalog.recentlyWatchedShows(upNextShows),
      const <LibraryItem>[],
    );
    for (final show in shows) {
      final e = await _safe(() => _upNextOf(show), null);
      if (e != null) out.add(e);
    }
    out.sort((a, b) => b.after.compareTo(a.after));
    return out;
  }

  Future<UpNextEntry?> _upNextOf(LibraryItem show) async {
    final episodes = await library.episodes(show.id).first;
    LibraryItem? last;
    Progress? lastP;
    for (final e in episodes) {
      final p = await library.progress(e.id);
      if (p != null &&
          (lastP == null || p.updatedAt.isAfter(lastP.updatedAt))) {
        last = e;
        lastP = p;
      }
    }
    // A started episode is in the Continue row.
    if (last == null || lastP == null || !_finished(lastP)) return null;
    // PLAY-005: the same rule the player uses for its Up Next card.
    final next = await library.nextEpisode(last.id);
    if (next == null) return null;
    final np = await library.progress(next.id);
    if (np != null && (np.watched || np.positionMs > 0)) return null;
    return UpNextEntry(episode: next, show: show, after: lastP.updatedAt);
  }

  /// Favorites pinned to Home (LIB-010), newest first; empty on a library
  /// without favorites (the v0 fallback).
  Stream<List<LibraryItem>> favorites() => switch (library) {
    final LibraryLists l => l
        .favorites(pinnedOnly: true)
        .map((items) => items.take(rowLimit).toList()),
    _ => Stream.value(const <LibraryItem>[]),
  };

  static bool _finished(Progress p) =>
      p.watched ||
      (p.durationMs > 0 && p.positionMs / p.durationMs > continueWatchingMax);

  /// Poster rows: recently added, pinned smart shelves, genres.
  Stream<List<HomeShelf>> shelves() => latestOnChange(_changes(), _shelves);

  Future<List<HomeShelf>> _shelves() async {
    const none = <LibraryItem>[];
    final out = <HomeShelf>[];
    for (final (kind, shelf) in const [
      (ItemKind.movie, HomeShelfKind.recentMovies),
      (ItemKind.show, HomeShelfKind.recentShows),
    ]) {
      final items = await _safe(
        () => catalog.recentlyAdded(kind, rowLimit),
        none,
      );
      if (items.isNotEmpty) out.add(HomeShelf(kind: shelf, items: items));
    }
    final pinned = await _safe(
      () => catalog.pinnedShelves(rowLimit),
      const <(SmartShelf, List<LibraryItem>)>[],
    );
    for (final (s, items) in pinned) {
      if (items.isNotEmpty) {
        out.add(
          HomeShelf(kind: HomeShelfKind.smart, name: s.name, items: items),
        );
      }
    }
    final genres = await _safe(
      () => catalog.topGenres(genreRows),
      const <String>[],
    );
    for (final g in genres) {
      final items = await _safe(() => catalog.genreItems(g, rowLimit), none);
      if (items.isNotEmpty) {
        out.add(HomeShelf(kind: HomeShelfKind.genre, name: g, items: items));
      }
    }
    return out;
  }
}

/// [run] now (once [ticks] emits) and after every tick. Ticks during a run
/// coalesce into one more run, so a burst of changes costs at most two.
Stream<T> latestOnChange<T>(Stream<void> ticks, Future<T> Function() run) {
  late StreamController<T> c;
  StreamSubscription<void>? sub;
  var busy = false;
  var dirty = false;
  Future<void> go() async {
    if (busy) {
      dirty = true;
      return;
    }
    busy = true;
    do {
      dirty = false;
      try {
        final v = await run();
        if (!c.isClosed) c.add(v);
      } catch (e, st) {
        if (!c.isClosed) c.addError(e, st);
      }
    } while (dirty && !c.isClosed);
    busy = false;
  }

  c = StreamController<T>(
    onListen: () => sub = ticks.listen((_) => go(), onError: (_) {}),
    onPause: () => sub?.pause(),
    onResume: () => sub?.resume(),
    onCancel: () => sub?.cancel(),
  );
  return c.stream;
}

/// The latest list of each stream, concatenated; a stream that has not
/// emitted yet counts as empty.
Stream<List<T>> combineLatestLists<T>(List<Stream<List<T>>> streams) {
  if (streams.length == 1) return streams.single.map((l) => [...l]);
  late StreamController<List<T>> c;
  final latest = List<List<T>>.filled(streams.length, const []);
  final subs = <StreamSubscription<List<T>>>[];
  c = StreamController<List<T>>(
    onListen: () {
      for (var i = 0; i < streams.length; i++) {
        subs.add(
          streams[i].listen((v) {
            latest[i] = v;
            c.add([for (final l in latest) ...l]);
          }, onError: c.addError),
        );
      }
    },
    onPause: () {
      for (final s in subs) {
        s.pause();
      }
    },
    onResume: () {
      for (final s in subs) {
        s.resume();
      }
    },
    onCancel: () => Future.wait([for (final s in subs) s.cancel()]),
  );
  return c.stream;
}

Future<T> _safe<T>(Future<T> Function() f, T fallback) async {
  try {
    return await f();
  } catch (_) {
    return fallback;
  }
}

/// Home rows on the Rust core (LIB-009).
class RustHomeCatalog implements HomeCatalog {
  RustHomeCatalog(this.library);

  final RustLibrary library;

  static const _added = LibrarySort(field: SortField.added, descending: true);

  @override
  Future<List<String>> topGenres(int count) async {
    final counts = <String, (String, int)>{};
    for (final kind in const [ItemKind.movie, ItemKind.show]) {
      for (final g in (await library.facets(kind)).genres) {
        final key = g.value.toLowerCase();
        final (name, n) = counts[key] ?? (g.value, 0);
        counts[key] = (name, n + g.count);
      }
    }
    final sorted = counts.values.toList()
      ..sort((a, b) {
        final c = b.$2.compareTo(a.$2);
        return c != 0 ? c : a.$1.compareTo(b.$1);
      });
    return [for (final (name, _) in sorted.take(count)) name];
  }

  @override
  Future<List<LibraryItem>> genreItems(String genre, int limit) async {
    final items = <LibraryItem>[];
    for (final kind in const [ItemKind.movie, ItemKind.show]) {
      final page = await library.query(
        LibraryQuery(
          kind,
          filter: LibraryFilter(genres: [genre]),
          sort: _added,
        ),
        limit: limit,
      );
      items.addAll(page.items);
    }
    return newestFirst(items).take(limit).toList();
  }

  @override
  Future<List<(SmartShelf, List<LibraryItem>)>> pinnedShelves(
    int limit,
  ) async => [
    for (final s in await library.shelves(pinnedOnly: true))
      (s, (await library.shelfItems(s.id, limit: limit)).items),
  ];

  @override
  Future<List<LibraryItem>> recentlyAdded(ItemKind kind, int limit) async =>
      (await library.query(
        LibraryQuery(kind, sort: _added),
        limit: limit,
      )).items;

  @override
  Future<List<LibraryItem>> recentlyWatchedShows(int limit) async =>
      (await library.query(
        const LibraryQuery(
          ItemKind.show,
          filter: LibraryFilter(watched: {WatchState.inProgress}),
          sort: LibrarySort(field: SortField.lastWatched, descending: true),
        ),
        limit: limit,
      )).items;
}

/// Home rows on a library without LIB-009 queries (the v0 fallback):
/// recently added only.
class BasicHomeCatalog implements HomeCatalog {
  BasicHomeCatalog(this.library);

  final Library library;

  @override
  Future<List<String>> topGenres(int count) async => const [];

  @override
  Future<List<LibraryItem>> genreItems(String genre, int limit) async =>
      const [];

  @override
  Future<List<(SmartShelf, List<LibraryItem>)>> pinnedShelves(
    int limit,
  ) async => const [];

  @override
  Future<List<LibraryItem>> recentlyAdded(ItemKind kind, int limit) =>
      library.recentlyAdded(kind: kind, limit: limit).first;

  @override
  Future<List<LibraryItem>> recentlyWatchedShows(int limit) async => const [];
}

/// [items] by [LibraryItem.addedAt], newest first (stable; unknown last).
List<LibraryItem> newestFirst(Iterable<LibraryItem> items) {
  final list = items.toList();
  final order = {for (var i = 0; i < list.length; i++) list[i].id: i};
  list.sort((a, b) {
    final x = a.addedAt, y = b.addedAt;
    if (x != null && y != null && x != y) return y.compareTo(x);
    if (x == null && y != null) return 1;
    if (x != null && y == null) return -1;
    return order[a.id]!.compareTo(order[b.id]!);
  });
  return list;
}
