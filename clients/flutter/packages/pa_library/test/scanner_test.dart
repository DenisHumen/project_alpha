// SPDX-License-Identifier: MPL-2.0

import 'dart:convert';
import 'dart:io';

import 'package:pa_library/src/music_tags.dart';
import 'package:pa_library/src/scanner.dart';
import 'package:pa_library/src/smb.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

void touch(String root, String rel, [int size = 16]) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.filled(size, 0));
}

void main() {
  late Directory tmp;
  setUp(() => tmp = Directory.systemTemp.createTempSync('pa_scan_'));
  tearDown(() => tmp.deleteSync(recursive: true));

  test('finds video and audio recursively, skips extras and junk', () async {
    final r = tmp.path;
    touch(r, 'Movies/The Matrix (1999)/The.Matrix.1999.1080p.mkv');
    touch(r, 'Movies/The Matrix (1999)/The.Matrix.1999.1080p.nfo');
    touch(r, 'Movies/The Matrix (1999)/sample-matrix.mkv');
    touch(r, 'Movies/The Matrix (1999)/Sample/matrix.sample.mkv');
    touch(r, 'Movies/The Matrix (1999)/Extras/making-of.mkv');
    touch(r, 'Movies/The Matrix (1999)/Featurettes/vfx.mkv');
    touch(r, 'Movies/Heat (1995)/Heat-trailer.mp4');
    touch(r, 'Movies/Heat (1995)/Heat.mp4');
    touch(r, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E01.mkv');
    touch(r, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.MKV');
    touch(r, 'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.srt');
    touch(r, 'Music/Artist/Album/01 - Song.flac');
    touch(r, 'Music/Artist/Album/cover.jpg');
    touch(r, '.hidden/secret.mkv');
    touch(r, '@eaDir/thumb.mp4');
    touch(r, 'Disc/BDMV/STREAM/00001.m2ts');
    touch(r, 'Movies/.partial.mkv');

    final files = await scanFolder(r).toList();
    final rel = files.map((f) => p.relative(f.path, from: r)).toSet();
    expect(rel, {
      'Movies/The Matrix (1999)/The.Matrix.1999.1080p.mkv',
      'Movies/Heat (1995)/Heat.mp4',
      'TV/Breaking Bad/Season 1/Breaking.Bad.S01E01.mkv',
      'TV/Breaking Bad/Season 1/Breaking.Bad.S01E02.MKV',
      'Music/Artist/Album/01 - Song.flac',
    });
    final song = files.firstWhere((f) => f.path.endsWith('.flac'));
    expect(song.type, MediaType.audio);
    expect(song.size, 16);
    expect(files.where((f) => f.type == MediaType.video), hasLength(4));
  });

  test('survives symlink loops', () async {
    touch(tmp.path, 'a/movie.mkv');
    Link(p.join(tmp.path, 'a', 'loop')).createSync(tmp.path);
    final files = await scanFolder(tmp.path).toList();
    expect(files, hasLength(1));
  });

  test('missing folder reports an error and yields nothing', () async {
    final errors = <String>[];
    final files = await scanFolder(
      p.join(tmp.path, 'nope'),
      onError: (path, e) => errors.add(path),
    ).toList();
    expect(files, isEmpty);
    expect(errors, hasLength(1));
  });

  test('empty folder', () async {
    expect(await scanFolder(tmp.path).toList(), isEmpty);
  });

  test('mediaTypeOf / isExtraFile', () {
    expect(mediaTypeOf('a.MKV'), MediaType.video);
    expect(mediaTypeOf('a.opus'), MediaType.audio);
    expect(mediaTypeOf('a.txt'), isNull);
    expect(isExtraFile('Movie-trailer.mkv'), isTrue);
    expect(isExtraFile('Movie-featurette.mkv'), isTrue);
    expect(isExtraFile('movie.sample.mkv'), isTrue);
    expect(isExtraFile('Samples of Life (2010).mkv'), isFalse);
    expect(isExtraFile('The.Trailer.Park.Boys.S01E01.mkv'), isFalse);
  });

  group('music path fallback', () {
    test('Artist/Album/NN - Title', () {
      final t = trackInfoFromPath(
        '/m/Queen/A Night at the Opera/11 - Bohemian Rhapsody.mp3',
        root: '/m',
      );
      expect(t.artist, 'Queen');
      expect(t.album, 'A Night at the Opera');
      expect(t.trackNumber, 11);
      expect(t.title, 'Bohemian Rhapsody');
    });
    test('"Artist - Album (Year)" folder', () {
      final t = trackInfoFromPath(
        '/m/Кино - Группа крови (1988)/01. Группа крови.flac',
        root: '/m',
      );
      expect(t.artist, 'Кино');
      expect(t.album, 'Группа крови');
      expect(t.title, 'Группа крови');
      expect(t.trackNumber, 1);
    });
    test('CD folders are skipped', () {
      final t = trackInfoFromPath(
        '/m/Artist/Album/CD2/03 Song.mp3',
        root: '/m',
      );
      expect(t.album, 'Album');
      expect(t.artist, 'Artist');
      expect(t.title, 'Song');
    });
    test('loose "Artist - Title" file', () {
      final t = trackInfoFromPath(
        '/m/Music/Daft Punk - One More Time.mp3',
        root: '/m',
      );
      expect(t.artist, 'Daft Punk');
      expect(t.title, 'One More Time');
      expect(t.album, isNull);
    });
    test('ID3v2 tags win over the path', () {
      List<int> frame(String id, String text) {
        final data = [0x03, ...utf8.encode(text)]; // UTF-8 encoding byte
        final n = data.length;
        return [
          ...ascii.encode(id),
          n >> 24,
          (n >> 16) & 0xff,
          (n >> 8) & 0xff,
          n & 0xff,
          0,
          0,
          ...data,
        ];
      }

      final frames = [
        ...frame('TIT2', 'Группа крови'),
        ...frame('TPE1', 'Кино'),
        ...frame('TALB', 'Группа крови'),
        ...frame('TRCK', '1/11'),
      ];
      final size = frames.length;
      final header = [
        ...ascii.encode('ID3'), 4, 0, 0, // v2.4
        (size >> 21) & 0x7f,
        (size >> 14) & 0x7f,
        (size >> 7) & 0x7f,
        size & 0x7f,
      ];
      final f = File(p.join(tmp.path, 'x', 'y', 'track01.mp3'));
      f.parent.createSync(recursive: true);
      f.writeAsBytesSync([...header, ...frames, ...List.filled(256, 0)]);
      final t = trackInfo(f.path, root: tmp.path);
      expect(t.title, 'Группа крови');
      expect(t.artist, 'Кино');
      expect(t.album, 'Группа крови');
      expect(t.trackNumber, 1);
    });
    test('unreadable tags fall back to the path', () {
      touch(tmp.path, 'Artist/Album/02 - Track.mp3');
      final t = trackInfo(
        p.join(tmp.path, 'Artist/Album/02 - Track.mp3'),
        root: tmp.path,
      );
      expect(t.title, 'Track');
      expect(t.artist, 'Artist');
    });
  });

  test('findLocalImage', () {
    touch(tmp.path, 'Album/01.mp3');
    touch(tmp.path, 'Album/Cover.JPG');
    expect(
      findLocalImage(p.join(tmp.path, 'Album/01.mp3')),
      endsWith('Cover.JPG'),
    );
    touch(tmp.path, 'Movie/movie.mkv');
    touch(tmp.path, 'Movie/folder.jpg');
    expect(
      findLocalImage(p.join(tmp.path, 'Movie/movie.mkv'), video: true),
      startsWith('file://'),
    );
    expect(
      findLocalImage(
        p.join(tmp.path, 'Movie/movie.mkv'),
        video: true,
        allowGeneric: false,
      ),
      isNull,
    );
    touch(tmp.path, 'Movie/movie-poster.png');
    expect(
      findLocalImage(
        p.join(tmp.path, 'Movie/movie.mkv'),
        video: true,
        allowGeneric: false,
      ),
      endsWith('movie-poster.png'),
    );
  });

  group('smb', () {
    test('SmbUrl.parse', () {
      final u = SmbUrl.parse('smb://nas.local/Media/Movies/HD');
      expect(u.host, 'nas.local');
      expect(u.share, 'Media');
      expect(u.path, 'Movies/HD');
      expect(u.shareUrl, 'smb://nas.local/Media');
      expect(SmbUrl.parse('smb://bob@nas/share').user, 'bob');
      expect(() => SmbUrl.parse('/mnt/nas'), throwsFormatException);
      expect(() => SmbUrl.parse('smb://nas'), throwsFormatException);
    });
    test('findGvfsPath', () {
      Directory(
        p.join(tmp.path, 'smb-share:server=nas.local,share=media,user=bob'),
      ).createSync();
      Directory(
        p.join(tmp.path, 'smb-share:server=other,share=media'),
      ).createSync();
      final u = SmbUrl.parse('smb://NAS.local/Media/Movies');
      expect(
        findGvfsPath(u, tmp.path),
        p.join(
          tmp.path,
          'smb-share:server=nas.local,share=media,user=bob',
          'Movies',
        ),
      );
      expect(findGvfsPath(SmbUrl.parse('smb://x/y'), tmp.path), isNull);
    });
    test('resolver finds a share the user mounted', () async {
      Directory(
        p.join(tmp.path, 'smb-share:server=nas,share=media'),
      ).createSync();
      final resolver = SmbResolver(gvfsRootOverride: tmp.path);
      expect(
        await resolver.resolve('smb://nas/media/TV'),
        p.join(tmp.path, 'smb-share:server=nas,share=media', 'TV'),
      );
      expect(
        await resolver.mounted('smb://nas/media'),
        p.join(tmp.path, 'smb-share:server=nas,share=media'),
      );
    });
    test('resolver never mounts: a share that is not mounted fails', () async {
      final resolver = SmbResolver(gvfsRootOverride: tmp.path);
      await expectLater(
        resolver.resolve('smb://nas/media'),
        throwsA(
          isA<SmbMountException>().having(
            (e) => e.message,
            'message',
            contains('not mounted'),
          ),
        ),
      );
      expect(await resolver.mounted('smb://nas/media'), isNull);
    });
  }, testOn: 'linux');
}
