// SPDX-License-Identifier: MPL-2.0
// MUS-004: music grouping (albums with discs, artists, genres, composers,
// folders), the Hi-Res / Lossless / downloaded chips, Home rows and mixes,
// format badges and the v0 fallback; then RustLibrary on the native core.

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'native_library.dart';

MusicTrack track(
  String id, {
  String album = 'Album',
  List<String> albumArtists = const ['Band'],
  List<String> artists = const ['Band'],
  List<String> genres = const [],
  List<String> composers = const [],
  int? disc,
  int? discTotal,
  int? number,
  String codec = 'flac',
  int? rate = 44100,
  int? bits = 16,
  bool local = true,
  int plays = 0,
  DateTime? lastPlayed,
  DateTime? added,
  String folder = '/m/Band/Album',
}) => MusicTrack(
  item: LibraryItem(
    id: id,
    kind: ItemKind.track,
    title: 'Song $id',
    album: album,
    artist: artists.isEmpty ? null : artists.first,
    trackNumber: number,
    filePath: '$folder/$id.$codec',
    durationMs: 60000,
    addedAt: added,
  ),
  albumId: null,
  albumArtists: albumArtists,
  artists: artists,
  genres: genres,
  composers: composers,
  discNumber: disc,
  discTotal: discTotal,
  codec: codec,
  sampleRateHz: rate,
  bitDepth: bits,
  local: local,
  playCount: plays,
  lastPlayedAt: lastPlayed,
);

