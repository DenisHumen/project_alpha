// SPDX-License-Identifier: MPL-2.0
//
// Library grid (UI-008): filter chips, the sort menu saved per view, the
// alphabet index, list mode, badges, the context menu, the v0 fallback
// without filters, and a smoke test over 10 000 titles (lazy pages and
// builders; real-device frame times are QA-006).

import 'package:flutter/gestures.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/integration/player.dart';
import 'package:kinolune/library/library_queries.dart';
import 'package:kinolune/platform/form_factor.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/preferences.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:kinolune/shell/app_shell.dart';
import 'package:kinolune/widgets/cards.dart';
import 'package:kinolune/widgets/focusable.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pa_player/testing.dart';

import 'support/test_app.dart';

final _now = DateTime(2026, 11, 14, 12);

LibraryItem _movie(String id, String title, int year, {int ageDays = 90}) =>
    LibraryItem(
      id: id,
      kind: ItemKind.movie,
      title: title,
      year: year,
      filePath: '/media/Movies/$title.mkv',
      durationMs: 100 * 60000,
      addedAt: _now.subtract(Duration(days: ageDays)),
    );

final _items = [
  _movie('m-alpha', 'Alpha Station', 1995),
  _movie('m-bravo', 'Bravo Run', 2005),
  _movie('m-charlie', 'Charlie Game', 2012),
  _movie('m-delta', 'Delta Blues', 2021, ageDays: 2),
  _movie('m-zulu', 'Zulu Dawn', 2003),
];

const _genres = {
  'm-alpha': ['Drama'],
  'm-bravo': ['Comedy'],
  'm-charlie': ['Drama', 'Comedy'],
  'm-delta': ['Comedy'],
  'm-zulu': ['Drama'],
};

final _progress = [
  Progress(
    itemId: 'm-alpha',
    positionMs: 100 * 60000,
    durationMs: 100 * 60000,
    updatedAt: _now,
    watched: true,
  ),
  Progress(
    itemId: 'm-bravo',
    positionMs: 40 * 60000,
    durationMs: 100 * 60000,
    updatedAt: _now,
  ),
];

/// Services over [items]; [filters] false is the v0 fallback.
Future<AppServices> _services(
  List<LibraryItem> items, {
  List<Progress> progress = const [],
  Map<String, List<MediaVersion>> versions = const {},
  bool filters = true,
  LibraryQueries Function(LibraryQueries q)? wrap,
  Preferences? preferences,
}) async {
  final lib = FakeLibrary.items(items, progress: progress, versions: versions);
  final LibraryQueries q = MemoryLibraryQueries(
    lib,
    supportsFilters: filters,
    genresOf: (i) => _genres[i.id] ?? const [],
    badgeSource: filters ? lib.badges : null,
  );
  final s = AppServices(
    library: lib,
    secureStore: MemorySecureStore(),
    preferences: preferences,
    playerEngineFactory: ({bool video = true}) => FakePlayerEngine(),
    queries: wrap?.call(q) ?? q,
    clock: () => _now,
  );
  await s.init();
  return s;
}

Future<void> _pump(
  WidgetTester t,
  AppServices s, {
  FormFactor ff = FormFactor.desktop,
}) async {
  setSurface(
    t,
    ff == FormFactor.phone ? const Size(390, 844) : const Size(1280, 800),
    dpr: ff == FormFactor.phone ? 2 : 1,
  );
  await t.pumpWidget(testApp(s, formFactor: ff, section: Section.movies));
  await settle(t, 8);
}

Future<void> _tapKey(WidgetTester t, String key) async {
  final f = find.byKey(ValueKey(key));
  await t.ensureVisible(f);
  await settle(t, 2);
  await t.tap(f);
  await settle(t, 6);
}

/// The caption (grid) or row title (list) of an item, not the title drawn
/// into its generated artwork.
Finder _title(String s) => find.byWidgetPredicate(
  (w) => w is Text && w.data == s && w.maxLines == 1,
  description: 'title "$s"',
);

/// Titles on screen in reading order.
List<String> _order(WidgetTester t, List<String> titles) {
  final shown =
      [
        for (final title in titles)
          if (_title(title).evaluate().isNotEmpty)
            (title, t.getTopLeft(_title(title).first)),
      ]..sort((a, b) {
        final dy = a.$2.dy.compareTo(b.$2.dy);
        return dy != 0 ? dy : a.$2.dx.compareTo(b.$2.dx);
      });
  return [for (final (title, _) in shown) title];
}

