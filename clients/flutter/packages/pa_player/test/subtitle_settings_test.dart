// SPDX-License-Identifier: MPL-2.0
//
// PLAY-007: subtitle settings — storage, system caption style, the look on
// screen, the series memory and the delay label.
import 'dart:ui';

import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';

void main() {
  group('SubtitleSettings', () {
    test('defaults follow the system style', () {
      const s = SubtitleSettings();
      expect(s.useSystemStyle, isTrue);
      expect(s.scale, 1.0);
      expect(s.font, SubtitleFont.sansSerif);
      expect(s.background, SubtitleBackground.none);
      expect(s.position, SubtitleSettings.defaultPosition);
    });

    test('JSON round trip', () {
      const s = SubtitleSettings(
        useSystemStyle: false,
        scale: 1.5,
        font: SubtitleFont.monospace,
        textColor: Color(0xFFFFFF00),
        background: SubtitleBackground.translucent,
        position: 0.2,
      );
      expect(SubtitleSettings.fromJson(s.toJson()), s);
    });

    test('invalid or missing JSON values get defaults', () {
      expect(SubtitleSettings.fromJson(null), const SubtitleSettings());
      expect(SubtitleSettings.fromJson('x'), const SubtitleSettings());
      final s = SubtitleSettings.fromJson({
        'system': 'yes',
        'scale': -2,
        'font': 'comic',
        'color': 'red',
        'background': 'glass',
        'position': 9,
      });
      expect(s.useSystemStyle, isTrue);
      expect(s.scale, 1.0);
      expect(s.font, SubtitleFont.sansSerif);
      expect(s.textColor, const Color(0xFFFFFFFF));
      expect(s.background, SubtitleBackground.none);
      expect(s.position, closeTo(SubtitleSettings.maxPosition, 1e-9));
    });

    test('position is clamped and snapped to the step', () {
      expect(
        const SubtitleSettings().copyWith(position: -1).position,
        SubtitleSettings.minPosition,
      );
      expect(
        const SubtitleSettings().copyWith(position: 0.131).position,
        closeTo(0.14, 1e-9),
      );
    });

    test('system style on, system captions enabled: the system look', () {
      const system = SystemCaptionStyle(
        enabled: true,
        fontScale: 1.5,
        foregroundColor: 0xFFFFFF00,
        backgroundColor: 0xFF0000FF,
        edgeType: 2,
        edgeColor: 0xFF000000,
        typeface: 'serif',
      );
      final a = const SubtitleSettings(position: 0.2).resolve(system);
      expect(a.fontScale, 1.5);
      expect(a.textColor, const Color(0xFFFFFF00));
      expect(a.backgroundColor, const Color(0xFF0000FF));
      expect(a.edge, SubtitleEdge.dropShadow);
      expect(a.font, SubtitleFont.serif);
      expect(a.bottomMargin, 0.2, reason: 'the position stays the user one');
    });

    test('system style on without a system style (desktop): standard', () {
      const settings = SubtitleSettings(
        scale: 2,
        textColor: Color(0xFF00FF00),
        position: 0.1,
      );
      expect(
        settings.resolve(null),
        SubtitleAppearance.standard.copyWith(bottomMargin: 0.1),
      );
    });

    test('system captions switched off: standard, as Android players do', () {
      const off = SystemCaptionStyle(
        enabled: false,
        fontScale: 2,
        foregroundColor: 0xFFFFFF00,
      );
      expect(
        const SubtitleSettings().resolve(off),
        SubtitleAppearance.standard,
      );
    });

    test('custom style ignores the system one', () {
      const system = SystemCaptionStyle(
        enabled: true,
        foregroundColor: 0xFFFFFF00,
      );
      const settings = SubtitleSettings(
        useSystemStyle: false,
        scale: 0.75,
        font: SubtitleFont.casual,
        textColor: Color(0xFF00FFFF),
        background: SubtitleBackground.opaque,
        position: 0.3,
      );
      final a = settings.resolve(system);
      expect(a.fontScale, 0.75);
      expect(a.font, SubtitleFont.casual);
      expect(a.textColor, const Color(0xFF00FFFF));
      expect(a.backgroundColor, const Color(0xFF000000));
      expect(a.edge, SubtitleEdge.none);
      expect(a.bottomMargin, 0.3);

      final outlined = settings
          .copyWith(background: SubtitleBackground.none)
          .resolve(system);
      expect(outlined.edge, SubtitleEdge.outline);
      expect(outlined.backgroundColor.a, 0);
      final box = settings
          .copyWith(background: SubtitleBackground.translucent)
          .resolve(system);
      expect(box.backgroundColor.a, greaterThan(0));
      expect(box.backgroundColor.a, lessThan(1));
    });
  });

  group('SystemCaptionStyle', () {
    test('reads the Android channel map (signed ARGB ints)', () {
      final s = SystemCaptionStyle.fromMap({
        'enabled': true,
        'fontScale': 2.0,
        'foregroundColor': -256, // 0xFFFFFF00 as a Java int
        'backgroundColor': -16777216, // 0xFF000000
        'edgeType': 1,
        'edgeColor': -16777216,
        'typeface': 'monospace',
        'locale': 'ru-RU',
      });
      expect(s.enabled, isTrue);
      expect(s.fontScale, 2.0);
      expect(s.foregroundColor, 0xFFFFFF00);
      expect(s.backgroundColor, 0xFF000000);
      expect(s.edgeType, 1);
      expect(s.typeface, 'monospace');
      expect(s.locale, 'ru-RU');
      expect(s.windowColor, isNull);
      final a = s.appearance();
      expect(a.font, SubtitleFont.monospace);
      expect(a.edge, SubtitleEdge.outline);
      expect(a.windowColor, SubtitleAppearance.standard.windowColor);
    });

    test('missing and bad values', () {
      final s = SystemCaptionStyle.fromMap({
        'fontScale': 0,
        'typeface': '',
        'edgeType': -1,
      });
      expect(s.enabled, isFalse);
      expect(s.fontScale, 1.0);
      expect(s.typeface, isNull);
      expect(s.appearance().edge, SubtitleAppearance.standard.edge);
    });

    test('Android typeface names', () {
      expect(SystemCaptionStyle.fontOf(null), SubtitleFont.sansSerif);
      expect(SystemCaptionStyle.fontOf('sans-serif'), SubtitleFont.sansSerif);
      expect(SystemCaptionStyle.fontOf('serif'), SubtitleFont.serif);
      expect(
        SystemCaptionStyle.fontOf('serif-monospace'),
        SubtitleFont.monospace,
      );
      expect(
        SystemCaptionStyle.fontOf('sans-serif-condensed'),
        SubtitleFont.condensed,
      );
      expect(
        SystemCaptionStyle.fontOf('sans-serif-smallcaps'),
        SubtitleFont.smallCaps,
      );
      expect(SystemCaptionStyle.fontOf('casual'), SubtitleFont.casual);
      expect(SystemCaptionStyle.fontOf('cursive'), SubtitleFont.cursive);
    });

    testWidgets('read() is null on desktop', (t) async {
      // Tests run on the host OS: no system caption style, no throw.
      expect(await SystemCaptionStyle.read(), isNull);
    });
  });

  group('languages', () {
    test('normalize to ISO 639-2/B', () {
      expect(normalizeLanguage('ru'), 'rus');
      expect(normalizeLanguage('RUS'), 'rus');
      expect(normalizeLanguage('Russian'), 'rus');
      expect(normalizeLanguage('en-US'), 'eng');
      expect(normalizeLanguage('deu'), 'ger');
      expect(normalizeLanguage('pt_BR'), 'por');
      expect(normalizeLanguage('und'), isNull);
      expect(normalizeLanguage(''), isNull);
      expect(normalizeLanguage(null), isNull);
    });
  });

  group('series memory', () {
    test('choice and delay JSON round trip', () {
      const m = SubtitleMemory(
        choice: SubtitleChoice(language: 'rus', forced: true, external: true),
        delay: Duration(milliseconds: -1300),
      );
      expect(SubtitleMemory.fromJson(m.toJson()), m);
      const off = SubtitleMemory(choice: SubtitleChoice.none);
      expect(SubtitleMemory.fromJson(off.toJson()), off);
      expect(SubtitleMemory.fromJson(null), const SubtitleMemory());
      expect(
        SubtitleMemory.fromJson({'delayMs': 99999999}).delay,
        const Duration(milliseconds: SubtitleMemory.maxDelayMs),
      );
    });

    test('a choice from a track', () {
      expect(SubtitleChoice.of(null), SubtitleChoice.none);
      expect(SubtitleChoice.of(MediaTrack.off), SubtitleChoice.none);
      expect(
        SubtitleChoice.of(
          const MediaTrack(id: '3', language: 'ru', hearingImpaired: true),
        ),
        const SubtitleChoice(language: 'rus', hearingImpaired: true),
      );
    });
  });

  group('delay label', () {
    test('says which way subtitles move', () {
      const en = PaPlayerStrings.en;
      const ru = PaPlayerStrings.ru;
      expect(en.delayLabel(Duration.zero), en.subtitleDelayNone);
      expect(
        en.delayLabel(const Duration(milliseconds: 300)),
        'Subtitles 0.3 s later',
      );
      expect(
        en.delayLabel(const Duration(milliseconds: -1250)),
        'Subtitles 1.25 s earlier',
      );
      expect(
        ru.delayLabel(const Duration(milliseconds: 500)),
        'Субтитры позже на 0,5 с',
      );
      expect(
        ru.delayLabel(const Duration(milliseconds: -2000)),
        'Субтитры раньше на 2,0 с',
      );
    });
  });
}
