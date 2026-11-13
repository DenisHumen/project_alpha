// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/testing.dart';

class _Harness {
  final engine = FakePlayerEngine(loadedDuration: const Duration(minutes: 30));
  final progress = <(int, int)>[];
  int finished = 0;

  Widget app({PlayRequest? request}) => MaterialApp(
    home: Builder(
      builder: (context) => Scaffold(
        body: Center(
          child: TextButton(
            onPressed: () => Navigator.of(
              context,
            ).push(MaterialPageRoute<void>(builder: (_) => screen(request))),
            child: const Text('open'),
          ),
        ),
      ),
    ),
  );

  Widget screen(PlayRequest? request) => PlayerScreen(
    request:
        request ??
        const PlayRequest(
          uri: '/media/movie.mkv',
          title: 'Big Buck Bunny',
          startPositionMs: 60000,
        ),
    onProgress: (p, d) => progress.add((p, d)),
    onFinished: () => finished++,
    engineFactory: ({bool video = true}) => engine,
  );

  PlayerScreenState state(WidgetTester t) =>
      t.state<PlayerScreenState>(find.byType(PlayerScreen));
}

Future<_Harness> _open(WidgetTester t, {PlayRequest? request}) async {
  final h = _Harness();
  await t.pumpWidget(h.app(request: request));
  await t.tap(find.text('open'));
  await t.pumpAndSettle();
  return h;
}

double _osdOpacity(WidgetTester t) =>
    t.widget<AnimatedOpacity>(find.byKey(const ValueKey('osd'))).opacity;

