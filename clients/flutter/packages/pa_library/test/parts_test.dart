// SPDX-License-Identifier: MPL-2.0
// LIB-020 through RustLibrary on the real native core: a split movie and a
// DVD title set give all their parts in order, a disc image is marked
// unplayable.

import 'dart:io';

import 'package:pa_library/pa_library.dart';
import 'package:path/path.dart' as p;
import 'package:test/test.dart';

import 'native_library.dart';

void media(String root, String rel, int size, int seed) {
  final f = File(p.join(root, rel));
  f.parent.createSync(recursive: true);
  f.writeAsBytesSync(List.generate(size, (i) => (i * 31 + seed) & 0xff));
}

const cd1 = 'Movies/Heat (1995)/Heat.1995.DVDRip.cd1.avi';
const cd2 = 'Movies/Heat (1995)/Heat.1995.DVDRip.cd2.avi';
const dvd = 'Movies/Alien (1979)/VIDEO_TS';
const iso = 'Movies/Spirited Away (2001).iso';
const one = 'Movies/Arrival (2016)/Arrival.2016.1080p.mkv';

void main() {
  late Directory tmp;
  late String root;
  late RustLibrary lib;

  setUpAll(() async {
    await RustLibrary.ensureInitialized(externalLibrary: openBuiltCoreLibrary());
  });

  setUp(() async {
    tmp = Directory.systemTemp.createTempSync('pa_parts_');
    root = p.join(tmp.path, 'media');
    media(root, cd1, 120000, 1);
    media(root, cd2, 110000, 2);
    media(root, '$dvd/VIDEO_TS.VOB', 9000, 3);
    media(root, '$dvd/VTS_01_0.VOB', 5000, 4);
    media(root, '$dvd/VTS_01_1.VOB', 160000, 5);
    media(root, '$dvd/VTS_01_2.VOB', 90000, 6);
    media(root, iso, 70000, 7);
    media(root, one, 150000, 8);
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

  Future<LibraryItem> movie(String title) async =>
      (await lib.movies().first).firstWhere((m) => m.title == title);

  test('a cd1/cd2 movie plays both parts in order', () async {
    final heat = await movie('Heat');
    expect(heat.unplayable, isNull);
    expect(await lib.playParts(heat), [p.join(root, cd1), p.join(root, cd2)]);
  });

  test('a DVD folder plays the VOBs of its main title set', () async {
    final alien = await movie('Alien');
    expect(await lib.playParts(alien), [
      p.join(root, '$dvd/VTS_01_1.VOB'),
      p.join(root, '$dvd/VTS_01_2.VOB'),
    ]);
  });

  test('a single file plays alone; unknown files have no parts', () async {
    final arrival = await movie('Arrival');
    expect(await lib.playParts(arrival), [p.join(root, one)]);
    final outside = arrival.withFile(p.join(tmp.path, 'elsewhere.mkv'));
    expect(await lib.playParts(outside), isEmpty);
  });

  test('a disc image is unplayable, with its versions', () async {
    final image = await movie('Spirited Away');
    expect(image.unplayable, Unplayable.discImage);
    final versions = await lib.versions(image.id);
    expect(versions.single.unplayable, Unplayable.discImage);
  });

  test('unplayable codes', () {
    expect(Unplayable.fromCode(null), isNull);
    expect(Unplayable.fromCode(''), isNull);
    expect(Unplayable.fromCode('disc_image'), Unplayable.discImage);
    expect(Unplayable.fromCode('from_the_future'), Unplayable.other);
  });
}
