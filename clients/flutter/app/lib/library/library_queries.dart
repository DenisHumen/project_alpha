// SPDX-License-Identifier: MPL-2.0
//
// What the library grid (UI-008) asks the library: pages of a filtered and
// sorted query, the alphabet index, facets for filter chips, per-view
// preferences and watch badges. On the Rust core these are LIB-009 queries
// ([RustLibraryQueries]); the v0 fallback and the demo library answer them
// in memory ([MemoryLibraryQueries]), the v0 one without filters.

import 'dart:async';

import 'package:pa_library/pa_library.dart'
    show
        AlphabetEntry,
        FacetCount,
        LibraryFacets,
        LibraryFilter,
        LibraryPage,
        LibraryQuery,
        LibrarySort,
        SortField,
        ViewPrefs,
        WatchState;

import '../integration/library.dart';

export 'package:pa_library/pa_library.dart'
    show
        AlphabetEntry,
        FacetCount,
        HdrFilter,
        LibraryFacets,
        LibraryFilter,
        LibraryPage,
        LibraryQuery,
        LibrarySort,
        SortField,
        VideoResolution,
        ViewPrefs,
        WatchState;

/// Watch badge of a movie or show.
class ItemBadge {
  const ItemBadge(this.state, [this.progress]);

  final WatchState state;

  /// 0..1 for a movie in progress, when the position is known.
  final double? progress;

  @override
  bool operator ==(Object other) =>
      other is ItemBadge && other.state == state && other.progress == progress;

  @override
  int get hashCode => Object.hash(state, progress);
}

/// Library queries for the grid.
abstract class LibraryQueries {
  /// Queries for [library]: LIB-009 on the Rust core, the library's own
  /// ([LibraryQueriesProvider], the demo library) or in memory without
  /// filters (the v0 fallback).
  factory LibraryQueries.forLibrary(Library library) => switch (library) {
    RustLibrary() => RustLibraryQueries(library),
    LibraryQueriesProvider(:final queries) => queries,
    _ => MemoryLibraryQueries(library),
  };

  /// Whether filters (and facets) are available; the v0 fallback has none.
  bool get supportsFilters;

  /// One page of [query].
  Future<LibraryPage> query(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  });

  /// [query] now and after every library change.
  Stream<LibraryPage> watchQuery(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  });

  /// Alphabet index of [query] in title order.
  Future<List<AlphabetEntry>> alphabetIndex(LibraryQuery query);

  /// Filter values for chips; null without filters.
  Future<LibraryFacets?> facets(ItemKind kind);

  Future<ViewPrefs> viewPrefs(String view);
  Future<void> setViewPrefs(ViewPrefs prefs);

  /// Watch badges of movies or shows by item id, now and after changes.
  /// Items without an entry are unwatched.
  Stream<Map<String, ItemBadge>> badges(ItemKind kind);
}

/// A library that answers [LibraryQueries] itself (the demo library).
abstract interface class LibraryQueriesProvider {
  LibraryQueries get queries;
}

/// Latest values of [a] and [b], once both have one.
Stream<R> combineLatest2<A, B, R>(
  Stream<A> a,
  Stream<B> b,
  R Function(A, B) combine,
) {
  late StreamController<R> c;
  StreamSubscription<A>? sa;
  StreamSubscription<B>? sb;
  A? va;
  B? vb;
  var hasA = false, hasB = false;
  void emit() {
    if (hasA && hasB) c.add(combine(va as A, vb as B));
  }

  c = StreamController<R>(
    onListen: () {
      sa = a.listen((v) {
        va = v;
        hasA = true;
        emit();
      }, onError: c.addError);
      sb = b.listen((v) {
        vb = v;
        hasB = true;
        emit();
      }, onError: c.addError);
    },
    onCancel: () async {
      await sa?.cancel();
      await sb?.cancel();
    },
  );
  return c.stream;
}

/// Badges from "Continue watching": in-progress movies with their position,
/// shows with an episode in progress (the v0 library knows no more).
Stream<Map<String, ItemBadge>> continueWatchingBadges(Library library) =>
    library.continueWatching().map(
      (rows) => {
        for (final (item, p) in rows)
          if (item.showId case final show?)
            show: const ItemBadge(WatchState.inProgress)
          else
            item.id: ItemBadge(WatchState.inProgress, p.fraction),
      },
    );

/// LIB-009 queries of the Rust core.
class RustLibraryQueries implements LibraryQueries {
  RustLibraryQueries(this.library);

