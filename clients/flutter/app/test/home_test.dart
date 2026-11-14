// SPDX-License-Identifier: MPL-2.0
//
// Home (UI-007): the shared Continue row follows library events without
// reloading the screen, Up Next, genre rows, pinned smart shelves, the
// sources row, the Twitch hook of the Continue row and the v0 fallback.

import 'dart:async';

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_home_catalog.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/screens/home_feed.dart';
import 'package:kinolune/screens/home_screen.dart';
import 'package:kinolune/screens/sources_screen.dart';
import 'package:material_ui/material_ui.dart';

import 'support/test_app.dart';

Finder _continue(String id) => find.byKey(ValueKey('continue-$id'));
Finder _upNext(String id) => find.byKey(ValueKey('up-next-$id'));
final _homeScroll = find
    .descendant(of: find.byType(HomeScreen), matching: find.byType(Scrollable))
    .first;

Progress _progress(String id, double f, {bool watched = false}) => Progress(
  itemId: id,
  positionMs: (f * 6000000).round(),
  durationMs: 6000000,
  updatedAt: DateTime(2030),
  watched: watched,
);

void main() {
  setUpAll(loadAppFonts);

  testWidgets('the Continue row updates on a library event in place', (
    t,
  ) async {
    setSurface(t, const Size(1280, 800));
    final services = await demoServices();
    final library = services.library as FakeLibrary;
    await t.pumpWidget(testApp(services));
    await settle(t);

    final state = t.state(find.byType(HomeScreen));
    expect(find.text('Continue Watching'), findsOneWidget);
    expect(_continue('movie-the-quiet-orbit'), findsOneWidget);
    expect(_continue('movie-harbor-lights'), findsNothing);
    // Type badges.
    expect(find.text('Movie'), findsWidgets);
    expect(find.text('Episode'), findsWidgets);

    // Progress saved elsewhere (the player, the watcher, a sync): the card
    // appears first in the row.
    await library.saveProgress(_progress('movie-harbor-lights', .3));
    await settle(t, 4);
    expect(_continue('movie-harbor-lights'), findsOneWidget);
    expect(
      t.getTopLeft(_continue('movie-harbor-lights')).dx,
      lessThan(t.getTopLeft(_continue('movie-the-quiet-orbit')).dx),
    );

    // Finished: the card leaves the row.
    await library.saveProgress(
      _progress('movie-the-quiet-orbit', 1, watched: true),
    );
    await settle(t, 4);
    expect(_continue('movie-the-quiet-orbit'), findsNothing);

    // Same screen, not rebuilt from scratch.
    expect(identical(t.state(find.byType(HomeScreen)), state), isTrue);
  });

  testWidgets('Up Next offers the episode after a finished one', (t) async {
    // Wide enough for every card of both rows to be built.
    setSurface(t, const Size(2600, 1400));
    final services = await demoServices();
    final library = services.library as FakeLibrary;
    await t.pumpWidget(testApp(services));
    await settle(t);

    expect(find.text('Up Next'), findsOneWidget);
    expect(_upNext('show-signal-fires-s1e4'), findsOneWidget);
    // Across seasons: S2E6 was the last of season 2.
    expect(_upNext('show-orchard-street-s3e1'), findsOneWidget);
    // Coastline S1E2 is in progress: Continue, not Up Next.
    expect(_continue('show-coastline-s1e2'), findsOneWidget);
    expect(_upNext('show-coastline-s1e3'), findsNothing);

    await library.saveProgress(
      _progress('show-coastline-s1e2', 1, watched: true),
    );
    await settle(t, 6);
    expect(_continue('show-coastline-s1e2'), findsNothing);
    expect(_upNext('show-coastline-s1e3'), findsOneWidget);
  });

  testWidgets('genre rows, pinned smart shelves and sources', (t) async {
    setSurface(t, const Size(1280, 800));
    final services = await demoServices();
    await t.pumpWidget(testApp(services));
    await settle(t);

    for (final row in [
      'Recently Added Movies',
      'Recently Added TV Shows',
      demoShelfName, 'Science Fiction', 'Comedy', 'Drama']) {
      await t.scrollUntilVisible(find.text(row), 300, scrollable: _homeScroll);
      expect(find.text(row), findsOneWidget);
    }
    await t.scrollUntilVisible(
      find.byKey(const ValueKey('home-source-src-demo')),
      300,
      scrollable: _homeScroll,
    );
    await settle(t, 4);
    expect(find.text('Sources'), findsWidgets);
    expect(find.text('Demo library'), findsOneWidget);
    await t.tap(find.text('Demo library'));
    await settle(t);
    expect(find.byType(SourcesScreen), findsOneWidget);
  });

  group('HomeFeed', () {
    test('Continue merges extra sources (Twitch hook) by recency', () async {
      final library = FakeLibrary.demo();
      final vods = StreamController<List<ContinueEntry>>();
      final feed = HomeFeed(library, continueSources: [vods.stream]);
      final rows = <List<ContinueEntry>>[];
      final sub = feed.continueRow().listen(rows.add);
      await pumpEventQueue();
      expect(rows.last.first.id, 'movie-the-quiet-orbit');

      vods.add([
        ContinueEntry(
          kind: ContinueKind.twitchVod,
          id: 'twitch:v1',
          title: 'Speedrun night',
          progress: Progress(
            itemId: 'twitch:v1',
            positionMs: 600000,
            durationMs: 7200000,
            updatedAt: DateTime(2030),
          ),
        ),
      ]);
      await pumpEventQueue();
      expect(rows.last.first.kind, ContinueKind.twitchVod);
      expect(rows.last.length, rows.first.length + 1);
      await sub.cancel();
      await vods.close();
    });

    test('v0 fallback: no LIB-009 rows, recently added still works', () async {
      final library = FakeLibrary.demo();
      final feed = HomeFeed(library, catalog: BasicHomeCatalog(library));
      final shelves = await feed.shelves().first;
      expect(shelves.map((s) => s.kind), [
        HomeShelfKind.recentMovies,
        HomeShelfKind.recentShows,
      ]);
      expect(await feed.upNext().first, isEmpty);
      expect(await feed.continueRow().first, isNotEmpty);
    });

    test('newest first by date added, unknown last', () {
      LibraryItem item(String id, int? day) => LibraryItem(
        id: id,
        kind: ItemKind.movie,
        title: id,
        filePath: '/$id',
        addedAt: day == null ? null : DateTime(2026, 1, day),
      );
      expect(
        newestFirst([item('a', 1), item('b', null), item('c', 3), item('d', 2)])
            .map((i) => i.id),
        ['c', 'd', 'a', 'b'],
      );
    });
  });
}
