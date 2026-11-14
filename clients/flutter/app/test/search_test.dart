// SPDX-License-Identifier: MPL-2.0
//
// Search (UI-011): debounce, stale results, grouping, history, keyboard.

import 'dart:async';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/screens/movie_detail_screen.dart';
import 'package:kinolune/screens/search_model.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/preferences.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

LibraryItem _item(String id, ItemKind kind) =>
    LibraryItem(id: id, kind: kind, title: id, filePath: '/media/$id');

Future<AppServices> _services(Preferences prefs) async {
  final s = AppServices(
    library: FakeLibrary.demo(scanStepDelay: Duration.zero),
    secureStore: MemorySecureStore(),
    preferences: prefs,
  );
  await s.init();
  return s;
}

void main() {
  group('LibrarySearch', () {
    testWidgets('searches once, after the debounce of the last edit', (
      tester,
    ) async {
      final calls = <String>[];
      final search = LibrarySearch((q) async {
        calls.add(q);
        return [
          SearchGroup(ItemKind.movie, [_item(q, ItemKind.movie)]),
        ];
      });
      addTearDown(search.dispose);
      var notified = 0;
      search.addListener(() => notified++);

      search.update('d');
      await tester.pump(const Duration(milliseconds: 20));
      search.update('da');
      await tester.pump(const Duration(milliseconds: 20));
      search.update('dar ');
      expect(search.busy, isTrue);
      await tester.pump(searchDebounce - const Duration(milliseconds: 1));
      expect(calls, isEmpty);
      await tester.pump(const Duration(milliseconds: 1));
      expect(calls, ['dar']);
      expect(search.query, 'dar');
      expect(search.groups.single.items.single.id, 'dar');
      expect(search.busy, isFalse);
      expect(notified, greaterThan(0));

      // The same text again does not search again.
      search.update('dar');
      await tester.pump(searchDebounce * 2);
      expect(calls, ['dar']);

      // Clearing the text clears the results at once.
      search.update('  ');
      expect(search.groups, isEmpty);
      expect(search.query, '');
      expect(search.isEmptyResult, isFalse);
    });

    testWidgets('results of an older query are dropped', (tester) async {
      final pending = <String, Completer<List<SearchGroup>>>{};
      final search = LibrarySearch(
        (q) => (pending[q] = Completer<List<SearchGroup>>()).future,
      );
      addTearDown(search.dispose);

      unawaited(search.submit('ab'));
      search.update('abc');
      await tester.pump(searchDebounce);
      expect(pending.keys, ['ab', 'abc']);
      pending['abc']!.complete([
        SearchGroup(ItemKind.show, [_item('abc', ItemKind.show)]),
      ]);
      await tester.pump();
      pending['ab']!.complete([
        SearchGroup(ItemKind.movie, [_item('ab', ItemKind.movie)]),
      ]);
      await tester.pump();
      expect(search.query, 'abc');
      expect(search.groups.single.kind, ItemKind.show);
    });

    testWidgets('no results and errors', (tester) async {
      final search = LibrarySearch((q) async {
        if (q == 'boom') throw StateError('index');
        return const [];
      });
      addTearDown(search.dispose);
      await search.submit('zzz');
      expect(search.isEmptyResult, isTrue);
      await search.submit('boom');
      expect(search.error, isA<StateError>());
      expect(search.isEmptyResult, isFalse);
    });
  });

  test('demo library groups movies, shows, episodes and music', () async {
    final lib = FakeLibrary.demo(scanStepDelay: Duration.zero);
    // "o" is in titles of every kind.
    final groups = await lib.searchGrouped('o', perGroup: 3);
    expect(groups.map((g) => g.kind), searchGroupOrder);
    for (final g in groups) {
      expect(g.items.length, lessThanOrEqualTo(3));
      expect(g.items.every((i) => i.kind == g.kind), isTrue);
    }
    // Titles that start with the query come first.
    final moon = await lib.searchGrouped('moon');
    expect(moon.first.items.first.title.toLowerCase(), startsWith('moon'));
    expect(await lib.searchGrouped(' '), isEmpty);
  });

  group('SearchHistory', () {
    test('newest first, no duplicates, capped, saved', () async {
      final prefs = MemoryPreferences();
      final h = SearchHistory(prefs, max: 3);
      await h.add('Dune');
      await h.add('  ');
      await h.add('Dark');
      await h.add('dune');
      expect(h.value, ['dune', 'Dark']);
      await h.add('Alien');
      await h.add('Up');
      expect(h.value, ['Up', 'Alien', 'dune']);

      final again = SearchHistory(prefs, max: 3);
      await again.load();
      expect(again.value, ['Up', 'Alien', 'dune']);

      await again.remove('Alien');
      expect(again.value, ['Up', 'dune']);
      await again.clear();
      expect(again.value, isEmpty);
      expect(prefs.values.containsKey(Preferences.searchHistory), isFalse);
    });

    test('a broken saved value is ignored', () async {
      final prefs = MemoryPreferences()
        ..values[Preferences.searchHistory] = '{not json';
      final h = SearchHistory(prefs);
      await h.load();
      expect(h.value, isEmpty);
    });
  });

  group('screen', () {
    setUpAll(loadAppFonts);

    testWidgets('results appear within 100 ms of typing, grouped', (
      tester,
    ) async {
      setSurface(tester, const Size(1280, 800));
      await tester.pumpWidget(
        testApp(
          await _services(MemoryPreferences()),
          section: Section.search,
        ),
      );
      await settle(tester);
      await tester.enterText(find.byType(TextField), 'lunar');
      await tester.pump(const Duration(milliseconds: 100));
      await tester.pump();
      expect(find.text('Episodes'), findsOneWidget);
      expect(find.text('Lunar Paperwork'), findsWidgets);
      await tester.enterText(find.byType(TextField), 'qqqqq');
      await settle(tester, 2);
      expect(find.text('No results for “qqqqq”'), findsOneWidget);
    });

    testWidgets('keyboard: ↓ to the results, Enter opens, Esc clears', (
      tester,
    ) async {
      setSurface(tester, const Size(1280, 800));
      final prefs = MemoryPreferences();
      await tester.pumpWidget(
        testApp(await _services(prefs), section: Section.search),
      );
      await settle(tester);
      await tester.tap(find.byType(TextField));
      await tester.enterText(find.byType(TextField), 'orbit');
      await settle(tester, 3);
      final field = FocusManager.instance.primaryFocus;
      await pressKey(tester, LogicalKeyboardKey.arrowDown);
      await settle(tester, 2);
      expect(FocusManager.instance.primaryFocus, isNot(field));
      await pressKey(tester, LogicalKeyboardKey.enter);
      await settle(tester, 6);
      expect(find.byType(MovieDetailScreen), findsOneWidget);
      // The query that led to a result is remembered.
      expect(prefs.values[Preferences.searchHistory], '["orbit"]');
      await tester.tap(find.byTooltip('Back').first);
      await settle(tester, 6);

      await tester.tap(find.byType(TextField));
      await settle(tester, 2);
      await pressKey(tester, LogicalKeyboardKey.escape);
      await settle(tester, 2);
      expect(
        tester.widget<TextField>(find.byType(TextField)).controller!.text,
        isEmpty,
      );
      expect(find.text('Recent searches'), findsOneWidget);
    });

    testWidgets('history: submit saves, an entry searches, clear empties', (
      tester,
    ) async {
      setSurface(tester, const Size(1280, 800));
      final prefs = MemoryPreferences();
      await tester.pumpWidget(
        testApp(await _services(prefs), section: Section.search),
      );
      await settle(tester);
      expect(find.text('Search your library'), findsOneWidget);
      await tester.enterText(find.byType(TextField), 'moonlit');
      await tester.testTextInput.receiveAction(TextInputAction.search);
      await settle(tester, 2);
      expect(find.text('Moonlit Bureau'), findsWidgets);
      await tester.tap(find.byTooltip('Clear'));
      await settle(tester, 2);
      expect(find.text('Recent searches'), findsOneWidget);
      await tester.tap(find.text('moonlit'));
      await settle(tester, 2);
      expect(find.text('Moonlit Bureau'), findsWidgets);
      await tester.tap(find.byTooltip('Clear'));
      await settle(tester, 2);
      await tester.tap(find.text('Clear history'));
      await settle(tester, 2);
      expect(find.text('Search your library'), findsOneWidget);
      expect(prefs.values.containsKey(Preferences.searchHistory), isFalse);
    });
  });
}