  final RustLibrary library;

  /// Upper bound of the watched / in-progress id lists behind the badges.
  static const badgeLimit = 100000;

  @override
  bool get supportsFilters => true;

  @override
  Future<LibraryPage> query(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) => library.query(query, offset: offset, limit: limit);

  @override
  Stream<LibraryPage> watchQuery(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) => library.watchQuery(query, offset: offset, limit: limit);

  @override
  Future<List<AlphabetEntry>> alphabetIndex(LibraryQuery query) =>
      library.alphabetIndex(query);

  @override
  Future<LibraryFacets?> facets(ItemKind kind) => library.facets(kind);

  @override
  Future<ViewPrefs> viewPrefs(String view) => library.viewPrefs(view);

  @override
  Future<void> setViewPrefs(ViewPrefs prefs) => library.setViewPrefs(prefs);

  @override
  Stream<Map<String, ItemBadge>> badges(ItemKind kind) {
    Stream<Set<String>> ids(WatchState s) => library
        .watchQuery(
          LibraryQuery(kind, filter: LibraryFilter(watched: {s})),
          limit: badgeLimit,
        )
        .map((p) => {for (final i in p.items) i.id});
    final states = combineLatest2(
      ids(WatchState.watched),
      ids(WatchState.inProgress),
      (w, p) => (w, p),
    );
    return combineLatest2(states, continueWatchingBadges(library), (s, cw) {
      final (watched, inProgress) = s;
      return {
        for (final id in inProgress)
          id: cw[id] ?? const ItemBadge(WatchState.inProgress),
        for (final id in watched) id: const ItemBadge(WatchState.watched),
      };
    });
  }
}

/// Queries answered in memory from [Library.movies] / [Library.shows]: the
/// v0 fallback (no filters) and the demo library.
class MemoryLibraryQueries implements LibraryQueries {
  MemoryLibraryQueries(
    this.library, {
    this.supportsFilters = false,
    this.genresOf,
    Stream<Map<String, ItemBadge>> Function(ItemKind kind)? badges,
  }) : _badges = badges;

  final Library library;

  @override
  final bool supportsFilters;

  /// Genres of an item (filters and facets).
  final List<String> Function(LibraryItem item)? genresOf;

  final Stream<Map<String, ItemBadge>> Function(ItemKind kind)? _badges;

  final _prefs = <String, ViewPrefs>{};

  // The last sorted result, reused by the pages of one query.
  (LibraryQuery, List<LibraryItem>, Map<String, ItemBadge>, List<LibraryItem>)?
  _cache;

  Stream<List<LibraryItem>> _items(ItemKind kind) =>
      kind == ItemKind.show ? library.shows() : library.movies();

  @override
  Stream<Map<String, ItemBadge>> badges(ItemKind kind) =>
      _badges?.call(kind) ?? continueWatchingBadges(library);

  Stream<(List<LibraryItem>, Map<String, ItemBadge>)> _state(ItemKind kind) =>
      combineLatest2(_items(kind), badges(kind), (i, b) => (i, b));

  List<LibraryItem> _apply(
    LibraryQuery q,
    List<LibraryItem> all,
    Map<String, ItemBadge> badges,
  ) {
    final c = _cache;
    if (c != null &&
        c.$1 == q &&
        identical(c.$2, all) &&
        identical(c.$3, badges)) {
      return c.$4;
    }
    final out = applyQuery(q, all, badges, genresOf: genresOf);
    _cache = (q, all, badges, out);
    return out;
  }

  LibraryPage _page(List<LibraryItem> items, int offset, int limit) {
    final start = offset.clamp(0, items.length);
    final end = (start + limit).clamp(start, items.length);
    return LibraryPage(
      items: items.sublist(start, end),
      total: items.length,
      offset: start,
    );
  }

  @override
  Future<LibraryPage> query(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) async {
    final (all, b) = await _state(query.kind).first;
    return _page(_apply(query, all, b), offset, limit);
  }

  @override
  Stream<LibraryPage> watchQuery(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) => _state(
    query.kind,
  ).map((s) => _page(_apply(query, s.$1, s.$2), offset, limit));

  @override
  Future<List<AlphabetEntry>> alphabetIndex(LibraryQuery query) async {
    final (all, b) = await _state(query.kind).first;
    final byTitle = LibraryQuery(
      query.kind,
      filter: query.filter,
      sort: LibrarySort(
        descending:
            query.sort.field == SortField.title && query.sort.descending,
      ),
    );
    return alphabetOf(_apply(byTitle, all, b));
  }

