// SPDX-License-Identifier: MPL-2.0
//
// The grid's queries on the real Rust core (UI-008 over LIB-009): pages,
// the alphabet index, per-view sort, badges after "Mark as watched".

import 'dart:io';

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/library/library_queries.dart';
import 'package:kinolune/screens/library_item_menu.dart';

import 'support/native_library.dart';

/// A fake media file with content unique to [seed] (fingerprints differ).
void _media(Directory root, String rel, int seed) {
  final f = File('${root.path}/$rel');
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.generate(150000, (i) => (i * 31 + seed) & 0xff));
}

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();

  late Directory tmp;
  late RustLibrary lib;
  late LibraryQueries q;

  setUpAll(() async {
    await RustLibrary.ensureInitialized(
      externalLibrary: openBuiltCoreLibrary(),
    );
  });

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('kinolune_grid_');
    final media = Directory('${tmp.path}/media');
    _media(media, 'Movies/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv', 1);
    _media(media, 'Movies/Inception (2010)/inception.mkv', 2);
    _media(media, 'Movies/Alien.1979.mkv', 3);
    lib = await RustLibrary.open('${tmp.path}/db/core.db');
    await lib.addSource(
      MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: 'Films',
        path: media.path,
      ),
    );
    await lib.scan().toList();
    q = LibraryQueries.forLibrary(lib);
  });

  tearDown(() async {
    await lib.close();
    tmp.deleteSync(recursive: true);
  });

  test('uses the core queries', () async {
    expect(q, isA<RustLibraryQueries>());
    expect(q.supportsFilters, isTrue);
    final page = await q.query(const LibraryQuery(ItemKind.movie), limit: 2);
    expect(page.total, 3);
    expect([for (final i in page.items) i.title], ['Alien', 'Inception']);
    final index = await q.alphabetIndex(const LibraryQuery(ItemKind.movie));
    // "The Matrix" is indexed under M (leading articles are skipped).
    expect([for (final e in index) e.letter], ['A', 'I', 'M']);
    expect((await q.facets(ItemKind.movie))!.yearMax, 2010);

    const sort = LibrarySort(field: SortField.year, descending: true);
    await q.setViewPrefs(const ViewPrefs('movies', sort: sort));
    expect((await q.viewPrefs('movies')).sort, sort);
    expect((await q.viewPrefs('shows')).sort, const LibrarySort());
  });

  test('badges follow "Mark as watched"', () async {
    final badges = q.badges(ItemKind.movie);
    expect(await badges.first, isEmpty);
    final alien = (await q.query(const LibraryQuery(ItemKind.movie)))
        .items
        .first;
    await markWatched(lib, alien);
    final after = await q.badges(ItemKind.movie).first;
    expect(after[alien.id]?.state, WatchState.watched);
    final watched = await q.query(
      const LibraryQuery(
        ItemKind.movie,
        filter: LibraryFilter(watched: {WatchState.watched}),
      ),
    );
    expect([for (final i in watched.items) i.id], [alien.id]);
  });
}
