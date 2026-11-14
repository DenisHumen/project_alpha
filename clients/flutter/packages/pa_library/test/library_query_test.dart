// SPDX-License-Identifier: MPL-2.0
// Library queries on RustLibrary (LIB-009): filters and their combinations,
// sorts, paging, alphabet index, facets, folder browsing, view preferences
// and smart shelves, through the real native core.

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'native_library.dart';

/// A fake media file with content unique to [seed] (fingerprints differ).
void media(String root, String rel, int seed) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.generate(150000, (i) => (i * 31 + seed) & 0xff));
}

void main() {
  late Directory tmp;
  late RustLibrary lib;
  late MediaSource films;
  late MediaSource nas;

  setUpAll(() async {
    await RustLibrary.ensureInitialized(
      externalLibrary: openBuiltCoreLibrary(),
    );
  });

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('pa_lib_query_');
    final root = p.join(tmp.path, 'media');
    media(root, 'Movies/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv', 1);
    media(root, 'Movies/Inception (2010)/inception.mkv', 2);
    media(root, 'Movies/Alien.1979.mkv', 3);
    media(root, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E01.720p.mkv', 4);
    media(root, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.720p.mkv', 5);
    final other = p.join(tmp.path, 'nas');
    media(other, 'Kino/Amelie (2001).mkv', 6);
    media(other, 'Kino/Ёлки (2010).mkv', 7);

    lib = await RustLibrary.open(p.join(tmp.path, 'db', 'core.db'));
    films = await lib.addSource(
      MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: 'Films',
        path: root,
      ),
    );
    nas = await lib.addSource(
      MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: 'NAS',
        path: other,
      ),
    );
    final events = await lib.scan().toList();
    expect(events.expand((e) => e.errors), isEmpty);
  });

  tearDown(() async {
    await lib.close();
    tmp.deleteSync(recursive: true);
  });

  Future<List<String>> titles(LibraryQuery q) async {
    final page = await lib.query(q, limit: 1000);
    expect(page.total, page.items.length);
    return [for (final i in page.items) i.title];
  }

  const movies = LibraryQuery(ItemKind.movie);

  Future<LibraryItem> movie(String title) async =>
      (await lib.movies().first).firstWhere((m) => m.title == title);

  test('sorts by title across scripts and by other fields', () async {
    expect(await titles(movies), [
      'Alien',
      'Amelie',
      'Inception',
      'The Matrix',
      'Ёлки',
    ]);
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          sort: LibrarySort(field: SortField.year),
        ),
      ),
      ['Alien', 'The Matrix', 'Amelie', 'Inception', 'Ёлки'],
    );
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          sort: LibrarySort(field: SortField.title, descending: true),
        ),
      ),
      ['Ёлки', 'The Matrix', 'Inception', 'Amelie', 'Alien'],
    );

    final page = await lib.query(movies, offset: 1, limit: 2);
    expect(page.total, 5);
    expect(page.offset, 1);
    expect([for (final i in page.items) i.title], ['Amelie', 'Inception']);
    expect((await lib.query(movies, offset: 10)).items, isEmpty);

    expect(await titles(const LibraryQuery(ItemKind.show)), ['Breaking Bad']);
    expect(
      () => lib.query(const LibraryQuery(ItemKind.episode)),
      throwsArgumentError,
    );
  });

  test('filters combine: AND across fields, OR inside one', () async {
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          filter: LibraryFilter(yearMin: 2000, yearMax: 2010),
        ),
      ),
      ['Amelie', 'Inception', 'Ёлки'],
    );
    expect(
      await titles(
        LibraryQuery(ItemKind.movie, filter: LibraryFilter(sources: [nas.id])),
      ),
      ['Amelie', 'Ёлки'],
    );

    final matrix = await movie('The Matrix');
    await lib.saveProgress(
      Progress(
        itemId: matrix.id,
        positionMs: 7900000,
        durationMs: 8000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
        watched: true,
      ),
    );
    final inception = await movie('Inception');
    await lib.saveProgress(
      Progress(
        itemId: inception.id,
        positionMs: 600000,
        durationMs: 8000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(2000),
      ),
    );
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          filter: LibraryFilter(watched: {WatchState.watched}),
        ),
      ),
      ['The Matrix'],
    );
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          filter: LibraryFilter(
            watched: {WatchState.watched, WatchState.inProgress},
          ),
          sort: LibrarySort(field: SortField.lastWatched, descending: true),
        ),
      ),
      ['Inception', 'The Matrix'],
    );
    expect(
      await titles(
        LibraryQuery(
          ItemKind.movie,
          filter: LibraryFilter(
            yearMin: 2000,
            sources: [films.id, nas.id],
            watched: {WatchState.unwatched},
          ),
        ),
      ),
      ['Amelie', 'Ёлки'],
    );
    // Unprobed files match no video filter.
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          filter: LibraryFilter(resolutions: {VideoResolution.uhd}),
        ),
      ),
      isEmpty,
    );
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.movie,
          filter: LibraryFilter(genres: ['Drama']),
        ),
      ),
      isEmpty,
    );

    // A show is in progress while some of its episodes are watched.
    final show = (await lib.shows().first).single;
    final eps = await lib.episodes(show.id).first;
    await lib.saveProgress(
      Progress(
        itemId: eps.first.id,
        positionMs: 2990000,
        durationMs: 3000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(3000),
        watched: true,
      ),
    );
    expect(
      await titles(
        const LibraryQuery(
          ItemKind.show,
          filter: LibraryFilter(watched: {WatchState.inProgress}),
        ),
      ),
      ['Breaking Bad'],
    );
  });

  test('watchQuery emits again after a change', () async {
    const unwatched = LibraryQuery(
      ItemKind.movie,
      filter: LibraryFilter(watched: {WatchState.unwatched}),
    );
    final pages = lib.watchQuery(unwatched).take(2).toList();
    await Future<void>.delayed(const Duration(milliseconds: 50));
    await lib.saveProgress(
      Progress(
        itemId: (await movie('Alien')).id,
        positionMs: 100,
        durationMs: 8000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
        watched: true,
      ),
    );
    final got = await pages;
    expect(got.first.total, 5);
    expect(got.last.total, 4);
  });

  test('alphabet index and facets', () async {
    expect(await lib.alphabetIndex(movies), const [
      AlphabetEntry(letter: 'A', offset: 0, count: 2),
      AlphabetEntry(letter: 'I', offset: 2, count: 1),
      AlphabetEntry(letter: 'M', offset: 3, count: 1),
      AlphabetEntry(letter: 'Е', offset: 4, count: 1),
    ]);
    final facets = await lib.facets(ItemKind.movie);
    expect((facets.yearMin, facets.yearMax), (1979, 2010));
    expect(facets.sources, [FacetCount(films.id, 3), FacetCount(nas.id, 2)]);
    expect(facets.resolutions, isEmpty);
    expect(facets.genres, isEmpty);
    expect(await lib.genres((await movie('Alien')).id), isEmpty);
  });

  test('browse lists folders first or mixed', () async {
    Future<List<String>> names(
      String dir, {
      required bool foldersFirst,
    }) async => [
      for (final e in await lib.browse(
        films.id,
        dir: dir,
        foldersFirst: foldersFirst,
      ))
        e.name,
    ];
    expect(await names('', foldersFirst: true), ['Movies', 'TV']);
    expect(await names('Movies', foldersFirst: true), [
      'Inception (2010)',
      'Alien.1979.mkv',
      'The.Matrix.1999.1080p.BluRay.x264-GRP.mkv',
    ]);
    expect(await names('Movies', foldersFirst: false), [
      'Alien.1979.mkv',
      'Inception (2010)',
      'The.Matrix.1999.1080p.BluRay.x264-GRP.mkv',
    ]);
    final entries = await lib.browse(films.id, dir: 'Movies');
    expect(entries.first.isFolder, isTrue);
    expect(entries.first.relPath, 'Movies/Inception (2010)');
    expect(entries.first.fileCount, 1);
    expect(entries[1].itemId, (await movie('Alien')).id);
    expect(await lib.browse('nope'), isEmpty);
  });

  test('view preferences are kept per view', () async {
    expect(await lib.viewPrefs('movies'), const ViewPrefs('movies'));
    const moviesPrefs = ViewPrefs(
      'movies',
      sort: LibrarySort(field: SortField.added, descending: true),
      filter: LibraryFilter(
        resolutions: {VideoResolution.uhd},
        hdr: {HdrFilter.dolbyVision},
      ),
    );
    final folderPrefs = ViewPrefs('folder:${films.id}', foldersFirst: false);
    await lib.setViewPrefs(moviesPrefs);
    await lib.setViewPrefs(folderPrefs);
    expect(await lib.viewPrefs('movies'), moviesPrefs);
    expect(await lib.viewPrefs('folder:${films.id}'), folderPrefs);
    expect(await lib.viewPrefs('shows'), const ViewPrefs('shows'));
  });

  test('smart shelves save queries and pin to Home', () async {
    const recent = LibraryQuery(
      ItemKind.movie,
      filter: LibraryFilter(yearMin: 2000),
      sort: LibrarySort(field: SortField.year, descending: true),
    );
    final a = await lib.createShelf('2000s', recent, pinnedHome: true);
    final b = await lib.createShelf(
      'NAS',
      LibraryQuery(ItemKind.movie, filter: LibraryFilter(sources: [nas.id])),
    );
    expect(a.query, recent);
    expect((a.position, b.position), (0, 1));
    expect(
      [for (final s in await lib.shelves(pinnedOnly: true)) s.name],
      ['2000s'],
    );
    final page = await lib.shelfItems(a.id);
    expect(
      [for (final i in page.items) i.title],
      ['Inception', 'Ёлки', 'Amelie'],
    );

    await lib.reorderShelves([b.id, a.id]);
    expect([for (final s in await lib.shelves()) s.id], [b.id, a.id]);
    await lib.updateShelf(b.id, 'NAS films', b.query, pinnedHome: true);
    expect(
      [for (final s in await lib.shelves(pinnedOnly: true)) s.name],
      ['NAS films', '2000s'],
    );
    await lib.deleteShelf(a.id);
    expect(await lib.shelves(), hasLength(1));
    expect(
      () => lib.createShelf('Episodes', const LibraryQuery(ItemKind.episode)),
      throwsArgumentError,
    );
  });
}