  @override
  Future<LibraryFacets?> facets(ItemKind kind) async {
    if (!supportsFilters) return null;
    final all = await _items(kind).first;
    final genres = <String, int>{};
    int? yMin, yMax;
    for (final i in all) {
      for (final g in genresOf?.call(i) ?? const <String>[]) {
        genres[g] = (genres[g] ?? 0) + 1;
      }
      if (i.year case final y?) {
        if (yMin == null || y < yMin) yMin = y;
        if (yMax == null || y > yMax) yMax = y;
      }
    }
    final g = genres.entries.toList()
      ..sort((a, b) => a.key.toLowerCase().compareTo(b.key.toLowerCase()));
    return LibraryFacets(
      genres: [for (final e in g) FacetCount(e.key, e.value)],
      yearMin: yMin,
      yearMax: yMax,
      resolutions: const [],
      hdr: const [],
      audioFormats: const [],
      audioLanguages: const [],
      sources: const [],
    );
  }

  @override
  Future<ViewPrefs> viewPrefs(String view) async =>
      _prefs[view] ?? ViewPrefs(view);

  @override
  Future<void> setViewPrefs(ViewPrefs prefs) async => _prefs[prefs.view] = prefs;
}

/// [query] over [all] in memory: genre, year and watch filters (file
/// filters match nothing here), then the sort. Missing values sort last in
/// both directions; ties go by title A→Z, as on the core.
List<LibraryItem> applyQuery(
  LibraryQuery query,
  List<LibraryItem> all,
  Map<String, ItemBadge> badges, {
  List<String> Function(LibraryItem item)? genresOf,
}) {
  final f = query.filter;
  final genres = {for (final g in f.genres) g.toLowerCase()};
  final fileFilter =
      f.resolutions.isNotEmpty ||
      f.hdr.isNotEmpty ||
      f.audioFormats.isNotEmpty ||
      f.audioLanguages.isNotEmpty ||
      f.sources.isNotEmpty;
  final items = [
    for (final i in all)
      if (!fileFilter &&
          (genres.isEmpty ||
              (genresOf?.call(i) ?? const <String>[]).any(
                (g) => genres.contains(g.toLowerCase()),
              )) &&
          (f.yearMin == null || (i.year != null && i.year! >= f.yearMin!)) &&
          (f.yearMax == null || (i.year != null && i.year! <= f.yearMax!)) &&
          (f.watched.isEmpty ||
              f.watched.contains(badges[i.id]?.state ?? WatchState.unwatched)))
        i,
  ];
  final s = query.sort;
  int byTitle(LibraryItem a, LibraryItem b) =>
      a.title.toLowerCase().compareTo(b.title.toLowerCase());
  int? key(LibraryItem i) => switch (s.field) {
    SortField.title => null,
    SortField.year => i.year,
    SortField.added => i.addedAt?.millisecondsSinceEpoch,
    SortField.duration => i.durationMs,
    // Play times are not kept in memory: by title.
    SortField.lastWatched => null,
  };
  items.sort((a, b) {
    if (s.field == SortField.title) {
      return s.descending ? byTitle(b, a) : byTitle(a, b);
    }
    final ka = key(a), kb = key(b);
    if (ka == null || kb == null) {
      if (ka != null) return -1;
      if (kb != null) return 1;
      return byTitle(a, b);
    }
    final c = s.descending ? kb.compareTo(ka) : ka.compareTo(kb);
    return c != 0 ? c : byTitle(a, b);
  });
  return items;
}

/// Index letter of a title: its first letter upper-cased, `#` for digits
/// and symbols.
String indexLetter(String title) {
  final t = title.trimLeft();
  if (t.isEmpty) return '#';
  final c = String.fromCharCode(t.runes.first).toUpperCase();
  return c == c.toLowerCase() ? '#' : c;
}

/// Alphabet index of a title-sorted list.
List<AlphabetEntry> alphabetOf(List<LibraryItem> byTitle) {
  final out = <AlphabetEntry>[];
  String? letter;
  var start = 0;
  for (var i = 0; i <= byTitle.length; i++) {
    final l = i < byTitle.length ? indexLetter(byTitle[i].title) : null;
    if (l == letter) continue;
    if (letter != null) {
      out.add(AlphabetEntry(letter: letter, offset: start, count: i - start));
    }
    letter = l;
    start = i;
  }
  return out;
}
