// SPDX-License-Identifier: MPL-2.0

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'fake_tmdb.dart';

void touch(String root, String rel, [int size = 16]) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.filled(size, 1));
}

void main() {
  late Directory tmp;
  late String media;
  late FakeTmdb fake;
  late Library lib;

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('pa_lib_');
    media = p.join(tmp.path, 'media');
    fake = FakeTmdb();
    touch(media, 'Movies/The.Matrix.1999.1080p.BluRay.x264-GRP.mkv');
    touch(media, 'Movies/Inception (2010)/inception.mkv');
    touch(media, 'Movies/Unknown Film 2003.avi');
    touch(media, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E01.720p.mkv');
    touch(media, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.720p.mkv');
    touch(media, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.sample.mkv');
    touch(media, 'Anime/[SubsPlease] Sousou no Frieren - 01 (1080p).mkv');
    touch(media, 'Anime/[SubsPlease] Sousou no Frieren - 02 (1080p).mkv');
    touch(media, 'Music/Queen/A Night at the Opera/11 - Bohemian Rhapsody.mp3');
    lib = await Library.open(
      p.join(tmp.path, 'db', 'library.sqlite'),
      httpClient: fake.client,
    );
  });

  tearDown(() async {
    await lib.close();
    tmp.deleteSync(recursive: true);
  });

  Future<List<ScanProgress>> scanAll() => lib.scan().toList();

  test('sources CRUD', () async {
    expect(await lib.sources(), isEmpty);
    final s = await lib.addSource(
      MediaSource(
        id: '',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    expect(s.id, isNotEmpty);
    final smb = await lib.addSource(
      const MediaSource(
        id: 'nas',
        kind: SourceKind.smb,
        name: 'NAS',
        path: 'smb://nas/media',
        username: 'bob',
      ),
    );
    expect((await lib.sources()).map((e) => e.id), [s.id, 'nas']);
    await lib.addSource(smb.copyWith(name: 'NAS 2'));
    expect((await lib.sources()).last.name, 'NAS 2');
    await lib.removeSource('nas');
    expect(await lib.sources(), [s]);
    expect(
      () => lib.addSource(
        const MediaSource(
          id: '',
          kind: SourceKind.smb,
          name: 'bad',
          path: '/not/smb',
        ),
      ),
      throwsFormatException,
    );
  });

  test('scan without TMDB key uses parsed titles', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    final events = await scanAll();
    expect(events.last.phase, ScanPhase.done);
    expect(events.last.errors, isEmpty);
    expect(events.last.filesFound, 8);
    expect(fake.requests, isEmpty);

    final movies = await lib.movies().first;
    expect(movies.map((m) => (m.title, m.year)).toList(), [
      ('Inception', 2010),
      ('The Matrix', 1999),
      ('Unknown Film', 2003),
    ]);
    expect(movies.every((m) => m.posterUrl == null), isTrue);

    final shows = await lib.shows().first;
    expect(shows.map((s) => s.title), ['Breaking Bad', 'Sousou no Frieren']);
    final bb = shows.first;
    expect(bb.id, showIdFor('Breaking Bad'));
    final eps = await lib.episodes(bb.id).first;
    expect(eps.map((e) => (e.season, e.episode)), [(1, 1), (1, 2)]);
    expect(eps.first.title, 'Episode 1');
    expect(eps.first.showId, bb.id);
    expect(eps.first.filePath, endsWith('Breaking.Bad.S01E01.720p.mkv'));

    final frieren = await lib.episodes(shows[1].id).first;
    expect(frieren.map((e) => e.episode), [1, 2]);

    final tracks = await lib.tracks().first;
    expect(tracks.single.title, 'Bohemian Rhapsody');
    expect(tracks.single.artist, 'Queen');
    expect(tracks.single.album, 'A Night at the Opera');
    expect(tracks.single.kind, ItemKind.track);
  });

  test('scan with TMDB key fetches posters and episode titles', () async {
    await lib.setTmdbApiKey('test-key');
    expect(await lib.tmdbApiKey(), 'test-key');
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    final events = await scanAll();
    expect(events.any((e) => e.phase == ScanPhase.metadata), isTrue);
    expect(events.last.errors, isEmpty);

    final matrix = (await lib.movies().first).firstWhere(
      (m) => m.title == 'The Matrix',
    );
    expect(matrix.tmdbId, 603);
    expect(
      matrix.posterUrl,
      'https://image.tmdb.org/t/p/w500/fixture-matrix-poster.jpg',
    );
    expect(matrix.overview, startsWith('Fixture overview'));
    expect(matrix.matchConfidence, greaterThan(0.9));

    final unknown = (await lib.movies().first).firstWhere(
      (m) => m.title == 'Unknown Film',
    );
    expect(unknown.tmdbId, isNull);
    expect(unknown.year, 2003);

    final bb = (await lib.shows().first).firstWhere(
      (s) => s.title == 'Breaking Bad',
    );
    expect(bb.tmdbId, 1396);
    expect(bb.year, 2008);
    final eps = await lib.episodes(bb.id).first;
    expect(eps.map((e) => e.title), ['Pilot', "Cat's in the Bag..."]);
    expect(eps.first.posterUrl, bb.posterUrl);
    expect(
      eps.first.backdropUrl,
      'https://image.tmdb.org/t/p/w780/fixture-bb-s1e1.jpg',
    );

    // Second scan: nothing changed → no new TMDB requests.
    final before = fake.requests.length;
    await scanAll();
    expect(fake.requests.length, before);
  });

  test('invalid TMDB key is reported, library still filled', () async {
    fake.forceStatus = 401;
    await lib.setTmdbApiKey('bad');
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    final events = await scanAll();
    expect(events.last.errors, contains('TMDB: invalid API key'));
    expect(await lib.movies().first, hasLength(3));
  });

  test('removed files disappear, offline source keeps items', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    File(p.join(media, 'Movies/Unknown Film 2003.avi')).deleteSync();
    Directory(p.join(media, 'Anime')).deleteSync(recursive: true);
    await scanAll();
    expect(await lib.movies().first, hasLength(2));
    expect((await lib.shows().first).map((s) => s.title), ['Breaking Bad']);

    // Source folder unavailable (unplugged drive): items are kept.
    final moved = '$media.off';
    Directory(media).renameSync(moved);
    final events = await scanAll();
    expect(events.last.errors.single, contains('folder not found'));
    expect(await lib.movies().first, hasLength(2));
    Directory(moved).renameSync(media);
  });

  test('removeSource deletes its items', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    await lib.removeSource('m');
    expect(await lib.movies().first, isEmpty);
    expect(await lib.shows().first, isEmpty);
    expect(await lib.tracks().first, isEmpty);
  });

  test('scanning a single source / unknown source', () async {
    final other = p.join(tmp.path, 'other');
    touch(other, 'Heat (1995).mkv');
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await lib.addSource(
      MediaSource(
        id: 'o',
        kind: SourceKind.localFolder,
        name: 'Other',
        path: other,
      ),
    );
    await lib.scan(sourceId: 'o').toList();
    expect((await lib.movies().first).map((m) => m.title), ['Heat']);
    final ev = await lib.scan(sourceId: 'zzz').toList();
    expect(ev.last.error, contains('Unknown source'));
  });

  test('SMB source on an unreachable server reports an error', () async {
    final l = await Library.open(
      ':memory:',
      httpClient: fake.client,
      smbResolver: SmbResolver(
        gvfsRootOverride: tmp.path,
        runner: (exe, args, {stdinText}) async =>
            ProcessResult(0, 2, '', 'nope'),
      ),
    );
    await l.addSource(
      const MediaSource(
        id: 's',
        kind: SourceKind.smb,
        name: 'NAS',
        path: 'smb://nas/media',
      ),
    );
    final ev = await l.scan(smbPassword: 'pw').toList();
    expect(ev.last.phase, ScanPhase.done);
    expect(ev.last.errors.single, contains('Could not mount'));
    await l.close();
  });

  test('progress and continueWatching (2%..95%)', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    final movies = await lib.movies().first;
    final t0 = DateTime(2026, 1, 1);
    await lib.saveProgress(
      Progress(
        itemId: movies[0].id,
        positionMs: 1000,
        durationMs: 100000,
        updatedAt: t0,
      ),
    ); // 1%
    await lib.saveProgress(
      Progress(
        itemId: movies[1].id,
        positionMs: 50000,
        durationMs: 100000,
        updatedAt: t0.add(const Duration(minutes: 1)),
      ),
    ); // 50%
    await lib.saveProgress(
      Progress(
        itemId: movies[2].id,
        positionMs: 30000,
        durationMs: 100000,
        updatedAt: t0.add(const Duration(minutes: 2)),
      ),
    ); // 30%
    var cw = await lib.continueWatching().first;
    expect(cw.map((e) => e.$1.id), [movies[2].id, movies[1].id]);
    expect(cw.first.$2.positionMs, 30000);
    expect(cw.first.$2.fraction, closeTo(0.3, 1e-9));

    // 96% → watched, leaves the row.
    await lib.saveProgress(
      Progress(
        itemId: movies[2].id,
        positionMs: 96000,
        durationMs: 100000,
        updatedAt: t0.add(const Duration(minutes: 3)),
      ),
    );
    cw = await lib.continueWatching().first;
    expect(cw.map((e) => e.$1.id), [movies[1].id]);
    expect((await lib.progress(movies[2].id))!.watched, isTrue);
    expect((await lib.item(movies[2].id))!.durationMs, 100000);
  });

  test('streams re-emit on changes', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    final seen = <int>[];
    final sub = lib.movies().listen((m) => seen.add(m.length));
    await scanAll();
    await Future<void>.delayed(Duration.zero);
    await sub.cancel();
    expect(seen.first, 0);
    expect(seen.last, 3);
  });

  test('fixMatch by title without key, then by TMDB id', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    final unknown = (await lib.movies().first).firstWhere(
      (m) => m.title == 'Unknown Film',
    );
    await lib.fixMatch(unknown.id, title: 'Inception', year: 2010);
    var fixed = (await lib.item(unknown.id))!;
    expect(fixed.title, 'Inception');
    expect(fixed.year, 2010);
    expect(fixed.matchConfidence, 1.0);

    await lib.setTmdbApiKey('k');
    await lib.fixMatch(unknown.id, tmdbId: 27205);
    fixed = (await lib.item(unknown.id))!;
    expect(fixed.tmdbId, 27205);
    expect(
      fixed.posterUrl,
      'https://image.tmdb.org/t/p/w500/fixture-inception-poster.jpg',
    );

    // Locked items survive a rescan.
    await scanAll();
    fixed = (await lib.item(unknown.id))!;
    expect(fixed.title, 'Inception');
    expect(fixed.tmdbId, 27205);
  });

  test('fixMatch on an episode applies to the show', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    final frieren = (await lib.shows().first).firstWhere(
      (s) => s.title.contains('Frieren'),
    );
    final ep = (await lib.episodes(frieren.id).first).first;
    await lib.setTmdbApiKey('k');
    await lib.fixMatch(ep.id, title: 'Breaking Bad');
    final show = (await lib.item(frieren.id))!;
    expect(show.tmdbId, 1396);
    expect(show.title, 'Breaking Bad');
  });

  test('search is case-insensitive and Unicode-aware', () async {
    touch(media, 'Movies/Брат.1997.mkv');
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    expect((await lib.search('МАТРИ')), isEmpty);
    expect((await lib.search('брат')).single.title, 'Брат');
    expect((await lib.search('matrix')).single.title, 'The Matrix');
    expect((await lib.search('queen')).single.kind, ItemKind.track);
    expect(await lib.search('  '), isEmpty);
  });

  test('recentlyAdded', () async {
    await lib.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await scanAll();
    expect(await lib.recentlyAdded().first, hasLength(3));
    expect(
      await lib.recentlyAdded(kind: ItemKind.show, limit: 1).first,
      hasLength(1),
    );
  });

  test('database persists across open/close', () async {
    final dbPath = p.join(tmp.path, 'persist.sqlite');
    var l = await Library.open(dbPath, httpClient: fake.client);
    await l.addSource(
      MediaSource(
        id: 'm',
        kind: SourceKind.localFolder,
        name: 'Media',
        path: media,
      ),
    );
    await l.scan().toList();
    await l.close();
    l = await Library.open(dbPath, httpClient: fake.client);
    expect(await l.movies().first, hasLength(3));
    expect(await l.sources(), hasLength(1));
    await l.close();
  });
}