final _titles = [for (final i in _items) i.title];

Finder _poster(String id) => find.descendant(
  of: find.byKey(ValueKey(id)),
  matching: find.byType(KFocusable),
);

void main() {
  setUpAll(loadAppFonts);

  group('filter chips', () {
    testWidgets('combine: OR inside a group, AND between groups', (t) async {
      await _pump(t, await _services(_items, progress: _progress));
      expect(find.text('5 titles'), findsOneWidget);
      expect(_order(t, _titles), _titles);

      await _tapKey(t, 'filter-watch-inProgress');
      expect(_order(t, _titles), ['Bravo Run']);
      await _tapKey(t, 'filter-watch-watched');
      expect(_order(t, _titles), ['Alpha Station', 'Bravo Run']);
      await _tapKey(t, 'filter-genre-Drama');
      expect(_order(t, _titles), ['Alpha Station']);
      expect(find.text('1 title'), findsOneWidget);

      await _tapKey(t, 'filter-clear');
      expect(_order(t, _titles), _titles);
      expect(find.byKey(const ValueKey('filter-clear')), findsNothing);

      // Decades come from the facets' year range; one at a time.
      for (final y in [1990, 2000, 2010, 2020]) {
        expect(find.byKey(ValueKey('filter-decade-$y')), findsOneWidget);
      }
      await _tapKey(t, 'filter-decade-2000');
      expect(_order(t, _titles), ['Bravo Run', 'Zulu Dawn']);
      await _tapKey(t, 'filter-decade-2020');
      expect(_order(t, _titles), ['Delta Blues']);
      await _tapKey(t, 'filter-genre-Drama');
      expect(find.text('Nothing matches this filter'), findsOneWidget);
    });

    testWidgets('v0 fallback hides the filters', (t) async {
      await _pump(t, await _services(_items, filters: false));
      expect(find.text('5 titles'), findsOneWidget);
      expect(find.byKey(const ValueKey('sort-button')), findsOneWidget);
      expect(find.byKey(const ValueKey('layout-list')), findsOneWidget);
      for (final k in [
        'filter-watch-inProgress',
        'filter-genre-Drama',
        'filter-decade-2000',
      ]) {
        expect(find.byKey(ValueKey(k)), findsNothing, reason: k);
      }
      expect(_order(t, _titles), _titles);
    });
  });

  group('sort', () {
    testWidgets('menu picks a field, flips direction, is saved per view', (
      t,
    ) async {
      final s = await _services(_items);
      await _pump(t, s);
      expect(find.text('Sort: Title'), findsOneWidget);

      await _tapKey(t, 'sort-button');
      for (final f in SortField.values) {
        expect(find.byKey(ValueKey('sort-${f.name}')), findsOneWidget);
      }
      await _tapKey(t, 'sort-year');
      expect(find.text('Sort: Year'), findsOneWidget);
      expect(_order(t, _titles), [
        'Delta Blues',
        'Charlie Game',
        'Bravo Run',
        'Zulu Dawn',
        'Alpha Station',
      ]);
      expect(
        (await s.queries.viewPrefs('movies')).sort,
        const LibrarySort(field: SortField.year, descending: true),
      );

      // The current field again: ascending.
      await _tapKey(t, 'sort-button');
      await _tapKey(t, 'sort-year');
      expect(_order(t, _titles).first, 'Alpha Station');

      await _tapKey(t, 'sort-button');
      await _tapKey(t, 'sort-added');
      expect(_order(t, _titles).first, 'Delta Blues');
      expect(
        (await s.queries.viewPrefs('movies')).sort,
        const LibrarySort(field: SortField.added, descending: true),
      );
      // Other views keep their own sort.
      expect((await s.queries.viewPrefs('shows')).sort, const LibrarySort());

      // A new screen starts with the saved sort.
      await t.pumpWidget(const SizedBox());
      await settle(t, 2);
      await _pump(t, s);
      expect(find.text('Sort: Recently added'), findsOneWidget);
      expect(_order(t, _titles).first, 'Delta Blues');
    });
  });

  group('alphabet index', () {
    final many = [
      for (var c = 0; c < 26; c++)
        for (var n = 0; n < 8; n++)
          _movie(
            'm-$c-$n',
            '${String.fromCharCode(65 + c)} title $n',
            1990 + n,
          ),
    ];

    testWidgets('a letter jumps to its first title', (t) async {
      await _pump(t, await _services(many));
      expect(find.byKey(const ValueKey('alphabet-index')), findsOneWidget);
      expect(_title('A title 0'), findsOneWidget);
      expect(_title('T title 0'), findsNothing);

      await t.tap(find.byKey(const ValueKey('alpha-T')));
      await settle(t, 4);
      expect(_title('T title 0'), findsOneWidget);
      expect(_title('A title 0'), findsNothing);
      final top = t.getTopLeft(_title('T title 0')).dy;
      expect(top, lessThan(400));

      // Dragging the strip scrubs through the letters.
      final strip = t.getRect(find.byKey(const ValueKey('alphabet-index')));
      await t.dragFrom(
        t.getCenter(find.byKey(const ValueKey('alpha-B'))),
        Offset(
          0,
          t.getCenter(find.byKey(const ValueKey('alpha-E'))).dy -
              t.getCenter(find.byKey(const ValueKey('alpha-B'))).dy,
        ),
      );
      await settle(t, 4);
      expect(strip.height, greaterThan(0));
      expect(_title('E title 0'), findsOneWidget);

      // List mode jumps too (the toolbar is at the top).
      t
          .state<ScrollableState>(find.byType(Scrollable).first)
          .position
          .jumpTo(0);
      await settle(t, 2);
      await _tapKey(t, 'layout-list');
      await t.tap(find.byKey(const ValueKey('alpha-W')));
      await settle(t, 4);
      expect(_title('W title 0'), findsOneWidget);
      expect(_title('A title 0'), findsNothing);
    });

    testWidgets('only shown when sorted by title', (t) async {
      await _pump(t, await _services(many));
      await _tapKey(t, 'sort-button');
      await _tapKey(t, 'sort-year');
      expect(find.byKey(const ValueKey('alphabet-index')), findsNothing);
    });
  });

  testWidgets('list mode is remembered per view', (t) async {
    final prefs = MemoryPreferences();
    final s = await _services(_items, progress: _progress, preferences: prefs);
    await _pump(t, s);
    expect(find.byType(PosterCard), findsNWidgets(5));
    await _tapKey(t, 'layout-list');
    expect(find.byType(PosterCard), findsNothing);
    expect(_order(t, _titles), _titles);
    expect(
      find.byKey(const ValueKey('badge-progress-m-bravo')),
      findsOneWidget,
    );
    expect(prefs.values[Preferences.libraryLayout('movies')], 'list');

    await t.pumpWidget(const SizedBox());
    await settle(t, 2);
    await _pump(t, s);
    expect(find.byType(PosterCard), findsNothing);
    expect(_title('Alpha Station'), findsOneWidget);
    await _tapKey(t, 'layout-grid');
    expect(find.byType(PosterCard), findsNWidgets(5));
    expect(
      prefs.values.containsKey(Preferences.libraryLayout('movies')),
      false,
    );
  });

  testWidgets('badges: watched, progress, new', (t) async {
    await _pump(t, await _services(_items, progress: _progress));
    expect(find.byKey(const ValueKey('badge-watched-m-alpha')), findsOneWidget);
    expect(
      find.byKey(const ValueKey('badge-progress-m-bravo')),
      findsOneWidget,
    );
    expect(find.byKey(const ValueKey('badge-new-m-delta')), findsOneWidget);
    expect(find.text('New'), findsOneWidget);
    for (final id in ['m-alpha', 'm-bravo', 'm-charlie', 'm-zulu']) {
      expect(find.byKey(ValueKey('badge-new-$id')), findsNothing, reason: id);
    }
  });

  group('context menu', () {
    testWidgets('right click: entries, "soon" ones disabled, mark watched', (
      t,
    ) async {
      await _pump(t, await _services(_items, progress: _progress));
      await t.tap(_poster('m-charlie'), buttons: kSecondaryMouseButton);
      await settle(t, 4);
      for (final a in [
        'play',
        'markWatched',
        'favorite',
        'playlist',
        'editMetadata',
        'download',
        'delete',
      ]) {
        expect(find.byKey(ValueKey('item-menu-$a')), findsOneWidget, reason: a);
      }
      bool enabled(String a) => t
          .widget<PopupMenuItem<Object?>>(find.byKey(ValueKey('item-menu-$a')))
          .enabled;
      for (final a in [
        'play',
        'markWatched',
        'editMetadata',
        'favorite',
        'playlist',
      ]) {
        expect(enabled(a), isTrue, reason: a);
      }
      for (final a in ['download', 'delete']) {
        expect(enabled(a), isFalse, reason: a);
      }
      expect(find.text('soon'), findsNWidgets(2));

      expect(
        find.byKey(const ValueKey('badge-watched-m-charlie')),
        findsNothing,
      );
      await _tapKey(t, 'item-menu-markWatched');
      expect(
        find.byKey(const ValueKey('badge-watched-m-charlie')),
        findsOneWidget,
      );

      // A watched title offers "unwatched" (no core API yet: disabled).
      await t.tap(_poster('m-charlie'), buttons: kSecondaryMouseButton);
      await settle(t, 4);
      expect(find.byKey(const ValueKey('item-menu-markWatched')), findsNothing);
      expect(
        find.byKey(const ValueKey('item-menu-markUnwatched')),
        findsOneWidget,
      );
      await t.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(t, 4);
    });

    testWidgets('long press on a phone, Play opens the player', (t) async {
      await _pump(
        t,
        await _services(_items, progress: _progress),
        ff: FormFactor.phone,
      );
      await t.longPress(_poster('m-bravo'));
      await settle(t, 4);
      expect(find.byKey(const ValueKey('item-menu-play')), findsOneWidget);
      await _tapKey(t, 'item-menu-play');
      await settle(t, 4);
      expect(find.byType(PlayerScreen), findsOneWidget);
      await t.pumpWidget(const SizedBox());
      await settle(t, 2);
    });

    testWidgets('two files of one film: one card, a version picker on play', (
      t,
    ) async {
      const hd = '/media/Movies/Dune/Dune.2021.1080p.mkv';
      const uhd = '/media/Movies/Dune/Dune.2021.2160p.HDR10.mkv';
      final dune = LibraryItem(
        id: 'm-dune',
        kind: ItemKind.movie,
        title: 'Dune',
        year: 2021,
        filePath: hd,
        durationMs: 155 * 60000,
        addedAt: _now,
        versionCount: 2,
      );
      final s = await _services(
        [..._items, dune],
        versions: {
          'm-dune': const [
            MediaVersion(id: 'v1', label: '1080p', filePath: hd),
            MediaVersion(id: 'v2', label: '4K · HDR10', filePath: uhd),
          ],
        },
      );
      await _pump(t, s);
      // One card for the film.
      expect(_title('Dune'), findsOneWidget);
      expect(find.byKey(const ValueKey('m-dune')), findsOneWidget);

      await t.tap(_poster('m-dune'), buttons: kSecondaryMouseButton);
      await settle(t, 4);
      await _tapKey(t, 'item-menu-play');
      expect(find.text('Choose a version'), findsOneWidget);
      expect(find.byKey(const ValueKey('version-v1')), findsOneWidget);
      expect(find.byKey(const ValueKey('version-v2')), findsOneWidget);
      expect(find.text('Default'), findsOneWidget);

      // Make 4K the default, then play it.
      await t.tap(find.byKey(const ValueKey('version-default-v2')));
      await settle(t, 4);
      expect(find.byKey(const ValueKey('version-default-v2')), findsNothing);
      expect(find.byKey(const ValueKey('version-default-v1')), findsOneWidget);
      await t.tap(find.byKey(const ValueKey('version-v2')));
      await settle(t, 4);
      final player = t.widget<PlayerScreen>(find.byType(PlayerScreen));
      expect(player.request.uri, uhd);
      final lib = s.library as FakeLibrary;
      expect((await lib.item('m-dune'))!.filePath, uhd);
      await t.pumpWidget(const SizedBox());
      await settle(t, 2);
    });

    testWidgets('favorite and add to a new playlist', (t) async {
      final s = await _services(_items);
      final lib = s.library as FakeLibrary;
      await _pump(t, s);

      await t.tap(_poster('m-bravo'), buttons: kSecondaryMouseButton);
      await settle(t, 4);
      expect(find.text('Add to favorites'), findsOneWidget);
      await _tapKey(t, 'item-menu-favorite');
      expect(await lib.isFavorite('m-bravo'), isTrue);
      expect(find.text('Added to favorites'), findsOneWidget);

      await t.tap(_poster('m-bravo'), buttons: kSecondaryMouseButton);
      await settle(t, 4);
      expect(find.text('Remove from favorites'), findsOneWidget);
      await _tapKey(t, 'item-menu-playlist');
      expect(find.text('No playlists yet'), findsOneWidget);
      await t.enterText(
        find.byKey(const ValueKey('playlist-new-name')),
        'Weekend',
      );
      await _tapKey(t, 'playlist-create');
      final playlists = await lib.playlists();
      expect([for (final p in playlists) p.name], ['Weekend']);
      expect(
        [for (final i in await lib.playlistItems(playlists.single.id)) i.id],
        ['m-bravo'],
      );
      expect(find.text('Added to “Weekend”'), findsOneWidget);
    });

    testWidgets('menu key and Edit metadata', (t) async {
      await _pump(t, await _services(_items));
      await t.tap(_poster('m-delta'), buttons: kSecondaryMouseButton);
      await settle(t, 4);
      await _tapKey(t, 'item-menu-editMetadata');
      expect(find.text('Fix match'), findsOneWidget);
      await t.sendKeyEvent(LogicalKeyboardKey.escape);
      await settle(t, 4);

      // Keyboard: focus a poster and press the menu key.
      Focus.of(
        t.element(
          find
              .descendant(
                of: _poster('m-zulu'),
                matching: find.byType(AnimatedScale),
              )
              .first,
        ),
      ).requestFocus();
      await settle(t, 2);
      await t.sendKeyEvent(LogicalKeyboardKey.contextMenu);
      await settle(t, 4);
      expect(find.byKey(const ValueKey('item-menu-play')), findsOneWidget);
    });
  });

  testWidgets('10 000 titles: lazy pages and builders (smoke)', (t) async {
    final items = [
      for (var i = 0; i < 10000; i++)
        _movie(
          'm-$i',
          '${String.fromCharCode(65 + i % 26)} movie ${i.toString().padLeft(5, '0')}',
          1950 + i % 75,
        ),
    ];
    final counting = <(int, int)>[];
    final watch = Stopwatch()..start();
    await _pump(
      t,
      await _services(items, wrap: (q) => _CountingQueries(q, counting)),
    );
    expect(find.text('10000 titles'), findsOneWidget);
    // Only a few pages are asked for, never the whole library.
    expect(counting, isNotEmpty);
    expect(counting.every((r) => r.$2 <= 200), isTrue, reason: '$counting');
    final built = find.byType(PosterCard).evaluate().length;
    expect(built, inInclusiveRange(1, 120));

    // Fling down a few screens: still only what is on screen.
    for (var i = 0; i < 10; i++) {
      await t.fling(
        find.byType(CustomScrollView),
        const Offset(0, -2000),
        4000,
      );
      await settle(t, 3);
    }
    expect(find.byType(PosterCard).evaluate().length, lessThanOrEqualTo(120));

    // Jump to the last letter and to the end of the list.
    await t.tap(find.byKey(const ValueKey('alpha-Z')));
    await settle(t, 4);
    expect(_title('Z movie 00025'), findsOneWidget);
    final scrollable = t.state<ScrollableState>(find.byType(Scrollable).first);
    scrollable.position.jumpTo(scrollable.position.maxScrollExtent);
    await settle(t, 4);
    expect(_title('Z movie 09983'), findsOneWidget);
    expect(find.byType(PosterCard).evaluate().length, lessThanOrEqualTo(120));
    // 84 pages in all; the grid asked only for the ones it showed.
    expect(counting.length, lessThan(40), reason: '$counting');
    // Generous bound for the whole test VM run; frame rate on devices is
    // profiled in QA-006.
    expect(watch.elapsed, lessThan(const Duration(seconds: 60)));
  });
}

/// Records (offset, limit) of every page query.
class _CountingQueries implements LibraryQueries {
  _CountingQueries(this.inner, this.calls);

  final LibraryQueries inner;
  final List<(int, int)> calls;

  @override
  bool get supportsFilters => inner.supportsFilters;

  @override
  Future<LibraryPage> query(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) {
    calls.add((offset, limit));
    return inner.query(query, offset: offset, limit: limit);
  }

  @override
  Stream<LibraryPage> watchQuery(
    LibraryQuery query, {
    int offset = 0,
    int limit = 100,
  }) {
    calls.add((offset, limit));
    return inner.watchQuery(query, offset: offset, limit: limit);
  }

  @override
  Future<List<AlphabetEntry>> alphabetIndex(LibraryQuery query) =>
      inner.alphabetIndex(query);

  @override
  Future<LibraryFacets?> facets(ItemKind kind) => inner.facets(kind);

  @override
  Future<ViewPrefs> viewPrefs(String view) => inner.viewPrefs(view);

  @override
  Future<void> setViewPrefs(ViewPrefs prefs) => inner.setViewPrefs(prefs);

  @override
  Stream<Map<String, ItemBadge>> badges(ItemKind kind) => inner.badges(kind);
}
