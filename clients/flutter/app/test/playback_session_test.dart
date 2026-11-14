// SPDX-License-Identifier: MPL-2.0
//
// PLAY-005 in the app: playVideo saves progress to the library and plays
// the next episode of the show after the Up Next countdown, resuming it.

import 'package:flutter_test/flutter_test.dart';
import 'package:kinolune/fakes/fake_library.dart';
import 'package:kinolune/integration/library.dart';
import 'package:kinolune/screens/playback.dart';
import 'package:kinolune/services/app_services.dart';
import 'package:kinolune/services/secure_store.dart';
import 'package:material_ui/material_ui.dart';
import 'package:pa_player/pa_player.dart' show PlayerScreen;
import 'package:pa_player/testing.dart';

import 'support/test_app.dart';

void main() {
  testWidgets('episode: progress saved, Up Next plays the next one', (t) async {
    setSurface(t, const Size(1280, 800));
    final engine = FakePlayerEngine(loadedDuration: const Duration(minutes: 40));
    final library = FakeLibrary.demo(scanStepDelay: Duration.zero);
    final services = AppServices(
      library: library,
      secureStore: MemorySecureStore(),
      playerEngineFactory: ({bool video = true}) => engine,
    );
    await services.init();
    await t.pumpWidget(testApp(services));
    await settle(t);

    const id = 'show-moonlit-bureau-s2e4';
    final item = (await library.item(id))!;
    final next = (await library.nextEpisode(id))!;
    expect((next.season, next.episode), (2, 5));
    // The next episode was half watched elsewhere: Up Next resumes it.
    await library.saveProgress(
      Progress(
        itemId: next.id,
        positionMs: 600000,
        durationMs: 2400000,
        updatedAt: DateTime(2026),
      ),
    );

    final context = t.element(find.byType(Scaffold).first);
    playVideo(context, item, startPositionMs: 1000000);
    await settle(t);
    expect(find.byType(PlayerScreen), findsOneWidget);
    expect(engine.uris, [item.filePath]);

    // Every 5 s while playing.
    for (var i = 0; i < 6; i++) {
      engine.advance(const Duration(seconds: 1));
      await t.pump(const Duration(seconds: 1));
    }
    final saved = (await library.progress(id))!;
    expect(saved.positionMs, greaterThanOrEqualTo(1004000));

    engine.finish();
    await t.pump();
    expect((await library.progress(id))!.watched, isTrue);
    expect(find.byKey(const ValueKey('up-next')), findsOneWidget);
    await t.pump(const Duration(seconds: 10));
    await t.pump();
    expect(engine.uris, [next.filePath]);
    expect(engine.calls.last, 'open:1:0:600000:true');

    for (var i = 0; i < 6; i++) {
      engine.advance(const Duration(seconds: 1));
      await t.pump(const Duration(seconds: 1));
    }
    expect(
      (await library.progress(next.id))!.positionMs,
      greaterThan(600000),
    );

    await t.pumpWidget(const SizedBox());
    await settle(t, 2);
  });

  testWidgets('movies have no Up Next', (t) async {
    final library = FakeLibrary.demo(scanStepDelay: Duration.zero);
    final movie = (await library.movies().first).first;
    expect(await library.nextEpisode(movie.id), isNull);
  });
}