void main() {
  testWidgets('opens at startPositionMs and shows the title', (t) async {
    final h = await _open(t);
    expect(h.engine.calls.first, 'open:1:0:60000:true');
    expect(h.engine.uris, ['/media/movie.mkv']);
    expect(find.text('Big Buck Bunny'), findsOneWidget);
    expect(h.engine.state.value.position, const Duration(minutes: 1));
  });

  testWidgets('onProgress every 5 s while playing', (t) async {
    final h = await _open(t);
    h.engine.advance(const Duration(seconds: 5));
    await t.pump(const Duration(seconds: 5));
    expect(h.progress, [(65000, 1800000)]);
    h.engine.advance(const Duration(seconds: 5));
    await t.pump(const Duration(seconds: 5));
    expect(h.progress.last, (70000, 1800000));
    expect(h.progress, hasLength(2));
  });

  testWidgets('onProgress on pause, seek and exit', (t) async {
    final h = await _open(t);
    // pause via Space
    h.engine.advance(const Duration(seconds: 2));
    await t.sendKeyEvent(LogicalKeyboardKey.space);
    await t.pump();
    expect(h.engine.calls, contains('pause'));
    expect(h.progress.last, (62000, 1800000));

    // no periodic reports while paused
    final n = h.progress.length;
    await t.pump(const Duration(seconds: 10));
    expect(h.progress, hasLength(n));

    // seek with arrow key → +10 s
    await t.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await t.pump();
    expect(h.engine.calls, contains('seek:72000'));
    expect(h.progress.last, (72000, 1800000));

    // seek back
    await t.sendKeyEvent(LogicalKeyboardKey.arrowLeft);
    await t.pump();
    expect(h.progress.last, (62000, 1800000));

    // exit with Esc → final report + pop + dispose
    h.engine.advance(const Duration(seconds: 1));
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    await t.pumpAndSettle();
    expect(find.byType(PlayerScreen), findsNothing);
    expect(h.progress.last, (63000, 1800000));
    expect(h.engine.disposed, isTrue);
  });

  testWidgets('Android back key exits and reports', (t) async {
    final h = await _open(t);
    h.engine.advance(const Duration(seconds: 3));
    await t.sendKeyDownEvent(
      LogicalKeyboardKey.goBack,
      physicalKey: PhysicalKeyboardKey.escape, // any mapped scan code
    );
    await t.sendKeyUpEvent(
      LogicalKeyboardKey.goBack,
      physicalKey: PhysicalKeyboardKey.escape, // any mapped scan code
    );
    await t.pumpAndSettle();
    expect(find.byType(PlayerScreen), findsNothing);
    expect(h.progress.last, (63000, 1800000));
  });

  testWidgets('onFinished once at the end', (t) async {
    final h = await _open(t);
    h.engine.finish();
    await t.pump();
    h.engine.emit((s) => s.copyWith(completed: true));
    await t.pump();
    expect(h.finished, 1);
    expect(h.progress.last, (1800000, 1800000));
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('OSD auto-hides while playing, stays while paused', (t) async {
    final h = await _open(t);
    expect(_osdOpacity(t), 1);
    await t.pump(const Duration(seconds: 5));
    await t.pumpAndSettle();
    expect(_osdOpacity(t), 0);

    // any key shows it again
    await t.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await t.pump();
    expect(_osdOpacity(t), 1);

    // paused → stays visible
    h.engine.emit((s) => s.copyWith(playing: false));
    await t.pump(const Duration(seconds: 10));
    expect(_osdOpacity(t), 1);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('tap toggles OSD; play button pauses', (t) async {
    final h = await _open(t);
    await t.tap(find.byKey(const ValueKey('osd-play')));
    await t.pump();
    expect(h.engine.calls, contains('pause'));
    expect(h.engine.state.value.playing, isFalse);
    await t.tap(find.byKey(const ValueKey('osd-play')));
    await t.pump();
    expect(h.engine.state.value.playing, isTrue);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('D-pad: select toggles play when scrubber focused', (t) async {
    final h = await _open(t);
    await t.sendKeyEvent(LogicalKeyboardKey.select);
    await t.pump();
    expect(h.engine.state.value.playing, isFalse);
    await t.sendKeyEvent(LogicalKeyboardKey.select);
    await t.pump();
    expect(h.engine.state.value.playing, isTrue);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('D-pad: down focuses buttons, right moves, select activates', (
    t,
  ) async {
    final h = await _open(t);
    await t.sendKeyEvent(LogicalKeyboardKey.arrowDown); // → play button
    await t.pump();
    await t.sendKeyEvent(LogicalKeyboardKey.arrowRight); // → +10 s
    await t.pump();
    await t.sendKeyEvent(LogicalKeyboardKey.select);
    await t.pump();
    expect(h.engine.calls, contains('seek:70000'));
    // up returns to the scrubber, where right seeks again
    await t.sendKeyEvent(LogicalKeyboardKey.arrowUp);
    await t.pump();
    await t.sendKeyEvent(LogicalKeyboardKey.arrowRight);
    await t.pump();
    expect(h.engine.calls, contains('seek:80000'));
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('audio and subtitle pickers, speed, info', (t) async {
    final h = await _open(t);
    h.engine.emit(
      (s) => s.copyWith(
        audioTracks: const [
          MediaTrack(id: '1', language: 'eng', codec: 'ac3'),
          MediaTrack(id: '2', language: 'rus', codec: 'aac'),
        ],
        audioTrack: const MediaTrack(id: '1', language: 'eng'),
        subtitleTracks: const [MediaTrack(id: '3', title: 'Forced')],
        subtitleTrack: MediaTrack.off,
        info: const {'Video codec': 'h264'},
      ),
    );
    await t.pump();

    await t.tap(find.byKey(const ValueKey('osd-audio')));
    await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('panel-audio')), findsOneWidget);
    await t.tap(find.text('rus (aac)'));
    await t.pumpAndSettle();
    expect(h.engine.calls, contains('audio:2'));
    expect(find.byKey(const ValueKey('panel-audio')), findsNothing);

    await t.tap(find.byKey(const ValueKey('osd-subtitles')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const ValueKey('sub-3')));
    await t.pumpAndSettle();
    expect(h.engine.calls, contains('sub:3'));

    await t.tap(find.byKey(const ValueKey('osd-speed')));
    await t.pumpAndSettle();
    await t.tap(find.byKey(const ValueKey('speed-1.5')));
    await t.pumpAndSettle();
    expect(h.engine.calls, contains('rate:1.5'));

    await t.tap(find.byKey(const ValueKey('osd-info')));
    await t.pumpAndSettle();
    expect(find.text('h264'), findsOneWidget);
    // Esc closes the panel first, not the player
    await t.sendKeyEvent(LogicalKeyboardKey.escape);
    await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('panel-info')), findsNothing);
    expect(find.byType(PlayerScreen), findsOneWidget);
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('external subtitleUri is loaded once the file is open', (
    t,
  ) async {
    final h = await _open(
      t,
      request: const PlayRequest(
        uri: '/m/a.mkv',
        title: 'A',
        subtitleUri: '/m/a.en.srt',
      ),
    );
    expect(
      h.engine.calls.where((c) => c == 'subfile:/m/a.en.srt'),
      hasLength(1),
    );
    await t.pumpWidget(const SizedBox());
  });

  testWidgets('system back (gesture) closes a panel, then exits', (t) async {
    final h = await _open(t);
    await t.tap(find.byKey(const ValueKey('osd-speed')));
    await t.pumpAndSettle();
    await t.binding.handlePopRoute();
    await t.pumpAndSettle();
    expect(find.byKey(const ValueKey('panel-speed')), findsNothing);
    expect(find.byType(PlayerScreen), findsOneWidget);
    await t.binding.handlePopRoute();
    await t.pumpAndSettle();
    expect(find.byType(PlayerScreen), findsNothing);
    expect(h.engine.disposed, isTrue);
  });
}
