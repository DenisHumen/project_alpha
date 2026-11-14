// SPDX-License-Identifier: MPL-2.0
//
// PLAY-005: progress saves (kill bound, background) and Up Next with a
// countdown in the video player.
import 'package:fake_async/fake_async.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/testing.dart';

class _Harness {
  _Harness({this.loadNext, this.continuous = true});

  final engine = FakePlayerEngine(loadedDuration: const Duration(minutes: 20));
  final saves = <(String, int, int)>[];
  final Future<UpNextItem?> Function()? loadNext;
  final bool continuous;

  ProgressCallback saver(String id) =>
      (p, d) => saves.add((id, p, d));

  Widget app() => MaterialApp(
    home: Builder(
      builder: (context) => Scaffold(
        body: TextButton(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => PlayerScreen(
                request: const PlayRequest(uri: '/tv/e1.mkv', title: 'E1'),
                onProgress: saver('e1'),
                engineFactory: ({bool video = true}) => engine,
                loadNext: loadNext,
                continuousPlayback: continuous,
              ),
            ),
          ),
          child: const Text('open'),
        ),
      ),
    ),
  );

  PlayerScreenState state(WidgetTester t) =>
      t.state<PlayerScreenState>(find.byType(PlayerScreen));
}

UpNextItem _item(_Harness h, String id, {int start = 0, bool chain = true}) =>
    UpNextItem(
      request: PlayRequest(
        uri: '/tv/$id.mkv',
        title: id.toUpperCase(),
        startPositionMs: start,
      ),
      onProgress: h.saver(id),
      loadNext: chain ? () async => _item(h, '${id}x', chain: false) : null,
    );

Future<_Harness> _open(
  WidgetTester t, {
  bool withNext = true,
  bool continuous = true,
}) async {
  late _Harness h;
  h = _Harness(
    loadNext: withNext ? () async => _item(h, 'e2', start: 30000) : null,
    continuous: continuous,
  );
  await t.pumpWidget(h.app());
  await t.tap(find.text('open'));
  await t.pumpAndSettle();
  return h;
}

/// Plays [seconds] in 1 s steps.
Future<void> _play(WidgetTester t, _Harness h, int seconds) async {
  for (var i = 0; i < seconds; i++) {
    h.engine.advance(const Duration(seconds: 1));
    await t.pump(const Duration(seconds: 1));
  }
}

