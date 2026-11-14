// SPDX-License-Identifier: MPL-2.0
//
// Music library (MUS-004): golden images of the music Home, the Library
// (albums), an album page (two discs, FLAC 24/96) and an artist page on the
// four densities, plus the chips, discs and play counting. Like
// test/home_golden_test.dart they use the default test font. Refresh
// deliberately:
//   flutter test --update-goldens test/music_golden_test.dart

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

Future<void> _tap(WidgetTester t, Finder f) async {
  await t.tap(f.first);
  await settle(t, 10);
}

Future<void> _openLibrary(WidgetTester t) =>
    _tap(t, find.byKey(const ValueKey('music-tab-library')));

final _flows = <String, Future<void> Function(WidgetTester)>{
  'home': (_) async {},
  'library': _openLibrary,
  'album': (t) => _tap(t, find.text('Nocturne Avenue')),
  'artist': (t) async {
    await _openLibrary(t);
    await _tap(t, find.byKey(const ValueKey('music-section-artists')));
    await _tap(t, find.text('Mara Quill'));
  },
};

void main() {
  final densities = <(String, FormFactor, Size)>[
    ('phone', FormFactor.phone, const Size(390, 844)),
    ('tablet', FormFactor.phone, const Size(820, 1180)),
    ('desktop', FormFactor.desktop, const Size(1280, 800)),
    ('tv', FormFactor.tv, const Size(960, 540)),
  ];
  for (final (name, ff, size) in densities) {
    for (final MapEntry(key: screen, value: flow) in _flows.entries) {
      testWidgets('music $screen on $name', (t) async {
        setSurface(t, size);
        final services = await demoServices();
        await t.pumpWidget(
          testApp(services, formFactor: ff, section: Section.music),
        );
        await settle(t);
        await flow(t);
        await expectLater(
          find.byKey(screenshotKey),
          matchesGoldenFile('goldens/music_${screen}_$name.png'),
        );
        await t.pumpWidget(const SizedBox());
        await settle(t, 2);
      });
    }
  }

  testWidgets('chips filter the albums', (t) async {
    setSurface(t, const Size(1280, 800));
    final services = await demoServices();
    await t.pumpWidget(testApp(services, section: Section.music));
    await settle(t);
    await _openLibrary(t);
    expect(find.text('Paper Planets'), findsWidgets);
    expect(find.text('Tides & Tunnels'), findsWidgets);

    // Hi-Res: more than 48 kHz or more than 16 bits.
    await _tap(t, find.byKey(const ValueKey('music-chip-hiRes')));
    expect(find.text('Nocturne Avenue'), findsOneWidget);
    expect(find.text('Tides & Tunnels'), findsOneWidget);
    expect(find.text('Analog Dawn'), findsNothing);
    expect(find.text('Paper Planets'), findsNothing);
    await _tap(t, find.byKey(const ValueKey('music-chip-hiRes')));

    // Lossless leaves out the MP3 album; Downloaded the network one.
    await _tap(t, find.byKey(const ValueKey('music-chip-lossless')));
    expect(find.text('Paper Planets'), findsNothing);
    expect(find.text('Analog Dawn'), findsOneWidget);
    await _tap(t, find.byKey(const ValueKey('music-chip-downloaded')));
    expect(find.text('Tides & Tunnels'), findsNothing);
    expect(find.text('Quiet Machines'), findsOneWidget);

    // Genres and composers come from the tags.
    await _tap(t, find.byKey(const ValueKey('music-section-genres')));
    expect(find.text('Ambient'), findsOneWidget);
    expect(find.text('Folk'), findsNothing, reason: 'filtered out');
    await _tap(t, find.byKey(const ValueKey('music-section-composers')));
    expect(find.text('E. Marlowe'), findsOneWidget);
  });

  testWidgets('album page: format badge, discs; playing counts', (t) async {
    setSurface(t, const Size(1280, 800));
    final services = await demoServices();
    final lib = services.library as FakeLibrary;
    await t.pumpWidget(testApp(services, section: Section.music));
    await settle(t);
    await _tap(t, find.text('Nocturne Avenue'));
    expect(find.text('FLAC 24/96'), findsOneWidget);
    expect(find.text('Disc 1'), findsOneWidget);
    expect(find.text('Disc 2'), findsOneWidget);

    final before = await lib.musicTracks().first;
    final first = before.firstWhere(
      (x) =>
          x.item.album == 'Nocturne Avenue' &&
          x.discNumber == 1 &&
          x.item.trackNumber == 1,
    );
    await _tap(t, find.text('Play'));
    final after = await lib.musicTracks().first;
    expect(
      after.firstWhere((x) => x.id == first.id).playCount,
      first.playCount + 1,
    );
    services.music.pause();
    await t.pumpWidget(const SizedBox());
    await settle(t, 2);
  });
}
