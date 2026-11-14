// SPDX-License-Identifier: MPL-2.0
//
// Home rows of the demo library (UI-007): genres and a pinned smart shelf,
// so the demo, screenshots and goldens show every Home row. Genre and shelf
// names are library data (metadata and user input), not UI strings.

import '../integration/library.dart';
import '../screens/home_feed.dart';
import 'fake_library.dart';

/// Demo genres by title.
const demoGenres = <String, List<String>>{
  'The Quiet Orbit': ['Science Fiction', 'Drama'],
  'Harbor Lights': ['Comedy'],
  'Northbound': ['Mystery', 'Adventure'],
  'Paper Lanterns': ['Drama'],
  'The Last Lighthouse': ['Science Fiction', 'Comedy'],
  'Glass Horizon': ['Science Fiction', 'Mystery'],
  'Midnight Cartographer': ['Mystery', 'Adventure'],
  'Salt & Ember': ['Drama'],
  'Low Tide': ['Drama', 'Mystery'],
  'Copper Sky': ['Adventure', 'Science Fiction'],
  'Atlas of Small Things': ['Comedy', 'Drama'],
  'The Long Weekend': ['Comedy'],
  'Kites over Kestrel Bay': ['Adventure', 'Comedy'],
  'Static Bloom': ['Science Fiction'],
  'Moonlit Bureau': ['Comedy', 'Mystery'],
  'Coastline': ['Comedy', 'Drama'],
  'The Archive': ['Science Fiction', 'Mystery'],
  'Signal Fires': ['Adventure', 'Drama'],
  'Orchard Street': ['Comedy'],
};

/// The demo's smart shelf pinned to Home.
const demoShelfName = 'Under two hours';

class FakeHomeCatalog implements HomeCatalog {
  FakeHomeCatalog(this.library);

  final FakeLibrary library;

  Future<List<LibraryItem>> _works() async => [
    ...await library.movies().first,
    ...await library.shows().first,
  ];

  @override
  Future<List<String>> topGenres(int count) async {
    final counts = <String, int>{};
    for (final w in await _works()) {
      for (final g in demoGenres[w.title] ?? const <String>[]) {
        counts[g] = (counts[g] ?? 0) + 1;
      }
    }
    final sorted = counts.entries.toList()
      ..sort((a, b) {
        final c = b.value.compareTo(a.value);
        return c != 0 ? c : a.key.compareTo(b.key);
      });
    return [for (final e in sorted.take(count)) e.key];
  }

  @override
  Future<List<LibraryItem>> genreItems(String genre, int limit) async => [
    for (final w in await _works())
      if (demoGenres[w.title]?.contains(genre) ?? false) w,
  ].take(limit).toList();

  @override
  Future<List<(SmartShelf, List<LibraryItem>)>> pinnedShelves(
    int limit,
  ) async {
    final items = [
      for (final m in await library.movies().first)
        if ((m.durationMs ?? 0) > 0 && m.durationMs! < 120 * 60000) m,
    ];
    return [
      (
        const SmartShelf(
          id: '1',
          name: demoShelfName,
          query: LibraryQuery(ItemKind.movie),
          pinnedHome: true,
          position: 0,
        ),
        items.take(limit).toList(),
      ),
    ];
  }

  @override
  Future<List<LibraryItem>> recentlyAdded(ItemKind kind, int limit) =>
      library.recentlyAdded(kind: kind, limit: limit).first;

  @override
  Future<List<LibraryItem>> recentlyWatchedShows(int limit) async {
    final last = <(LibraryItem, DateTime)>[];
    for (final show in await library.shows().first) {
      DateTime? at;
      for (final e in await library.episodes(show.id).first) {
        final p = await library.progress(e.id);
        if (p != null && (at == null || p.updatedAt.isAfter(at))) {
          at = p.updatedAt;
        }
      }
      if (at != null) last.add((show, at));
    }
    last.sort((a, b) => b.$2.compareTo(a.$2));
    return [for (final (s, _) in last.take(limit)) s];
  }
}
