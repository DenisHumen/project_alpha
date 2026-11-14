// SPDX-License-Identifier: MPL-2.0
//
// System insets of the shell pages: Android draws edge to edge, so the
// status bar (and a display cutout) overlaps the top of the window. Every
// top-level page must start its header below `MediaQuery.padding.top`,
// while desktop and TV (no inset) keep their layout.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/screens/home_screen.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/theme/kinolune_theme.dart';
import 'package:kinolune/widgets/cards.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

const _statusBar = 48.0;

AppShellState _shell(WidgetTester t) =>
    t.state<AppShellState>(find.byType(AppShell));

/// Top edge of the page header of the visible section: the [PageTitle]
/// text, or on Home the highest text of the page (the hero).
double _headerTop(WidgetTester t, Section s) {
  if (s == Section.home) {
    final texts = find.descendant(
      of: find.byType(HomeScreen),
      matching: find.byType(Text),
    );
    expect(texts, findsWidgets);
    return texts
        .evaluate()
        .map((e) => (e.renderObject! as RenderBox).localToGlobal(Offset.zero).dy)
        .reduce((a, b) => a < b ? a : b);
  }
  final title = find.descendant(
    of: find.byType(PageTitle),
    matching: find.byType(Text),
  );
  expect(title, findsWidgets, reason: '$s has a page title');
  return t.getTopLeft(title.first).dy;
}

void main() {
  final cases = <(String, Size, FormFactor)>[
    ('phone tabs', const Size(390, 844), FormFactor.phone),
    ('tablet rail', const Size(820, 1180), FormFactor.phone),
    ('tablet sidebar', const Size(1180, 820), FormFactor.phone),
  ];
  for (final (name, size, ff) in cases) {
    testWidgets('$name: page headers sit below the status bar', (t) async {
      setSurface(t, size, dpr: 2);
      t.view.padding = const FakeViewPadding(top: _statusBar * 2);
      t.view.viewPadding = const FakeViewPadding(top: _statusBar * 2);
      final services = await demoServices();
      services.showTwitch.value = true;
      await t.pumpWidget(testApp(services, formFactor: ff));
      await settle(t);

      final sections = [
        Section.home,
        Section.movies,
        Section.shows,
        Section.twitch,
        Section.music,
        Section.search,
        Section.settings,
      ];
      for (final s in sections) {
        _shell(t).select(s);
        await settle(t, 4);
        expect(
          _headerTop(t, s),
          greaterThanOrEqualTo(_statusBar),
          reason: '$name: the header of $s is under the status bar',
        );
      }
      expect(find.byKey(const ValueKey('status-bar-scrim')), findsOneWidget);
      expect(t.takeException(), isNull);
    });
  }

  testWidgets('desktop without insets keeps its page padding', (t) async {
    setSurface(t, const Size(1280, 800));
    await t.pumpWidget(
      testApp(await demoServices(), section: Section.movies),
    );
    await settle(t);
    final top = KDensity.desktop.pageInsets.top;
    final title = find.descendant(
      of: find.byType(PageTitle),
      matching: find.byType(Text),
    );
    // The title text sits in the header padding, not pushed further down.
    expect(t.getTopLeft(title.first).dy, closeTo(top, 12));
    expect(find.byKey(const ValueKey('status-bar-scrim')), findsNothing);
  });
}