void main() {
  group('format badge', () {
    test('lossless with depth and rate, lossy with bitrate', () {
      expect(
        formatBadge(
          codec: 'flac',
          sampleRateHz: 96000,
          bitDepth: 24,
          lossless: true,
        ),
        'FLAC 24/96',
      );
      expect(
        formatBadge(
          codec: 'alac',
          sampleRateHz: 44100,
          bitDepth: 16,
          lossless: true,
        ),
        'ALAC 16/44.1',
      );
      expect(formatBadge(codec: 'dsd', sampleRateHz: 2822400), 'DSD64');
      expect(formatBadge(codec: 'mp3', bitrateKbps: 320), 'MP3 320');
      expect(
        formatBadge(codec: 'pcm', lossless: true, extension: 'AIFF'),
        'AIFF',
      );
      expect(formatBadge(), isNull);
    });

    test('Hi-Res is more than 48 kHz or more than 16 bits', () {
      expect(track('a', rate: 48000, bits: 16).isHiRes, isFalse);
      expect(track('a', rate: 96000, bits: 16).isHiRes, isTrue);
      expect(track('a', rate: 44100, bits: 24).isHiRes, isTrue);
      expect(track('a', codec: 'mp3', rate: null, bits: null).isHiRes, isFalse);
    });

    test('lossless codecs; the tag wins', () {
      for (final c in ['flac', 'alac', 'pcm', 'ape', 'wavpack']) {
        expect(track('a', codec: c).isLossless, isTrue, reason: c);
      }
      expect(track('a', codec: 'mp3').isLossless, isFalse);
      expect(
        MusicTrack(
          item: track('a').item,
          codec: 'dsd',
          lossless: true,
        ).isLossless,
        isTrue,
      );
    });
  });

  group('index', () {
    test('albums group by album and split into discs', () {
      final idx = MusicIndex([
        track('2', disc: 2, number: 1, discTotal: 2),
        track('1', disc: 1, number: 2, discTotal: 2),
        track('0', disc: 1, number: 1, discTotal: 2),
        track('x', album: 'Other', folder: '/m/Band/Other'),
      ]);
      expect([for (final a in idx.albums) a.title], ['Album', 'Other']);
      final album = idx.albums.first;
      expect([for (final t in album.tracks) t.id], ['0', '1', '2']);
      expect(album.isMultiDisc, isTrue);
      expect(album.discs, [1, 2]);
      expect([for (final t in album.disc(2)) t.id], ['2']);
      expect(album.formatLabel, 'FLAC 16/44.1');
      expect(idx.albums.last.isMultiDisc, isFalse);
    });

    test('artists own albums and appear on compilations', () {
      final idx = MusicIndex([
        track('a1', album: 'Own', artists: ['Mara'], albumArtists: ['Mara']),
        track(
          'c1',
          album: 'Hits',
          artists: ['Mara', 'Guest'],
          albumArtists: ['Various Artists'],
          folder: '/m/Hits',
        ),
        track(
          'c2',
          album: 'Hits',
          artists: ['Other'],
          albumArtists: ['Various Artists'],
          folder: '/m/Hits',
          plays: 5,
        ),
      ]);
      final mara = idx.artistNamed('  mara ')!;
      expect([for (final a in mara.albums) a.title], ['Own']);
      expect([for (final a in mara.appearsOn) a.title], ['Hits']);
      expect([for (final t in mara.tracks) t.id], ['a1', 'c1']);
      final va = idx.artistNamed('Various Artists')!;
      expect([for (final a in va.albums) a.title], ['Hits']);
      expect(va.popular.first.id, 'c2');
      expect(
        [for (final a in idx.artists) a.name],
        ['Guest', 'Mara', 'Other', 'Various Artists'],
      );
    });

    test('genres, composers and folders', () {
      final idx = MusicIndex([
        track('1', genres: ['Jazz', 'Blues'], composers: ['Bach']),
        track('2', genres: ['jazz'], folder: '/m/B'),
        track('3', genres: []),
      ]);
      expect([for (final g in idx.genres) g.name], ['Blues', 'Jazz']);
      expect(idx.genres.last.tracks, hasLength(2));
      expect([for (final c in idx.composers) c.name], ['Bach']);
      expect([for (final f in idx.folders) f.name], ['/m/B', '/m/Band/Album']);
    });

    test('chips filter tracks and everything built on them', () {
      final idx = MusicIndex([
        track('hr', rate: 96000, bits: 24),
        track('cd'),
        track('mp3', codec: 'mp3', rate: 44100, bits: null, local: false),
      ]);
      ids(MusicIndex i) => [for (final t in i.tracks) t.id];
      expect(ids(idx.filter({MusicChip.hiRes})), ['hr']);
      expect(ids(idx.filter({MusicChip.lossless})), ['hr', 'cd']);
      expect(ids(idx.filter({MusicChip.downloaded})), ['hr', 'cd']);
      expect(ids(idx.filter({MusicChip.hiRes, MusicChip.lossless})), ['hr']);
      expect(identical(idx.filter({}), idx), isTrue);
      expect(idx.filter({MusicChip.hiRes}).albums.single.tracks, hasLength(1));
    });

    test('recently played, frequently played, recently added', () {
      final t0 = DateTime(2026, 11, 1);
      final idx = MusicIndex([
        track(
          'a',
          plays: 2,
          lastPlayed: t0.add(const Duration(hours: 1)),
          added: t0,
        ),
        track(
          'b',
          plays: 7,
          lastPlayed: t0,
          album: 'B',
          folder: '/m/B',
          added: t0.add(const Duration(days: 2)),
        ),
        track('c', album: 'C', folder: '/m/C'),
      ]);
      expect([for (final t in idx.recentlyPlayed()) t.id], ['a', 'b']);
      expect([for (final t in idx.frequentlyPlayed()) t.id], ['b', 'a']);
      expect(
        [for (final a in idx.recentlyPlayedAlbums()) a.title],
        ['Album', 'B'],
      );
      expect([for (final a in idx.recentlyAddedAlbums()) a.title], ['B', 'Album']);
    });

    test('mixes are deterministic', () {
      final tracks = [
        for (var i = 0; i < 12; i++)
          track('t$i', genres: [i.isEven ? 'Jazz' : 'Rock']),
      ];
      final a = MusicIndex(tracks).mixes(favoriteIds: {'t1', 't2', 't3'});
      final b = MusicIndex(tracks.reversed.toList()).mixes(
        favoriteIds: {'t3', 't2', 't1'},
      );
      expect([for (final m in a) m.kind], [
        MusicMixKind.favorites,
        MusicMixKind.genre,
        MusicMixKind.genre,
      ]);
      expect([for (final m in a) m.name], ['', 'Jazz', 'Rock']);
      for (var i = 0; i < a.length; i++) {
        expect(
          [for (final t in a[i].tracks) t.id],
          [for (final t in b[i].tracks) t.id],
        );
      }
      // Shuffled, not in library order.
      final jazz = [for (final t in a[1].tracks) t.id];
      expect(jazz.toSet(), {'t0', 't2', 't4', 't6', 't8', 't10'});
      expect(MusicIndex(tracks).mixes(favoriteIds: {'t1'}).first.kind,
          MusicMixKind.genre);
    });
  });

  group('v0 fallback', () {
    test('plain tracks get a guessed format and counted plays', () async {
      final lib = _TracksLibrary([
        const LibraryItem(
          id: '1',
          kind: ItemKind.track,
          title: 'One',
          artist: 'Band',
          album: 'Album',
          filePath: '/m/Band/Album/01.flac',
        ),
        const LibraryItem(
          id: '2',
          kind: ItemKind.track,
          title: 'Two',
          filePath: 'smb://nas/m/02.mp3',
        ),
      ]);
      final music = libraryMusic(lib);
      expect(identical(music, libraryMusic(lib)), isTrue);
      expect(music.hasMusicPlaylists, isFalse);
      final first = await music.musicTracks().first;
      expect(first.first.isLossless, isTrue);
      expect(first.first.local, isTrue);
      expect(first.first.artist, 'Band');
      expect(first.last.codec, 'mp3');
      expect(first.last.local, isFalse);

      final later = music.musicTracks().firstWhere(
        (l) => l.last.playCount == 1,
      );
      await music.recordTrackPlay('2');
      final after = await later.timeout(const Duration(seconds: 5));
      expect(after.last.playCount, 1);
      expect(after.last.lastPlayedAt, isNotNull);
    });
  });

  group('RustLibrary', () {
    late Directory tmp;
    late RustLibrary lib;

    setUpAll(() async {
      await RustLibrary.ensureInitialized(
        externalLibrary: openBuiltCoreLibrary(),
      );
    });

    setUp(() async {
      tmp = Directory.systemTemp.createTempSync('pa_music_');
      final root = p.join(tmp.path, 'media');
      for (final (rel, seed) in [
        ('Music/Band/Album/01 - One.flac', 1),
        ('Music/Band/Album/02 - Two.flac', 2),
      ]) {
        final f = File(p.join(root, rel));
        f.parent.createSync(recursive: true);
        f.writeAsBytesSync(List.generate(150000, (i) => (i * 31 + seed) & 0xff));
      }
      lib = await RustLibrary.open(p.join(tmp.path, 'db', 'core.db'));
      await lib.addSource(
        MediaSource(
          id: '',
          kind: SourceKind.localFolder,
          name: 'Media',
          path: root,
        ),
      );
      await lib.scan().toList();
    });

    tearDown(() async {
      await lib.close();
      tmp.deleteSync(recursive: true);
    });

    test('tracks, plays and audio playlists', () async {
      expect(libraryMusic(lib), same(lib));
      final tracks = await lib.musicTracks().first;
      expect(tracks, hasLength(2));
      expect(tracks.every((t) => t.local), isTrue);
      expect(tracks.first.albumId, isNotNull);
      expect(MusicIndex(tracks).albums, hasLength(1));

      final next = lib.musicTracks().firstWhere(
        (l) => l.any((t) => t.playCount == 1),
      );
      await lib.recordTrackPlay(tracks.first.id);
      final played = await next.timeout(const Duration(seconds: 10));
      expect(played.firstWhere((t) => t.id == tracks.first.id).playCount, 1);

      final pl = await lib.createMusicPlaylist('Drive');
      expect(await lib.addToMusicPlaylist(pl.id, tracks.last.id), isTrue);
      expect(await lib.addToMusicPlaylist(pl.id, tracks.last.id), isFalse);
      final lists = await lib.musicPlaylists().first;
      expect(lists.single.trackCount, 1);
      expect(
        [for (final i in await lib.musicPlaylistItems(pl.id)) i.id],
        [tracks.last.id],
      );
      expect(await lib.playlists(), isEmpty, reason: 'not a video playlist');
      await lib.deleteMusicPlaylist(pl.id);
      expect(await lib.musicPlaylists().first, isEmpty);
    });
  });
}

/// A [Library] with only tracks, like the v0 one.
class _TracksLibrary implements Library {
  _TracksLibrary(this.items);
  final List<LibraryItem> items;

  @override
  Stream<List<LibraryItem>> tracks() => Stream.value(items);

  @override
  dynamic noSuchMethod(Invocation invocation) => super.noSuchMethod(invocation);
}
