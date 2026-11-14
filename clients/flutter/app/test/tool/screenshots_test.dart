// SPDX-License-Identifier: MPL-2.0
//
// Screenshots of the main screens (demo library, art drawn in code) for
// docs/screenshots/0.1. Run from clients/flutter/app:
//   KINOLUNE_SCREENSHOTS_DIR=../../../docs/screenshots/0.1 \
//     flutter test test/tool/screenshots_test.dart
// Without the variable the test only exercises the flows (no files).

import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/widgets/cards.dart';
import 'package:kinolune/widgets/focusable.dart';
import 'package:material_ui/material_ui.dart';

import '../support/test_app.dart';

typedef _Setup = Future<void> Function(
  WidgetTester tester,
  AppServices services,
);

class _Shot {
  const _Shot(this.name, this.section, [this.setup, this.empty = false]);
  final String name;
  final Section section;
  final _Setup? setup;
  final bool empty;
}

Future<void> _tapText(WidgetTester tester, String text) async {
  await tester.tap(find.text(text).first);
  await settle(tester, 10);
}

final _desktopShots = <_Shot>[
  const _Shot('home', Section.home),
  _Shot('movies', Section.movies),
  // UI-008: filters, list mode and the context menu.
  _Shot('movies-filtered', Section.movies, (t, _) async {
    await t.tap(find.byKey(const ValueKey('filter-watch-inProgress')));
    await settle(t, 6);
  }),
  _Shot('movies-list', Section.movies, (t, _) async {
    await t.tap(find.byKey(const ValueKey('layout-list')));
    await settle(t, 6);
  }),
  _Shot('movies-menu', Section.movies, (t, _) async {
    await t.tap(
      find
          .descendant(
            of: find.byType(PosterCard).at(1),
            matching: find.byType(KFocusable),
          )
          .first,
      buttons: kSecondaryMouseButton,
    );
    await settle(t, 6);
  }),
  _Shot(
    'movie-detail',
    Section.movies,
    (t, _) => _tapText(t, 'The Quiet Orbit'),
  ),
  _Shot('shows', Section.shows),
  _Shot('show-detail', Section.shows, (t, _) => _tapText(t, 'Moonlit Bureau')),
  _Shot('music', Section.music, (t, s) async {
    await _tapText(t, 'Nocturne Avenue');
    await _tapText(t, 'Play');
    s.music.pause();
    s.music.seek(const Duration(seconds: 74));
    await t.tap(find.byTooltip('Back').first);
    await settle(t, 10);
  }),
  _Shot('album', Section.music, (t, s) async {
    await _tapText(t, 'Analog Dawn');
    await _tapText(t, 'Play');
    s.music.pause();
    s.music.seek(const Duration(seconds: 41));
    await settle(t, 3);
  }),
  _Shot('search', Section.search, (t, _) async {
    await t.enterText(find.byType(TextField), 'the');
    await settle(t, 6);
  }),
  const _Shot('settings', Section.settings),
  _Shot('add-source', Section.settings, (t, _) async {
    await _tapText(t, 'Add source');
    await _tapText(t, 'SMB share (Windows, NAS)');
    await t.enterText(find.widgetWithText(TextField, 'Server'), 'nas.local');
    await t.enterText(find.widgetWithText(TextField, 'Shared folder'), 'media');
    await t.enterText(
      find.widgetWithText(TextField, 'Folder inside the share (optional)'),
      'Movies',
    );
    await t.enterText(
      find.widgetWithText(TextField, 'Username (optional)'),
      'viewer',
    );
    await settle(t, 4);
  }),
  _Shot('about', Section.settings, (t, _) => _tapText(t, 'About Kinolune')),
  const _Shot('onboarding', Section.home, null, true),
];

final _tvShots = <_Shot>[
  _Shot('home', Section.home, (t, _) async {
    await pressKey(t, LogicalKeyboardKey.arrowDown);
    await pressKey(t, LogicalKeyboardKey.arrowRight);
    await settle(t, 8);
  }),
  _Shot('movies', Section.movies, (t, _) async {
    await pressKey(t, LogicalKeyboardKey.arrowDown, times: 2);
    await pressKey(t, LogicalKeyboardKey.arrowRight, times: 2);
  }),
  _Shot('movie-detail', Section.movies, (t, _) => _tapText(t, 'Glass Horizon')),
  _Shot('show-detail', Section.shows, (t, _) async {
    await _tapText(t, 'The Archive');
  }),
  _Shot('music', Section.music, (t, _) async {
    await pressKey(t, LogicalKeyboardKey.arrowDown, times: 2);
  }),
  _Shot('search', Section.search, (t, _) async {
    await t.enterText(find.byType(TextField), 'o');
    await settle(t, 6);
  }),
  const _Shot('settings', Section.settings),
  const _Shot('onboarding', Section.home, null, true),
];

final _phoneShots = <_Shot>[
  const _Shot('home', Section.home),
  const _Shot('movies', Section.movies),
  _Shot('movies-menu', Section.movies, (t, _) async {
    await t.longPress(
      find
          .descendant(
            of: find.byType(PosterCard).at(1),
            matching: find.byType(KFocusable),
          )
          .first,
    );
    await settle(t, 6);
  }),
  _Shot('movie-detail', Section.movies, (t, _) => _tapText(t, 'Harbor Lights')),
];

void main() {
  setUpAll(loadAppFonts);

  void group0(
    String prefix,
    FormFactor ff,
    Size size,
    double dpr,
    List<_Shot> shots,
  ) {
    for (final shot in shots) {
      testWidgets('$prefix-${shot.name}', (tester) async {
        setSurface(tester, size, dpr: dpr);
        final services = await demoServices(empty: shot.empty);
        await tester.pumpWidget(
          testApp(services, formFactor: ff, section: shot.section),
        );
        await settle(tester, 10);
        await shot.setup?.call(tester, services);
        await settle(tester, 8);
        await capture(tester, '$prefix-${shot.name}');
        services.music.pause();
        await tester.pumpWidget(const SizedBox());
        await settle(tester, 2);
      });
    }
  }

  group0(
    'desktop',
    FormFactor.desktop,
    const Size(1280, 800),
    1,
    _desktopShots,
  );
  group0('tv', FormFactor.tv, const Size(960, 540), 2, _tvShots);
  group0('phone', FormFactor.phone, const Size(390, 844), 2, _phoneShots);
}
