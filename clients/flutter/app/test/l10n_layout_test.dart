// SPDX-License-Identifier: MPL-2.0
//
// Russian and pseudo-locale (en-XA, ~40 % longer) layouts of the main screens
// on desktop, phone and TV (UI-020). A RenderFlex overflow or any other
// framework error fails the test. With KINOLUNE_SCREENSHOTS_DIR set the
// frames are also written as PNGs (see test/tool/screenshots_test.dart).

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/l10n/l10n.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/preferences.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

typedef _Step = Future<void> Function(WidgetTester t, AppLocalizations l);

Future<void> _tap(WidgetTester t, String text) async {
  await t.tap(find.text(text).first);
  await settle(t, 10);
}

final _flows = <String, (Section, _Step?)>{
  'home': (Section.home, null),
  'movies': (Section.movies, null),
  'movie-detail': (Section.movies, (t, _) => _tap(t, 'The Quiet Orbit')),
  'shows': (Section.shows, null),
  'show-detail': (Section.shows, (t, _) => _tap(t, 'Moonlit Bureau')),
  'music': (Section.music, null),
  'album': (Section.music, (t, _) => _tap(t, 'Nocturne Avenue')),
  'search': (
    Section.search,
    (t, _) async {
      await t.enterText(find.byType(TextField), 'o');
      await settle(t, 6);
    },
  ),
  // UI-023: the player's OSD in the app language, without overflow.
  'player': (
    Section.movies,
    (t, _) async {
      await _tap(t, 'The Quiet Orbit');
      await t.tap(find.byIcon(Icons.play_arrow_rounded).first);
      await settle(t, 10);
    },
  ),
  'settings': (Section.settings, null),
  'add-source': (Section.settings, (t, l) => _tap(t, l.addSourceTitle)),
  // UI-005: the SMB form and the Sources screen.
  'add-source-smb': (
    Section.settings,
    (t, l) async {
      await _tap(t, l.addSourceTitle);
      await _tap(t, l.sourcesTypeSmb);
    },
  ),
  'sources': (
    Section.settings,
    (t, l) async {
      await t.ensureVisible(find.text(l.sourcesManage));
      await settle(t, 4);
      await _tap(t, l.sourcesManage);
    },
  ),
  'about': (
    Section.settings,
    (t, l) async {
      await t.ensureVisible(find.text(l.settingsAboutApp('Kinolune')));
      await settle(t, 4);
      await _tap(t, l.settingsAboutApp('Kinolune'));
    },
  ),
};

const _surfaces = <String, (FormFactor, Size, double)>{
  'desktop': (FormFactor.desktop, Size(1280, 800), 1),
  'phone': (FormFactor.phone, Size(390, 844), 2),
  'tablet': (FormFactor.phone, Size(820, 1180), 2), // rail (UI-002)
  'tv': (FormFactor.tv, Size(960, 540), 2),
};

Future<AppServices> _services(Locale locale, {bool empty = false}) async {
  final s = await demoServices(empty: empty);
  await s.setLocale(locale);
  return s;
}

void main() {
  setUpAll(loadAppFonts);

  for (final locale in const [Locale('ru'), kPseudoLocale]) {
    final tag = localeTag(locale);
    final l = lookupAppLocalizations(locale);
    for (final MapEntry(key: surface, value: (ff, size, dpr))
        in _surfaces.entries) {
      for (final MapEntry(key: name, value: (section, step))
          in _flows.entries) {
        testWidgets('$tag $surface $name', (tester) async {
          setSurface(tester, size, dpr: dpr);
          final services = await _services(locale);
          await tester.pumpWidget(
            testApp(services, formFactor: ff, section: section),
          );
          await settle(tester, 10);
          await step?.call(tester, l);
          await settle(tester, 6);
          await capture(tester, '$tag-$surface-$name');
          services.music.pause();
          await tester.pumpWidget(const SizedBox());
          await settle(tester, 2);
        });
      }
      testWidgets('$tag $surface onboarding', (tester) async {
        setSurface(tester, size, dpr: dpr);
        final services = await _services(locale, empty: true);
        await tester.pumpWidget(testApp(services, formFactor: ff));
        await settle(tester, 10);
        expect(find.text(l.onboardingAddFolder), findsOneWidget);
        await capture(tester, '$tag-$surface-onboarding');
      });
    }
  }

  testWidgets('Russian strings are shown', (tester) async {
    setSurface(tester, const Size(1280, 800));
    await tester.pumpWidget(testApp(await _services(const Locale('ru'))));
    await settle(tester);
    expect(find.text('Продолжить просмотр'), findsOneWidget);
    await scrollHomeTo(tester, find.text('Недавно добавленные фильмы'));
    expect(find.text('Недавно добавленные фильмы'), findsOneWidget);
    expect(find.text('Настройки'), findsOneWidget);
  });

  testWidgets('language follows the system, then the manual choice', (
    tester,
  ) async {
    setSurface(tester, const Size(1280, 800));
    tester.platformDispatcher.localesTestValue = const [Locale('ru', 'RU')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    final base = await demoServices();
    final prefs = MemoryPreferences();
    final services = AppServices(
      library: base.library,
      secureStore: base.secureStore,
      preferences: prefs,
    );
    await services.init();
    final lib = services.library as FakeLibrary;
    await tester.pumpWidget(testApp(services, section: Section.settings));
    await settle(tester);
    // System locale ru-RU → Russian, metadata too (UI-024).
    expect(find.text('Настройки'), findsWidgets);
    expect(lib.metadataLanguage, 'ru-RU');
    await tester.ensureVisible(find.text('English'));
    await settle(tester, 4);
    await tester.tap(find.text('English'));
    await settle(tester, 4);
    expect(find.text('Settings'), findsWidgets);
    expect(prefs.values[Preferences.locale], 'en');
    expect(lib.metadataLanguage, 'en-US');
    // Back to the system language.
    await tester.tap(find.text('System default'));
    await settle(tester, 4);
    expect(find.text('Настройки'), findsWidgets);
    expect(prefs.values.containsKey(Preferences.locale), isFalse);
    expect(lib.metadataLanguage, 'ru-RU');
  });

  testWidgets('saved language is restored on start', (tester) async {
    setSurface(tester, const Size(1280, 800));
    final base = await demoServices();
    final prefs = MemoryPreferences()..values[Preferences.locale] = 'ru';
    final services = AppServices(
      library: base.library,
      secureStore: base.secureStore,
      preferences: prefs,
    );
    await services.init();
    expect(services.locale.value, const Locale('ru'));
    await tester.pumpWidget(testApp(services));
    await settle(tester);
    expect(find.text('Продолжить просмотр'), findsOneWidget);
  });

  testWidgets('unsupported system locale falls back to English', (
    tester,
  ) async {
    setSurface(tester, const Size(1280, 800));
    tester.platformDispatcher.localesTestValue = const [Locale('de', 'DE')];
    addTearDown(tester.platformDispatcher.clearLocalesTestValue);
    await tester.pumpWidget(testApp(await demoServices()));
    await settle(tester);
    expect(find.text('Continue Watching'), findsOneWidget);
  });
}