void main() {
  test('UpNextController counts down, plays, stops in background', () {
    fakeAsync((async) {
      final played = <String>[];
      final c = UpNextController(onPlay: (i) => played.add(i.request.uri));
      final item = UpNextItem(
        request: const PlayRequest(uri: 'n', title: 'N'),
        onProgress: (_, _) {},
      );
      c.show(item);
      expect(c.phase, UpNextPhase.countdown);
      expect(c.remaining, const Duration(seconds: 10));
      async.elapse(const Duration(seconds: 9));
      expect(c.remaining, const Duration(seconds: 1));
      expect(played, isEmpty);
      async.elapse(const Duration(seconds: 1));
      expect(played, ['n']);
      expect(c.visible, isFalse);

      c.show(item);
      async.elapse(const Duration(seconds: 3));
      c.stopCountdown();
      expect(c.phase, UpNextPhase.offered);
      async.elapse(const Duration(minutes: 1));
      expect(played, hasLength(1));
      c.playNow();
      expect(played, hasLength(2));

      c.show(item);
      c.hide();
      async.elapse(const Duration(minutes: 1));
      expect(played, hasLength(2));
      c.dispose();
    });
  });

  test('UpNextController without continuous playback waits', () {
    fakeAsync((async) {
      final played = <String>[];
      final c = UpNextController(
        onPlay: (i) => played.add(i.request.uri),
        continuous: false,
      );
      c.show(
        UpNextItem(
          request: const PlayRequest(uri: 'n', title: 'N'),
          onProgress: (_, _) {},
        ),
      );
      async.elapse(const Duration(minutes: 1));
      expect(c.phase, UpNextPhase.offered);
      expect(played, isEmpty);
      c.dispose();
    });
  });

  testWidgets('a kill at any moment loses at most 10 s', (t) async {
    final h = await _open(t, withNext: false);
    for (var s = 0; s < 600; s++) {
      if (s % 97 == 50) {
        h.state(t).seekTo(Duration(seconds: (s * 7) % 500));
      }
      if (s % 131 == 70) h.state(t).togglePlay();
      if (s % 131 == 90) h.state(t).togglePlay();
      h.engine.advance(
        h.engine.state.value.playing ? const Duration(seconds: 1) : Duration.zero,
      );
      await t.pump(const Duration(seconds: 1));
      final pos = h.engine.state.value.position.inMilliseconds;
      final saved = h.saves.isEmpty ? 0 : h.saves.last.$2;
      expect((pos - saved).abs(), lessThanOrEqualTo(10000), reason: 'at $s s');
    }
  });

  testWidgets('saves when the app goes to the background', (t) async {
    final h = await _open(t, withNext: false);
    await _play(t, h, 3);
    expect(h.saves, isEmpty);
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await t.pump();
    expect(h.saves.last, ('e1', 3000, 1200000));
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await t.pump();
  });

  testWidgets('Up Next counts down and plays the next episode', (t) async {
    final h = await _open(t);
    await _play(t, h, 5);
    h.engine.finish();
    await t.pump();
    expect(h.saves.last, ('e1', 1200000, 1200000));
    expect(find.byKey(const ValueKey('up-next')), findsOneWidget);
    expect(find.text('E2'), findsOneWidget);
    expect(find.text('Starts in 10 s'), findsOneWidget);
    await t.pump(const Duration(seconds: 3));
    expect(find.text('Starts in 7 s'), findsOneWidget);
    await t.pump(const Duration(seconds: 7));
    await t.pump();
    expect(h.engine.calls.last, 'open:1:0:30000:true');
    expect(h.engine.uris, ['/tv/e2.mkv']);
    expect(find.byKey(const ValueKey('up-next')), findsNothing);
    expect(find.byKey(const ValueKey('osd-title')), findsOneWidget);
    expect(
      t.widget<Text>(find.byKey(const ValueKey('osd-title'))).data,
      'E2',
    );

    // Progress now goes to e2.
    await _play(t, h, 5);
    expect(h.saves.last, ('e2', 35000, 1200000));

    // The chain goes on: e2 ends → e2x.
    h.engine.finish();
    await t.pump();
    expect(find.text('E2X'), findsOneWidget);
    h.state(t).exit();
    await t.pumpAndSettle();
    expect(find.byType(PlayerScreen), findsNothing);
  });

  testWidgets('Up Next: Enter plays now, Esc cancels, seek back hides', (
    t,
  ) async {
    final h = await _open(t);
    h.engine.finish();
    await t.pump();
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    await t.pump();
    expect(find.byKey(const ValueKey('up-next')), findsNothing);
    expect(find.byType(PlayerScreen), findsOneWidget, reason: 'still open');
    await t.pump(const Duration(seconds: 20));
    expect(h.engine.uris, ['/tv/e1.mkv'], reason: 'cancelled');

    // Play again from the start, end again: the card returns.
    h.state(t).togglePlay();
    await t.pump();
    h.engine.finish();
    await t.pump();
    expect(find.byKey(const ValueKey('up-next')), findsOneWidget);
    h.state(t).togglePlay(); // restarts: completed → seek 0 + play
    await t.pump();
    expect(find.byKey(const ValueKey('up-next')), findsNothing);

    h.engine.finish();
    await t.pump();
    await t.sendKeyEvent(LogicalKeyboardKey.enter);
    await t.pump();
    expect(h.engine.uris, ['/tv/e2.mkv']);
  });

  testWidgets('Up Next buttons and background stop', (t) async {
    final h = await _open(t);
    h.engine.finish();
    await t.pump();
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.paused);
    await t.pump();
    expect(find.byKey(const ValueKey('up-next-countdown')), findsNothing);
    await t.pump(const Duration(seconds: 30));
    expect(h.engine.uris, ['/tv/e1.mkv'], reason: 'no autoplay in background');
    t.binding.handleAppLifecycleStateChanged(AppLifecycleState.resumed);
    await t.pump();
    await t.tap(find.byKey(const ValueKey('up-next-play')));
    await t.pump();
    expect(h.engine.uris, ['/tv/e2.mkv']);
  });

  testWidgets('without continuous playback the card waits', (t) async {
    final h = await _open(t, continuous: false);
    h.engine.finish();
    await t.pump();
    expect(find.byKey(const ValueKey('up-next')), findsOneWidget);
    expect(find.byKey(const ValueKey('up-next-countdown')), findsNothing);
    await t.pump(const Duration(minutes: 1));
    expect(h.engine.uris, ['/tv/e1.mkv']);
    await t.tap(find.byKey(const ValueKey('up-next-cancel')));
    await t.pump();
    expect(find.byKey(const ValueKey('up-next')), findsNothing);
  });

  testWidgets('no Up Next for the last item', (t) async {
    final h = await _open(t, withNext: false);
    h.engine.finish();
    await t.pump(const Duration(seconds: 20));
    expect(find.byKey(const ValueKey('up-next')), findsNothing);
    expect(h.engine.uris, ['/tv/e1.mkv']);
  });
}
