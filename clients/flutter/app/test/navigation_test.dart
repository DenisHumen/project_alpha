// SPDX-License-Identifier: MPL-2.0
//
// Adaptive navigation (UI-002): breakpoints, the chrome per window size,
// the phone Library tab, the Twitch toggle and golden images of the shell
// on the four densities (phone, tablet, desktop, TV).
//
// Goldens use the default test font (no loadAppFonts) so they do not depend
// on fonts installed on the machine. Refresh deliberately:
//   flutter test --update-goldens test/navigation_test.dart

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/services/preferences.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/shell/breakpoints.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

const _phone = Size(390, 844);
const _tablet = Size(820, 1180);
const _desktop = Size(1280, 800);
const _large = Size(1600, 900);
const _tv = Size(960, 540);

Finder _layout(String name) => find.byKey(ValueKey('nav-$name'));

AppShellState _shell(WidgetTester t) =>
    t.state<AppShellState>(find.byType(AppShell));

void main() {
  group('breakpoints', () {
    test('window size classes', () {
      expect(WindowSize.fromWidth(320), WindowSize.compact);
      expect(WindowSize.fromWidth(599.9), WindowSize.compact);
      expect(WindowSize.fromWidth(600), WindowSize.medium);
      expect(WindowSize.fromWidth(1023), WindowSize.medium);
      expect(WindowSize.fromWidth(1024), WindowSize.expanded);
      expect(WindowSize.fromWidth(1439), WindowSize.expanded);
      expect(WindowSize.fromWidth(1440), WindowSize.large);
      expect(WindowSize.fromWidth(3840), WindowSize.large);
    });

    test('navigation chrome per size class and form factor', () {
      for (final f in [FormFactor.phone, FormFactor.desktop]) {
        expect(NavLayout.resolve(f, WindowSize.compact), NavLayout.tabs);
        expect(NavLayout.resolve(f, WindowSize.medium), NavLayout.rail);
        expect(NavLayout.resolve(f, WindowSize.expanded), NavLayout.sidebar);
        expect(NavLayout.resolve(f, WindowSize.large), NavLayout.sidebar);
      }
      for (final s in WindowSize.values) {
        expect(NavLayout.resolve(FormFactor.tv, s), NavLayout.topMenu);
      }
    });
  });

  group('shell layout', () {
    final cases = <(String, FormFactor, Size, String)>[
      ('phone', FormFactor.phone, _phone, 'tabs'),
      ('tablet portrait', FormFactor.phone, _tablet, 'rail'),
      ('tablet landscape', FormFactor.phone, const Size(1180, 820), 'sidebar'),
      ('narrow desktop window', FormFactor.desktop, const Size(560, 800), 'tabs'),
      ('desktop', FormFactor.desktop, _desktop, 'sidebar'),
      ('large desktop', FormFactor.desktop, _large, 'sidebar'),
      ('tv', FormFactor.tv, _tv, 'top-menu'),
    ];
    for (final (name, ff, size, layout) in cases) {
      testWidgets('$name → $layout', (t) async {
        setSurface(t, size, dpr: 2);
        await t.pumpWidget(testApp(await demoServices(), formFactor: ff));
        await settle(t);
        expect(_layout(layout), findsOneWidget);
        expect(t.takeException(), isNull);
      });
    }

    testWidgets('resizing the window keeps the open page', (t) async {
      setSurface(t, _desktop);
      await t.pumpWidget(
        testApp(await demoServices(), section: Section.movies),
      );
      await settle(t);
      await t.tap(find.text('Harbor Lights').first);
      await settle(t);
      expect(find.byIcon(Icons.play_arrow_rounded), findsWidgets);
      for (final (size, layout) in [
        (const Size(800, 800), 'rail'),
        (const Size(500, 800), 'tabs'),
        (_desktop, 'sidebar'),
      ]) {
        setSurface(t, size);
        await settle(t, 4);
        expect(_layout(layout), findsOneWidget);
        expect(_shell(t).section, Section.movies);
        // Still on the detail page, not back at the grid.
        expect(find.text('14 titles'), findsNothing);
      }
    });
  });

  group('phone tabs', () {
    testWidgets('Home · Library · Music · Search, Twitch hidden', (t) async {
      setSurface(t, _phone, dpr: 2);
      await t.pumpWidget(
        testApp(await demoServices(), formFactor: FormFactor.phone),
      );
      await settle(t);
      final labels = t
          .widgetList<NavigationDestination>(find.byType(NavigationDestination))
          .map((d) => d.label)
          .toList();
      expect(labels, ['Home', 'Library', 'Music', 'Search']);
      expect(find.byTooltip('Profile and settings'), findsOneWidget);
    });

    testWidgets('Library switches Movies / TV Shows and remembers', (t) async {
      setSurface(t, _phone, dpr: 2);
      await t.pumpWidget(
        testApp(await demoServices(), formFactor: FormFactor.phone),
      );
      await settle(t);
      await t.tap(find.text('Library'));
      await settle(t);
      expect(_shell(t).section, Section.movies);
      expect(find.text('14 titles'), findsOneWidget);

      await t.tap(find.text('TV Shows'));
      await settle(t);
      expect(_shell(t).section, Section.shows);
      expect(find.byIcon(Icons.video_library_rounded), findsOneWidget);

      await t.tap(find.text('Home'));
      await settle(t);
      await t.tap(find.text('Library'));
      await settle(t);
      expect(_shell(t).section, Section.shows);
    });
  });

  group('Twitch section', () {
    testWidgets('appears and disappears with the setting', (t) async {
      setSurface(t, _phone, dpr: 2);
      final services = await demoServices();
      await t.pumpWidget(testApp(services, formFactor: FormFactor.phone));
      await settle(t);
      expect(find.text('Twitch'), findsNothing);

      await services.setShowTwitch(true);
      await settle(t, 4);
      await t.tap(find.text('Twitch'));
      await settle(t);
      expect(_shell(t).section, Section.twitch);
      expect(find.text('Twitch is on its way'), findsOneWidget);

      await services.setShowTwitch(false);
      await settle(t, 4);
      expect(find.text('Twitch'), findsNothing);
      expect(_shell(t).section, Section.home);
    });

    testWidgets('Settings toggle is saved to preferences', (t) async {
      setSurface(t, _desktop);
      final services = await demoServices();
      await t.pumpWidget(testApp(services, section: Section.settings));
      await settle(t);
      // Sidebar: no Twitch item yet.
      expect(find.text('Twitch'), findsNothing);
      await t.ensureVisible(find.text('Show Twitch'));
      await settle(t, 4);
      await t.tap(find.text('Show Twitch'));
      await settle(t, 4);
      expect(services.showTwitch.value, isTrue);
      expect(await services.preferences.read(Preferences.showTwitch), 'true');
      expect(find.text('Twitch'), findsOneWidget); // sidebar item
    });

    testWidgets('is restored from preferences at start-up', (t) async {
      setSurface(t, _tv, dpr: 2);
      final services = await demoServices();
      await services.preferences.write(Preferences.showTwitch, 'true');
      await services.init();
      await t.pumpWidget(testApp(services, formFactor: FormFactor.tv));
      await settle(t);
      // TV menu: Home → Movies → TV Shows → Twitch.
      await pressKey(t, LogicalKeyboardKey.arrowRight, times: 3);
      expect(_shell(t).section, Section.twitch);
    });
  });

  group('goldens', () {
    final densities = <(String, FormFactor, Size, Section)>[
      ('phone', FormFactor.phone, _phone, Section.movies),
      ('tablet', FormFactor.phone, _tablet, Section.movies),
      ('desktop', FormFactor.desktop, _desktop, Section.movies),
      ('tv', FormFactor.tv, _tv, Section.movies),
    ];
    for (final (name, ff, size, section) in densities) {
      testWidgets('shell on $name', (t) async {
        setSurface(t, size);
        final services = await demoServices();
        await services.setShowTwitch(true);
        await t.pumpWidget(
          testApp(services, formFactor: ff, section: section),
        );
        await settle(t);
        await expectLater(
          find.byKey(screenshotKey),
          matchesGoldenFile('goldens/shell_$name.png'),
        );
        await t.pumpWidget(const SizedBox());
        await settle(t, 2);
      });
    }
  });
}
