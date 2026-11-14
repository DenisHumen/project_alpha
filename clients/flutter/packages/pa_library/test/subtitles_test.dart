// SPDX-License-Identifier: MPL-2.0
// PLAY-007 through RustLibrary on the real native core: sidecar subtitles of
// the played file, with the flags from their names; stream URLs map back to
// their source and path.

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'native_library.dart';

void media(String root, String rel, int seed) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.generate(150000, (i) => (i * 31 + seed) & 0xff));
}

void subtitle(String root, String rel) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsStringSync('1\n00:00:01,000 --> 00:00:02,000\nHi\n');
}

void main() {
  group('stream URLs', () {
    test('parse back to the source and the decoded path', () {
      const base = 'http://127.0.0.1:4000/tok';
      const rel = 'Фильмы/Dune (2021)/Dune #1?.mkv';
      final url = RustLibrary.streamUrl(base, 3, rel);
      expect(RustLibrary.parseStreamUrl(base, url), (3, rel));
      expect(RustLibrary.parseStreamUrl(base, '$base/x/a.mkv'), isNull);
      expect(RustLibrary.parseStreamUrl(base, '$base/3'), isNull);
      expect(RustLibrary.parseStreamUrl(base, 'http://other/tok/3/a'), isNull);
    });
  });

  group('sidecar subtitles', () {
    late Directory tmp;
    late String root;
    late RustLibrary lib;

    setUpAll(() async {
      await RustLibrary.ensureInitialized(
        externalLibrary: openBuiltCoreLibrary(),
      );
    });

    setUp(() async {
      tmp = Directory.systemTemp.createTempSync('pa_subs_');
      root = p.join(tmp.path, 'media');
      media(root, 'Movies/Arrival (2016)/Arrival.2016.mkv', 1);
      subtitle(root, 'Movies/Arrival (2016)/Arrival.2016.ru.forced.srt');
      subtitle(root, 'Movies/Arrival (2016)/Arrival.2016.en.srt');
      media(root, 'TV/Dark/Season 1/Dark.S01E01.mkv', 2);
      media(root, 'TV/Dark/Season 1/Dark.S01E02.mkv', 3);
      subtitle(root, 'TV/Dark/Season 1/Dark.S01E02.eng.sdh.srt');
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

    test('of a movie: local paths with language and flags', () async {
      final movie = (await lib.movies().first).single;
      final subs = await lib.sidecarSubtitles(movie);
      expect(subs, [
        SidecarSubtitle(
          uri: p.join(root, 'Movies/Arrival (2016)/Arrival.2016.en.srt'),
          name: 'Arrival.2016.en.srt',
          language: 'eng',
        ),
        SidecarSubtitle(
          uri: p.join(root, 'Movies/Arrival (2016)/Arrival.2016.ru.forced.srt'),
          name: 'Arrival.2016.ru.forced.srt',
          language: 'rus',
          forced: true,
        ),
      ]);
    });

    test('of episodes: only those named after the episode', () async {
      final show = (await lib.shows().first).single;
      final episodes = await lib.episodes(show.id).first;
      final e1 = episodes.firstWhere((e) => e.episode == 1);
      final e2 = episodes.firstWhere((e) => e.episode == 2);
      expect(await lib.sidecarSubtitles(e1), isEmpty);
      final subs = await lib.sidecarSubtitles(e2);
      expect(subs.single.name, 'Dark.S01E02.eng.sdh.srt');
      expect(subs.single.hearingImpaired, isTrue);
    });

    test('files outside the library have none', () async {
      final movie = (await lib.movies().first).single;
      final outside = movie.withFile(p.join(tmp.path, 'elsewhere.mkv'));
      expect(await lib.sidecarSubtitles(outside), isEmpty);
    });
  });
}
