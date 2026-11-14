// SPDX-License-Identifier: MPL-2.0
//
// Movie Detail (UI-009): golden images on the four densities, initial focus
// on Play/Resume, technical badges as text, the version picker on Play,
// hidden rows without data and the sections below the hero. The goldens
// use the default test font, like test/home_golden_test.dart. Refresh
// deliberately:
//   flutter test --update-goldens test/movie_detail_test.dart

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/integration/player.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/screens/detail_sections.dart';
import 'package:kinolune/screens/movie_detail_screen.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/widgets/focusable.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pa_player/testing.dart';

import 'support/test_app.dart';

final _now = DateTime(2026, 11, 14, 20);

const _hd = '/media/Movies/Arrival (2016)/Arrival.2016.1080p.mkv';
const _uhd = '/media/Movies/Arrival (2016)/Arrival.2016.2160p.DV.mkv';

final _arrival = LibraryItem(
  id: 'm-arrival',
  kind: ItemKind.movie,
  title: 'Arrival',
  year: 2016,
  overview: 'A linguist is recruited to talk to visitors.',
  filePath: _uhd,
  durationMs: 116 * 60000,
  addedAt: _now,
  matchConfidence: .97,
  versionCount: 2,
);

final _plain = LibraryItem(
  id: 'm-plain',
  kind: ItemKind.movie,
  title: 'Plain Film',
  year: 1999,
  filePath: '/media/Movies/plain.avi',
  addedAt: _now,
);

const _arrivalDetails = ItemDetails(
  genres: ['Drama', 'Sci-Fi'],
  runtimeMs: 116 * 60000,
  ratings: [ExternalRating(source: 'tmdb', value: 7.6, votes: 18000)],
  credits: [
    CreditPerson(name: 'Ann Example', character: 'Louise'),
    CreditPerson(name: 'Bob Example', character: 'Ian'),
    CreditPerson(name: 'Carl Example', crew: true, job: 'Director'),
  ],
  extras: [
    MediaExtra(
      title: 'Arrival Trailer',
      kind: MediaExtraKind.trailer,
      filePath: '/media/Movies/Arrival (2016)/Trailers/Arrival Trailer.mkv',
      durationMs: 150000,
    ),
  ],
  tech: TechInfo(
    container: 'matroska,webm',
    sizeBytes: 48000000000,
    durationMs: 116 * 60000,
    videoCodec: 'hevc',
    videoProfile: 'Main 10',
    width: 3840,
    height: 1600,
    bitDepth: 10,
    frameRate: 23.976,
    hdr: [HdrFormat.dolbyVision, HdrFormat.hdr10],
    audioFormats: ['truehd', 'atmos', 'ac3'],
    audioLanguages: ['eng'],
    subtitleLanguages: ['eng', 'rus'],
    filePath: _uhd,
  ),
);

Future<AppServices> _services({
  Map<String, ItemDetails> details = const {},
  List<Progress> progress = const [],
}) async {
  final lib = FakeLibrary.items(
    [_arrival, _plain],
    progress: progress,
    details: details,
    versions: {
      'm-arrival': const [
        MediaVersion(
          id: 'v-uhd',
          label: '4K · DV',
          resolution: '4K',
          hdr: 'DV',
          filePath: _uhd,
        ),
        MediaVersion(
          id: 'v-hd',
          label: '1080p',
          resolution: '1080p',
          filePath: _hd,
        ),
      ],
    },
  );
  final s = AppServices(
    library: lib,
    secureStore: MemorySecureStore(),
    playerEngineFactory: ({bool video = true}) => FakePlayerEngine(),
    clock: () => _now,
  );
  await s.init();
  return s;
}

Future<void> _open(
  WidgetTester t,
  AppServices s,
  String title, {
  FormFactor ff = FormFactor.desktop,
  Size size = const Size(1280, 800),
}) async {
  setSurface(t, size);
  await t.pumpWidget(testApp(s, formFactor: ff, section: Section.movies));
  await settle(t, 10);
  await t.tap(find.text(title).first);
  await settle(t, 10);
}

