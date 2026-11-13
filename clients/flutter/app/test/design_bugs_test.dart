// SPDX-License-Identifier: MPL-2.0
//
// Regressions for the bugs found by the design screenshots (UI-023):
// phone navigation highlight on Settings, empty licence page, and the video
// player on a phone in portrait (no overflow, texts in the app language).

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/integration/player.dart';
import 'package:kinolune/l10n/l10n.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

const _phone = Size(390, 844);

Color? _indicator(WidgetTester t) =>
    NavigationBarTheme.of(t.element(find.byType(NavigationDestination).first))
        .indicatorColor;

void main() {
  setUpAll(loadAppFonts);

  group('phone bottom navigation', () {
    testWidgets('highlights the active tab', (t) async {
      setSurface(t, _phone, dpr: 2);
      await t.pumpWidget(
        testApp(
          await demoServices(),
          formFactor: FormFactor.phone,
          section: Section.movies,
        ),
      );
      await settle(t);
      expect(find.byIcon(Icons.movie_rounded), findsOneWidget);
      expect(find.byIcon(Icons.home_rounded), findsNothing);
      expect(_indicator(t), isNot(Colors.transparent));
    });

    testWidgets('Settings (no tab) highlights nothing, not Home', (t) async {
      setSurface(t, _phone, dpr: 2);
      await t.pumpWidget(
        testApp(await demoServices(), formFactor: FormFactor.phone),
      );
      await settle(t);
      expect(find.byIcon(Icons.home_rounded), findsOneWidget);

      // The gear on Home opens Settings.
      await t.tap(find.byTooltip('Settings'));
      await settle(t);
      final shell = t.state<AppShellState>(find.byType(AppShell));
      expect(shell.section, Section.settings);
      expect(find.byIcon(Icons.home_rounded), findsNothing);
      for (final icon in [
        Icons.movie_rounded,
        Icons.tv_rounded,
        Icons.library_music_rounded,
      ]) {
        expect(find.byIcon(icon), findsNothing);
      }
      expect(_indicator(t), Colors.transparent);

      // Back to a tab restores the highlight.
      await t.tap(find.byIcon(Icons.home_outlined));
      await settle(t);
      expect(shell.section, Section.home);
      expect(find.byIcon(Icons.home_rounded), findsOneWidget);
      expect(_indicator(t), isNot(Colors.transparent));
    });
  });

  testWidgets('licence page lists the app notices', (t) async {
    setSurface(t, const Size(1280, 800));
    await t.pumpWidget(
      testApp(await demoServices(), section: Section.settings),
    );
    await settle(t);
    final l = lookupAppLocalizations(const Locale('en'));
    await t.ensureVisible(find.text(l.settingsAboutApp('Kinolune')));
    await settle(t, 4);
    await t.tap(find.text(l.settingsAboutApp('Kinolune')));
    await settle(t);
    await t.tap(find.text(l.aboutLicenses));
    // LicensePage collects entries asynchronously.
    await t.runAsync(() => Future<void>.delayed(const Duration(seconds: 1)));
    await settle(t, 20);
    expect(find.byType(LicensePage), findsOneWidget);
    for (final package in ['Kinolune', 'libmpv', 'FFmpeg', 'Inter', 'TMDB']) {
      expect(find.text(package), findsWidgets, reason: package);
    }
  });

  for (final locale in const [Locale('en'), Locale('ru')]) {
    testWidgets('video player on a phone in portrait ($locale)', (t) async {
      setSurface(t, _phone, dpr: 2);
      final services = await demoServices();
      await services.setLocale(locale);
      await t.pumpWidget(
        testApp(
          services,
          formFactor: FormFactor.phone,
          section: Section.movies,
        ),
      );
      await settle(t);
      await t.tap(find.text('Harbor Lights').first);
      await settle(t);
      await t.tap(find.byIcon(Icons.play_arrow_rounded).first);
      await settle(t);
      expect(find.byType(PlayerScreen), findsOneWidget);
      expect(t.takeException(), isNull);
      final p = PaPlayerStrings.forLocale(locale);
      expect(find.byTooltip(p.back10), findsOneWidget);
      expect(find.byTooltip(p.subtitles), findsOneWidget);
      for (final key in ['osd-play', 'osd-info']) {
        final r = t.getRect(find.byKey(ValueKey(key)));
        expect(r.right, lessThanOrEqualTo(_phone.width), reason: key);
      }
      await t.pumpWidget(const SizedBox());
      await settle(t, 2);
    });
  }
}
