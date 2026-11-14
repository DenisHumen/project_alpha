// SPDX-License-Identifier: MPL-2.0
// UI-009 through RustLibrary on the real native core: extras and the
// clearlogo next to a film, and what a library without metadata or probe
// data returns.

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

void main() {
  late Directory tmp;
  late String root;
  late RustLibrary lib;

  setUpAll(() async {
    await RustLibrary.ensureInitialized(
      externalLibrary: openBuiltCoreLibrary(),
    );
  });

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('pa_details_');
    root = p.join(tmp.path, 'media');
    media(root, 'Movies/Arrival (2016)/Arrival.2016.mkv', 1);
    media(root, 'Movies/Arrival (2016)/Trailers/Arrival Trailer.mkv', 2);
    File(
      p.join(root, 'Movies/Arrival (2016)/clearlogo.png'),
    ).writeAsBytesSync([1, 2, 3]);
    media(root, 'TV/Dark/Season 1/Dark.S01E01.mkv', 3);
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

  test('a movie has its extras and local clearlogo', () async {
    final movies = await lib.movies().first;
    expect([for (final m in movies) m.title], ['Arrival']);
    final d = await lib.details(movies.single.id);
    expect(d.extras, hasLength(1));
    final trailer = d.extras.single;
    expect(trailer.kind, MediaExtraKind.trailer);
    expect(trailer.title, 'Arrival Trailer');
    expect(
      trailer.playbackUri,
      p.join(root, 'Movies/Arrival (2016)/Trailers/Arrival Trailer.mkv'),
    );
    expect(
      d.logoUrl,
      Uri.file(p.join(root, 'Movies/Arrival (2016)/clearlogo.png')).toString(),
    );
    // No provider metadata here: no people or ratings.
    expect(d.credits, isEmpty);
    expect(d.ratings, isEmpty);
    // The scan read no tracks without FFmpeg, but the file is known.
    expect(d.tech, isNotNull);
    expect(
      d.tech!.filePath,
      p.join(root, 'Movies/Arrival (2016)/Arrival.2016.mkv'),
    );
    expect(d.tech!.sizeBytes, 150000);
  });

  test('a show has no file data; a bad id throws', () async {
    final show = (await lib.shows().first).single;
    final d = await lib.details(show.id);
    expect(d.tech, isNull);
    expect(d.extras, isEmpty);
    await expectLater(lib.details('nope'), throwsA(anything));
  });
}