Future<void> _close(WidgetTester t) async {
  await t.pumpWidget(const SizedBox());
  await settle(t, 2);
}

/// The button that has the primary focus, if any.
KButton? _focusedButton() => FocusManager.instance.primaryFocus?.context
    ?.findAncestorWidgetOfExactType<KButton>();

void main() {
  test('badges are text: resolution, HDR and audio, no logos', () {
    expect(techBadges(_arrivalDetails.tech, null), [
      '4K',
      'Dolby Vision',
      'Atmos',
      'TrueHD',
    ]);
    expect(
      techBadges(
        const TechInfo(width: 1920, height: 1080, audioFormats: ['eac3']),
        null,
      ),
      ['1080p', 'DD+'],
    );
    // Not probed yet: what the release name says.
    expect(
      techBadges(
        null,
        const MediaVersion(
          id: 'v',
          label: '4K · DV',
          resolution: '4K',
          hdr: 'DV',
          filePath: _uhd,
        ),
      ),
      ['4K', 'Dolby Vision'],
    );
    expect(techBadges(null, null), isEmpty);
  });

  group('golden', () {
    final densities = <(String, FormFactor, Size)>[
      ('phone', FormFactor.phone, const Size(390, 844)),
      ('tablet', FormFactor.phone, const Size(820, 1180)),
      ('desktop', FormFactor.desktop, const Size(1280, 800)),
      ('tv', FormFactor.tv, const Size(960, 540)),
    ];
    for (final (name, ff, size) in densities) {
      testWidgets('movie detail on $name', (t) async {
        final s = await _services(
          details: {'m-arrival': _arrivalDetails},
          progress: [
            Progress(
              itemId: 'm-arrival',
              positionMs: 5025000,
              durationMs: 116 * 60000,
              updatedAt: _now.subtract(const Duration(minutes: 5)),
            ),
          ],
        );
        await _open(t, s, 'Arrival', ff: ff, size: size);
        expect(find.byType(MovieDetailScreen), findsOneWidget);
        await expectLater(
          find.byKey(screenshotKey),
          matchesGoldenFile('goldens/movie_detail_$name.png'),
        );
        await _close(t);
      });
    }
  });

  for (final (name, ff, size) in const [
    ('desktop', FormFactor.desktop, Size(1280, 800)),
    ('tv', FormFactor.tv, Size(960, 540)),
  ]) {
    testWidgets('initial focus is on Play/Resume on $name', (t) async {
      final s = await _services(
        details: {'m-arrival': _arrivalDetails},
        progress: [
          Progress(
            itemId: 'm-arrival',
            positionMs: 5025000,
            durationMs: 116 * 60000,
            updatedAt: _now,
          ),
        ],
      );
      await _open(t, s, 'Arrival', ff: ff, size: size);
      expect(_focusedButton()?.key, detailPlayKey);
      expect(find.text('Resume 1:23:45'), findsOneWidget);
      await _close(t);
    });
  }

  testWidgets('initial focus is on Play without details (v0 data)', (t) async {
    final s = await _services();
    await _open(
      t,
      s,
      'Plain Film',
      ff: FormFactor.tv,
      size: const Size(960, 540),
    );
    expect(_focusedButton()?.key, detailPlayKey);
    expect(find.text('Play'), findsOneWidget);
    await _close(t);
  });

  testWidgets('hero: badges, ratings, resume line, trailer', (t) async {
    final s = await _services(
      details: {'m-arrival': _arrivalDetails},
      progress: [
        Progress(
          itemId: 'm-arrival',
          positionMs: 5025000,
          durationMs: 116 * 60000,
          updatedAt: _now.subtract(const Duration(minutes: 5)),
        ),
      ],
    );
    await _open(t, s, 'Arrival');
    for (final b in ['4K', 'Dolby Vision', 'Atmos', 'TrueHD']) {
      expect(
        find.descendant(of: find.byType(TechBadge), matching: find.text(b)),
        findsOneWidget,
        reason: b,
      );
    }
    expect(
      find.descendant(of: find.byType(TechBadge), matching: find.byType(Image)),
      findsNothing,
      reason: 'badges are text, no logos',
    );
    expect(find.text('TMDB 7.6'), findsOneWidget);
    expect(find.text('Stopped at 1:23:45 · 5 minutes ago'), findsOneWidget);
    expect(find.textContaining('Drama, Sci-Fi'), findsOneWidget);
    expect(find.byKey(const ValueKey('detail-trailer')), findsOneWidget);
    expect(find.byKey(const ValueKey('detail-favorite')), findsOneWidget);

    // Sections below the hero.
    await t.scrollUntilVisible(
      find.byKey(const ValueKey('detail-tech')),
      300,
      scrollable: find
          .descendant(
            of: find.byType(MovieDetailScreen),
            matching: find.byType(Scrollable),
          )
          .first,
    );
    await settle(t, 4);
    expect(find.text('Cast & Crew'), findsOneWidget);
    expect(find.text('Ann Example'), findsOneWidget);
    expect(find.text('Louise'), findsOneWidget);
    expect(find.text('Director'), findsOneWidget);
    expect(find.text('Arrival Trailer'), findsOneWidget);
    expect(find.text('Technical info'), findsOneWidget);
    expect(find.text('48.0 GB'), findsOneWidget);
    expect(
      find.textContaining('HEVC Main 10  ·  3840×1600  ·  10-bit'),
      findsOneWidget,
    );
    expect(find.text('ENG, RUS'), findsOneWidget);
    await _close(t);
  });

  testWidgets('without details the extra rows are hidden', (t) async {
    final s = await _services();
    await _open(t, s, 'Plain Film');
    expect(find.byKey(const ValueKey('detail-ratings')), findsNothing);
    expect(find.byType(TechBadge), findsNothing);
    expect(find.byKey(const ValueKey('detail-trailer')), findsNothing);
    expect(find.text('Cast & Crew'), findsNothing);
    expect(find.text('Extras'), findsNothing);
    expect(find.byKey(const ValueKey('detail-title')), findsOneWidget);
    await t.scrollUntilVisible(
      find.byKey(const ValueKey('detail-tech')),
      300,
      scrollable: find
          .descendant(
            of: find.byType(MovieDetailScreen),
            matching: find.byType(Scrollable),
          )
          .first,
    );
    await settle(t, 4);
    expect(find.text('/media/Movies/plain.avi'), findsOneWidget);
    await _close(t);
  });

  testWidgets('Play with two versions opens the version picker', (t) async {
    final s = await _services(details: {'m-arrival': _arrivalDetails});
    await _open(t, s, 'Arrival');
    await t.tap(find.byKey(detailPlayKey));
    await settle(t, 6);
    expect(find.text('Choose a version'), findsOneWidget);
    await t.tap(find.byKey(const ValueKey('version-v-hd')));
    await settle(t, 6);
    final player = t.widget<PlayerScreen>(find.byType(PlayerScreen));
    expect(player.request.uri, _hd);
    await _close(t);
  });

  testWidgets('demo collection row opens the other film', (t) async {
    final s = await demoServices();
    setSurface(t, const Size(1280, 800));
    await t.pumpWidget(testApp(s, section: Section.movies));
    await settle(t, 10);
    await t.tap(find.text('Harbor Lights').first);
    await settle(t, 10);
    await t.scrollUntilVisible(
      find.byKey(const ValueKey('detail-collection')),
      300,
      scrollable: find
          .descendant(
            of: find.byType(MovieDetailScreen),
            matching: find.byType(Scrollable),
          )
          .first,
    );
    await settle(t, 4);
    expect(find.text('Part of Coastal Tales'), findsOneWidget);
    await t.tap(
      find.descendant(
        of: find.byKey(const ValueKey('detail-collection')),
        matching: find.text('Low Tide'),
      ),
    );
    await settle(t, 10);
    expect(
      t.widget<Text>(find.byKey(const ValueKey('detail-title'))).data,
      'Low Tide',
    );
    await _close(t);
  });
}
