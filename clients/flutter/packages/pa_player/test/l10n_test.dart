// SPDX-License-Identifier: MPL-2.0
//
// UI-023: the player's own strings (ru/en) follow the app locale, and the
// video OSD fits a phone in portrait without overflow.
import 'package:flutter/material.dart';
import 'package:flutter_localizations/flutter_localizations.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/testing.dart';

Widget _app(Widget home, {Locale? locale, bool delegate = true}) => MaterialApp(
  locale: locale,
  supportedLocales: const [Locale('en'), Locale('ru'), Locale('de')],
  localizationsDelegates: [
    if (delegate) PaPlayerStrings.delegate,
    ...GlobalMaterialLocalizations.delegates,
  ],
  home: home,
);

PlayerScreen _player(FakePlayerEngine engine) => PlayerScreen(
  request: const PlayRequest(
    uri: '/media/movie.mkv',
    title: 'Очень длинное название фильма, которое не помещается в строку',
  ),
  onProgress: (_, _) {},
  engineFactory: ({bool video = true}) => engine,
);

void _surface(WidgetTester t, Size size) {
  t.view.physicalSize = size * 2;
  t.view.devicePixelRatio = 2;
  addTearDown(t.view.reset);
}

void main() {
  test('every string is translated', () {
    const en = PaPlayerStrings.en;
    const ru = PaPlayerStrings.ru;
    final pairs = <(String, String)>[
      (en.back, ru.back),
      (en.back10, ru.back10),
      (en.forward10, ru.forward10),
      (en.play, ru.play),
      (en.pause, ru.pause),
      (en.audio, ru.audio),
      (en.subtitles, ru.subtitles),
      (en.subtitleFile, ru.subtitleFile),
      (en.speed, ru.speed),
      (en.playbackSpeed, ru.playbackSpeed),
      (en.info, ru.info),
      (en.fullscreen, ru.fullscreen),
      (en.close, ru.close),
      (en.noAudioTracks, ru.noAudioTracks),
      (en.off, ru.off),
      (en.externalFile, ru.externalFile),
      (en.loadFromFile, ru.loadFromFile),
      (en.normalSpeed, ru.normalSpeed),
      (en.infoTitle, ru.infoTitle),
      (en.infoPosition, ru.infoPosition),
      (en.cannotOpenFolder, ru.cannotOpenFolder),
      (en.noSubtitleFiles, ru.noSubtitleFiles),
      (en.next, ru.next),
      (en.previous, ru.previous),
      (en.nowPlaying, ru.nowPlaying),
      (en.nothingPlaying, ru.nothingPlaying),
      (en.shuffleOn, ru.shuffleOn),
      (en.shuffleOff, ru.shuffleOff),
      (en.repeatOff, ru.repeatOff),
      (en.repeatAll, ru.repeatAll),
      (en.repeatOne, ru.repeatOne),
      (en.upNext, ru.upNext),
      (en.endOfQueue, ru.endOfQueue),
      (en.playNow, ru.playNow),
      (en.cancel, ru.cancel),
      (en.upNextIn(7), ru.upNextIn(7)),
      (
        en.remainingEndsAt('1:00', '21:40'),
        ru.remainingEndsAt('1:00', '21:40'),
      ),
      (
        en.trackLabel(const MediaTrack(id: '2')),
        ru.trackLabel(const MediaTrack(id: '2')),
      ),
      (en.engineInfoKey('Resolution'), ru.engineInfoKey('Resolution')),
    ];
    for (final (e, r) in pairs) {
      expect(e, isNotEmpty);
      expect(r, isNotEmpty);
      expect(r, isNot(e), reason: '"$e" is not translated');
    }
    expect(ru.trackLabel(MediaTrack.off), 'Выкл.');
    expect(ru.engineInfoKey('Unknown key'), 'Unknown key');
  });

  test('forLocale picks by language and falls back to English', () {
    expect(
      PaPlayerStrings.forLocale(const Locale('ru', 'RU')).languageCode,
      'ru',
    );
    expect(
      PaPlayerStrings.forLocale(const Locale('en', 'XA')).languageCode,
      'en',
    );
    expect(PaPlayerStrings.forLocale(const Locale('de')).languageCode, 'en');
    expect(PaPlayerStrings.forLocale(null).languageCode, 'en');
  });

  for (final delegate in [true, false]) {
    testWidgets('PlayerScreen in Russian (delegate: $delegate)', (t) async {
      final engine = FakePlayerEngine();
      await t.pumpWidget(
        _app(_player(engine), locale: const Locale('ru'), delegate: delegate),
      );
      await t.pump(const Duration(milliseconds: 300));
      expect(find.byTooltip('Пауза'), findsOneWidget);
      expect(find.byTooltip('Назад на 10 с'), findsOneWidget);
      expect(find.byTooltip('Субтитры'), findsOneWidget);
      expect(find.byTooltip('Pause'), findsNothing);
      expect(find.textContaining('закончится в'), findsOneWidget);

      await t.tap(find.byKey(const ValueKey('osd-subtitles')));
      await t.pumpAndSettle();
      expect(find.text('Выкл.'), findsOneWidget);
      expect(find.text('Загрузить из файла…'), findsOneWidget);
      await t.pumpWidget(const SizedBox());
    });
  }

  testWidgets('unsupported locale falls back to English', (t) async {
    final engine = FakePlayerEngine();
    await t.pumpWidget(_app(_player(engine), locale: const Locale('de')));
    await t.pump(const Duration(milliseconds: 300));
    expect(find.byTooltip('Pause'), findsOneWidget);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('NowPlayingBar and NowPlayingScreen in Russian', (t) async {
    _surface(t, const Size(1600, 900)); // wide layout: queue on screen
    final engine = FakePlayerEngine();
    final c = MusicPlayerController(
      engineFactory: ({bool video = true}) => engine,
    );
    await c.play(const [
      PlayRequest(uri: '/m/1.flac', title: 'One'),
      PlayRequest(uri: '/m/2.flac', title: 'Two'),
    ]);
    await t.pumpWidget(
      _app(
        Scaffold(bottomNavigationBar: NowPlayingBar(controller: c)),
        locale: const Locale('ru'),
      ),
    );
    await t.pump();
    expect(find.byTooltip('Пауза'), findsOneWidget);
    expect(find.byTooltip('Следующий'), findsOneWidget);

    await t.tap(find.byKey(const ValueKey('now-playing-bar')));
    await t.pumpAndSettle();
    expect(find.byType(NowPlayingScreen), findsOneWidget);
    expect(find.text('Сейчас играет'), findsOneWidget);
    expect(find.text('Далее'), findsOneWidget);
    expect(find.byTooltip('Повтор выключен'), findsOneWidget);
    await t.pumpWidget(const SizedBox());
    c.dispose();
  });

  // Phones in portrait: the bottom bar used to overflow by ~150 px.
  for (final size in const [Size(360, 740), Size(390, 844), Size(412, 915)]) {
    for (final locale in const [Locale('en'), Locale('ru')]) {
      testWidgets(
        'OSD fits a ${size.width.toInt()}x${size.height.toInt()} phone '
        'in portrait ($locale)',
        (t) async {
          _surface(t, size);
          final engine = FakePlayerEngine(
            loadedDuration: const Duration(hours: 2, minutes: 15),
          );
          await t.pumpWidget(_app(_player(engine), locale: locale));
          await t.pump(const Duration(milliseconds: 300));
          expect(t.takeException(), isNull);
          expect(find.byKey(const ValueKey('osd-transport')), findsOneWidget);
          expect(find.byKey(const ValueKey('osd-panels')), findsOneWidget);
          for (final key in [
            'osd-back10',
            'osd-play',
            'osd-fwd10',
            'osd-audio',
            'osd-subtitles',
            'osd-speed',
            'osd-info',
          ]) {
            final r = t.getRect(find.byKey(ValueKey(key)));
            expect(r.left, greaterThanOrEqualTo(0), reason: key);
            expect(r.right, lessThanOrEqualTo(size.width), reason: key);
            expect(r.bottom, lessThanOrEqualTo(size.height), reason: key);
          }

          // Side panels fit as well.
          await t.tap(find.byKey(const ValueKey('osd-info')));
          await t.pumpAndSettle();
          expect(t.takeException(), isNull);
          final panel = t.getRect(find.byKey(const ValueKey('panel-info')));
          expect(panel.left, greaterThanOrEqualTo(0));
          await t.pumpWidget(const SizedBox());
        },
      );
    }
  }

  testWidgets('wide windows keep the single-row bar', (t) async {
    _surface(t, const Size(1280, 720));
    final engine = FakePlayerEngine();
    await t.pumpWidget(_app(_player(engine)));
    await t.pump(const Duration(milliseconds: 300));
    expect(find.byKey(const ValueKey('osd-transport')), findsNothing);
    final play = t.getRect(find.byKey(const ValueKey('osd-play')));
    final info = t.getRect(find.byKey(const ValueKey('osd-info')));
    expect(play.center.dy, closeTo(info.center.dy, 1));
    await t.pumpWidget(const SizedBox());
  });
}
