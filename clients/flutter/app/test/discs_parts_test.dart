// SPDX-License-Identifier: MPL-2.0
//
// LIB-020 in the app: a disc image tells why it cannot be played and the
// player does not open; a movie split over two files plays as one mpv EDL
// timeline, and its progress is kept against the item.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/l10n/l10n.dart';
import 'package:kinolune/screens/playback.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pa_player/pa_player.dart' show PlayerScreen, partsOf;
import 'package:pa_player/testing.dart';

import 'support/test_app.dart';

const _iso = LibraryItem(
  id: 'm1',
  kind: ItemKind.movie,
  title: 'Spirited Away',
  year: 2001,
  filePath: '/media/Movies/Spirited Away (2001).iso',
  unplayable: Unplayable.discImage,
);

const _split = LibraryItem(
  id: 'm2',
  kind: ItemKind.movie,
  title: 'Heat',
  year: 1995,
  filePath: '/media/Movies/Heat (1995)/Heat.1995.cd1.avi',
);

const _single = LibraryItem(
  id: 'm3',
  kind: ItemKind.movie,
  title: 'Arrival',
  year: 2016,
  filePath: '/media/Movies/Arrival (2016)/Arrival.2016.mkv',
);

/// A library whose split movie has two parts.
class _PartsLibrary extends FakeLibrary implements LibraryParts {
  _PartsLibrary() : super.items(const [_iso, _split, _single]);

  final asked = <String>[];

  @override
  Future<List<String>> playParts(LibraryItem item) async {
    asked.add(item.playbackUri);
    if (item.id == _split.id) {
      return [item.playbackUri, item.playbackUri.replaceAll('cd1', 'cd2')];
    }
    return [item.playbackUri];
  }
}

Future<(FakePlayerEngine, _PartsLibrary, BuildContext)> _app(
  WidgetTester t,
) async {
  setSurface(t, const Size(1280, 800));
  final engine = FakePlayerEngine(loadedDuration: const Duration(hours: 2));
  final library = _PartsLibrary();
  final services = AppServices(
    library: library,
    secureStore: MemorySecureStore(),
    playerEngineFactory: ({bool video = true}) => engine,
  );
  await services.init();
  await t.pumpWidget(testApp(services));
  await settle(t);
  return (engine, library, t.element(find.byType(Scaffold).first));
}

void main() {
  testWidgets('a disc image shows the reason and does not open the player', (
    t,
  ) async {
    final (engine, library, context) = await _app(t);
    playVideo(context, _iso);
    await settle(t);

    expect(find.byKey(const ValueKey('unplayable-dialog')), findsOneWidget);
    expect(find.text("This file can't be played"), findsOneWidget);
    expect(find.textContaining('Disc images (.iso)'), findsOneWidget);
    expect(find.byType(PlayerScreen), findsNothing);
    expect(engine.calls, isEmpty);
    expect(library.asked, isEmpty);

    await t.tap(find.text('OK'));
    await settle(t);
    expect(find.byKey(const ValueKey('unplayable-dialog')), findsNothing);
    expect(find.byType(PlayerScreen), findsNothing);
    expect(engine.calls, isEmpty);
  });

  testWidgets('a split movie plays both parts as one item', (t) async {
    final (engine, library, context) = await _app(t);
    playVideo(context, _split, startPositionMs: 3000000);
    await settle(t);

    expect(find.byType(PlayerScreen), findsOneWidget);
    expect(library.asked, [_split.filePath]);
    expect(engine.uris, hasLength(1));
    expect(engine.uris.single, startsWith('edl://'));
    expect(partsOf(engine.uris.single), [
      '/media/Movies/Heat (1995)/Heat.1995.cd1.avi',
      '/media/Movies/Heat (1995)/Heat.1995.cd2.avi',
    ]);
    // Resume into the common timeline (part 2 of a 2-hour movie).
    expect(engine.calls.single, 'open:1:0:3000000:true');

    for (var i = 0; i < 6; i++) {
      engine.advance(const Duration(seconds: 1));
      await t.pump(const Duration(seconds: 1));
    }
    final saved = (await library.progress(_split.id))!;
    expect(saved.positionMs, greaterThanOrEqualTo(3004000));
    expect(saved.durationMs, const Duration(hours: 2).inMilliseconds);

    await t.pumpWidget(const SizedBox());
    await settle(t, 2);
  });

  testWidgets('a single file plays as it is', (t) async {
    final (engine, _, context) = await _app(t);
    playVideo(context, _single);
    await settle(t);
    expect(engine.uris, [_single.filePath]);
    await t.pumpWidget(const SizedBox());
    await settle(t, 2);
  });

  test('the Russian reason', () async {
    final ru = await AppLocalizations.delegate.load(const Locale('ru'));
    expect(ru.libraryUnplayableDiscImage, contains('.iso'));
    expect(ru.libraryUnplayableTitle, 'Этот файл не воспроизводится');
  });
}
