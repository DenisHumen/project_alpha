// SPDX-License-Identifier: MPL-2.0
// RustLibrary (UI-024): the Library interface on the Rust core. These tests
// call the real native core built by pa_core's Native Assets hook.

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
  late String root;

  setUpAll(() async {
    await RustLibrary.ensureInitialized(
      externalLibrary: openBuiltCoreLibrary(),
    );
  });

  setUp(() {
    tmp = Directory.systemTemp.createTempSync('pa_rust_lib_');
    root = p.join(tmp.path, 'media');
    media(root, 'Movies/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv', 1);
    media(root, 'Movies/Inception (2010)/inception.mkv', 2);
    File(p.join(root, 'Movies/Inception (2010)/poster.jpg')).writeAsBytesSync([1]);
    media(root, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E01.720p.mkv', 3);
    media(root, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.720p.mkv', 4);
    media(root, 'Music/Queen/A Night at the Opera/11 - Bohemian Rhapsody.mp3', 5);
  });

  tearDown(() => tmp.deleteSync(recursive: true));

  Future<MediaSource> addMedia(Library lib) => lib.addSource(
    MediaSource(
      id: '',
      kind: SourceKind.localFolder,
      name: 'Media',
      path: root,
    ),
  );

  test('scan, views, progress and search through the Library interface', () async {
    final lib = await RustLibrary.open(p.join(tmp.path, 'db', 'core.db'));
    addTearDown(lib.close);
    final src = await addMedia(lib);
    expect(int.tryParse(src.id), isNotNull);
    expect((await lib.sources()).single.path, root);

    final events = await lib.scan().toList();
    expect(events.last.isDone, isTrue);
    expect(events.last.filesFound, 5);
    expect(events.expand((e) => e.errors), isEmpty);

    final movies = await lib.movies().first;
    expect(movies.map((m) => (m.title, m.year)), [
      ('Inception', 2010),
      ('The Matrix', 1999),
    ]);
    expect(movies.first.posterUrl, endsWith('poster.jpg'));
    expect(
      movies.last.filePath,
      p.join(root, 'Movies/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv'),
    );

    final shows = await lib.shows().first;
    expect(shows.single.title, 'Breaking Bad');
    expect(shows.single.kind, ItemKind.show);
    final eps = await lib.episodes(shows.single.id).first;
    expect(eps.map((e) => (e.season, e.episode)), [(1, 1), (1, 2)]);
    expect(eps.first.showId, shows.single.id);

    final tracks = await lib.tracks().first;
    expect(tracks.single.kind, ItemKind.track);
    expect(await lib.recentlyAdded(kind: ItemKind.show).first, hasLength(1));

    // Progress and "Continue watching" update the stream.
    final cw = lib.continueWatching();
    final updates = cw.take(2).toList();
    await Future<void>.delayed(const Duration(milliseconds: 50));
    await lib.saveProgress(
      Progress(
        itemId: movies.last.id,
        positionMs: 600000,
        durationMs: 8000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(1000),
      ),
    );
    final rows = (await updates).last;
    expect(rows.single.$1.id, movies.last.id);
    expect(rows.single.$2.positionMs, 600000);
    expect((await lib.progress(movies.last.id))!.durationMs, 8000000);

    // Up Next after a finished episode.
    await lib.saveProgress(
      Progress(
        itemId: eps.first.id,
        positionMs: 2990000,
        durationMs: 3000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(2000),
      ),
    );
    final row = await lib.continueWatching().first;
    expect(row.first.$1.id, eps.last.id);

    expect((await lib.search('matrix')).first.title, 'The Matrix');
    expect(await lib.search('  '), isEmpty);
    expect(await lib.item('nope'), isNull);

    await lib.fixMatch(eps.first.id, title: 'Во все тяжкие', year: 2008);
    final fixed = await lib.item(shows.single.id);
    expect((fixed!.title, fixed.year), ('Во все тяжкие', 2008));
    expect(
      () => lib.fixMatch('m999999', title: 'x'),
      throwsA(isA<ArgumentError>()),
    );

    await lib.removeSource(src.id);
    expect(await lib.movies().first, isEmpty);
  });

  test('imports the v0 database of the Dart library once', () async {
    final v0Path = p.join(tmp.path, 'kinolune.db');
    final v0 = await Library.open(v0Path);
    await addMedia(v0);
    await v0.scan().drain<void>();
    final v0Movies = await v0.movies().first;
    final matrix = v0Movies.firstWhere((m) => m.title == 'The Matrix');
    await v0.saveProgress(
      Progress(
        itemId: matrix.id,
        positionMs: 1000000,
        durationMs: 8000000,
        updatedAt: DateTime.fromMillisecondsSinceEpoch(5000),
      ),
    );
    await v0.close();

    final corePath = p.join(tmp.path, 'kinolune-core.db');
    final lib = await RustLibrary.open(
      corePath,
      legacyV0Path: v0Path,
      renameLegacyV0: false,
    );
    expect(File(v0Path).existsSync(), isTrue);
    final movies = await lib.movies().first;
    expect(movies.map((m) => m.title), ['Inception', 'The Matrix']);
    final cw = await lib.continueWatching().first;
    expect(cw.single.$1.title, 'The Matrix');
    expect(cw.single.$2.positionMs, 1000000);
    expect(await lib.shows().first, hasLength(1));

    // A second import (next start) adds nothing; then the file is renamed.
    final again = await lib.importLegacyV0(v0Path);
    expect(again!.matched, 0);
    expect(again.progress, 0);
    expect(File(v0Path).existsSync(), isFalse);
    expect(File('$v0Path.v0.bak').existsSync(), isTrue);
    expect(await lib.importLegacyV0(v0Path), isNull);
    expect(await lib.movies().first, hasLength(2));
    await lib.close();
  });

  test(':memory: opens a temporary database', () async {
    final lib = await RustLibrary.open(':memory:');
    expect(await lib.sources(), isEmpty);
    await lib.setTmdbApiKey(' k ');
    expect(await lib.tmdbApiKey(), 'k');
    await lib.close();
  });
}
