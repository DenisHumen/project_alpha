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
    File(
      p.join(root, 'Movies/Inception (2010)/poster.jpg'),
    ).writeAsBytesSync([1]);
    media(root, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E01.720p.mkv', 3);
    media(root, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.720p.mkv', 4);
    media(
      root,
      'Music/Queen/A Night at the Opera/11 - Bohemian Rhapsody.mp3',
      5,
    );
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

  test(
    'scan, views, progress and search through the Library interface',
    () async {
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
    },
  );

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

  // --- openAppLibrary: the app's start (UI-024) -----------------------------

  /// A v0 database in [dataDir] with the media scanned and progress on The
  /// Matrix and the first Breaking Bad episode.
  Future<String> v0Library(String dataDir) async {
    final v0Path = p.join(dataDir, legacyV0DbFileName);
    final v0 = await Library.open(v0Path);
    await addMedia(v0);
    await v0.scan().drain<void>();
    final matrix = (await v0.movies().first).firstWhere(
      (m) => m.title == 'The Matrix',
    );
    final show = (await v0.shows().first).single;
    final ep1 = (await v0.episodes(show.id).first).first;
    for (final (id, pos) in [(matrix.id, 1000000), (ep1.id, 600000)]) {
      await v0.saveProgress(
        Progress(
          itemId: id,
          positionMs: pos,
          durationMs: 3000000,
          updatedAt: DateTime.fromMillisecondsSinceEpoch(5000),
        ),
      );
    }
    expect(await v0.tracks().first, hasLength(1));
    await v0.close();
    return v0Path;
  }

  test(
    'first start on the Rust core imports v0 and keeps it as a backup',
    () async {
      final dataDir = p.join(tmp.path, 'data');
      final v0Path = await v0Library(dataDir);
      final v0Bytes = File(v0Path).readAsBytesSync();

      final opened = await openAppLibrary(dataDir);
      expect(opened.onRustCore, isTrue);
      expect(opened.importedV0, isTrue);
      expect(opened.fallbackReason, isNull);
      final lib = opened.library;
      expect(lib, isA<RustLibrary>());
      expect(File(p.join(dataDir, coreDbFileName)).existsSync(), isTrue);

      // The v0 file is renamed, byte for byte, never deleted.
      expect(File(v0Path).existsSync(), isFalse);
      expect(File('$v0Path.v0.bak').readAsBytesSync(), v0Bytes);

      // Folders, works, progress.
      expect((await lib.sources()).single.path, root);
      expect((await lib.movies().first).map((m) => m.title), [
        'Inception',
        'The Matrix',
      ]);
      final show = (await lib.shows().first).single;
      expect(show.title, 'Breaking Bad');
      expect(await lib.episodes(show.id).first, hasLength(2));
      final cw = await lib.continueWatching().first;
      expect(cw.map((r) => (r.$1.kind, r.$2.positionMs)).toSet(), {
        (ItemKind.movie, 1000000),
        (ItemKind.episode, 600000),
      });
      expect(
        cw.firstWhere((r) => r.$1.kind == ItemKind.movie).$1.title,
        'The Matrix',
      );

      // Music: the import moves the file; its tags come with the scan the app
      // runs after an import.
      final events = await lib.scan().toList();
      expect(events.last.isDone, isTrue);
      expect(await lib.tracks().first, hasLength(1));
      expect(await lib.movies().first, hasLength(2));
      expect(await lib.continueWatching().first, hasLength(2));
      await lib.close();

      // Next start: nothing to import, the data is there.
      final next = await openAppLibrary(dataDir);
      expect((next.onRustCore, next.importedV0), (true, false));
      expect(await next.library.movies().first, hasLength(2));
      expect(await next.library.continueWatching().first, hasLength(2));
      expect(await next.library.tracks().first, hasLength(1));
      await next.library.close();
    },
  );

  test('an older v0 backup is never overwritten', () async {
    final dataDir = p.join(tmp.path, 'data');
    final v0Path = await v0Library(dataDir);
    File('$v0Path.v0.bak').writeAsStringSync('older backup');

    final opened = await openAppLibrary(dataDir);
    expect(opened.importedV0, isTrue);
    expect(File('$v0Path.v0.bak').readAsStringSync(), 'older backup');
    expect(File('$v0Path.v0.bak.1').existsSync(), isTrue);
    expect(newestLegacyV0Backup(v0Path), '$v0Path.v0.bak.1');
    expect(freeLegacyV0BackupPath(v0Path), '$v0Path.v0.bak.2');
    await opened.library.close();
  });

  test('falls back to v0 when the native core cannot be loaded', () async {
    final dataDir = p.join(tmp.path, 'data');
    final v0Path = await v0Library(dataDir);

    final opened = await openAppLibrary(
      dataDir,
      initCore: () async => throw ArgumentError('libpa_frb.so not found'),
    );
    expect(opened.onRustCore, isFalse);
    expect(opened.fallbackReason, isA<ArgumentError>());
    expect(opened.library, isNot(isA<RustLibrary>()));
    expect(File(v0Path).existsSync(), isTrue);
    expect(File(p.join(dataDir, coreDbFileName)).existsSync(), isFalse);
    expect(await opened.library.movies().first, hasLength(2));
    expect(await opened.library.continueWatching().first, hasLength(2));
    await opened.library.close();
  });

  test('v0 fallback after a migration moves the backup back', () async {
    final dataDir = p.join(tmp.path, 'data');
    final v0Path = await v0Library(dataDir);
    await (await openAppLibrary(dataDir)).library.close();
    expect(File(v0Path).existsSync(), isFalse);

    final opened = await openAppLibrary(dataDir, rustCore: false);
    expect(opened.onRustCore, isFalse);
    expect(opened.fallbackReason, isNull);
    expect(File(v0Path).existsSync(), isTrue);
    expect(File('$v0Path.v0.bak').existsSync(), isFalse);
    expect(await opened.library.movies().first, hasLength(2));
    await opened.library.close();

    // Back on the core: the v0 file is imported again (nothing new) and
    // renamed again; the core data was kept.
    final again = await openAppLibrary(dataDir);
    expect((again.onRustCore, again.importedV0), (true, true));
    expect(File('$v0Path.v0.bak').existsSync(), isTrue);
    expect(await again.library.movies().first, hasLength(2));
    expect(await again.library.continueWatching().first, hasLength(2));
    await again.library.close();
  });

  test('metadata language is taken as is, without a reset', () async {
    final lib = await RustLibrary.open(':memory:');
    expect(lib.metadataLanguage, RustLibrary.defaultMetadataLanguage);
    await lib.setMetadataLanguage('en-US');
    expect(lib.metadataLanguage, 'en-US');
    await lib.setMetadataLanguage(' ');
    expect(lib.metadataLanguage, 'ru-RU');
    await lib.close();
  });
}
