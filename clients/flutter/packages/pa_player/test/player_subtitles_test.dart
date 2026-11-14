// SPDX-License-Identifier: MPL-2.0
//
// PLAY-007 in the video player: sidecars from the library are loaded on
// open, a track is picked for the user's language, the overlay draws the
// cue in the subtitle style (system style by default), the delay control
// says which way subtitles move, and the choice and delay carry over to the
// next episode of the series.
import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/testing.dart';

const _forced = ExternalSubtitle(
  uri: 'http://127.0.0.1:9/t/1/Show/E1.ru.forced.srt',
  title: 'E1.ru.forced.srt',
  language: 'rus',
  forced: true,
);
const _full = ExternalSubtitle(
  uri: 'http://127.0.0.1:9/t/1/Show/E1.ru.srt',
  title: 'E1.ru.srt',
  language: 'rus',
);

class _Harness {
  _Harness({
    this.request = const PlayRequest(
      uri: 'http://127.0.0.1:9/t/1/Show/E1.mkv',
      title: 'E1',
      subtitles: [_forced, _full],
      seriesKey: 's1',
    ),
    this.system,
    this.loadNext,
  });

  final engine = FakePlayerEngine();
  final PlayRequest request;
  final SystemCaptionStyle? system;
  final Future<UpNextItem?> Function()? loadNext;
  final settings = ValueNotifier(const SubtitleSettings());
  final memories = <(String, SubtitleMemory)>[];

  Widget app() => MaterialApp(
    home: Builder(
      builder: (context) => Scaffold(
        body: TextButton(
          onPressed: () => Navigator.of(context).push(
            MaterialPageRoute<void>(
              builder: (_) => PlayerScreen(
                request: request,
                onProgress: (_, _) {},
                engineFactory: ({bool video = true}) => engine,
                loadNext: loadNext,
                subtitleSettings: settings,
                subtitleLanguages: const ['ru'],
                captionStyleLoader: () async => system,
                onSubtitleMemory: (k, m) => memories.add((k, m)),
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

Future<_Harness> _open(WidgetTester t, _Harness h) async {
  h.engine.openAudioTracks = const [MediaTrack(id: '1', language: 'rus')];
  await t.pumpWidget(h.app());
  await t.tap(find.text('open'));
  await t.pumpAndSettle();
  return h;
}

TextStyle _subtitleStyle(WidgetTester t) =>
    t.widget<Text>(find.byKey(const ValueKey('subtitle-text'))).style!;

void main() {
  testWidgets('sidecars are loaded and the forced track is picked', (t) async {
    final h = await _open(t, _Harness());
    expect(h.engine.autoLoadSubtitles, isFalse);
    expect(h.engine.calls, containsAllInOrder(['addsub:E1.ru.forced.srt', 'addsub:E1.ru.srt']));
    // Russian audio, Russian reader: only the forced lines.
    expect(h.engine.state.value.subtitleTrack!.title, 'E1.ru.forced.srt');
    expect(h.memories, isEmpty, reason: 'an automatic pick is not a choice');
  });

  testWidgets('without sidecars from the library the engine looks itself', (
    t,
  ) async {
    final h = await _open(
      t,
      _Harness(request: const PlayRequest(uri: '/m/a.mkv', title: 'A')),
    );
    expect(h.engine.autoLoadSubtitles, isTrue);
    expect(h.engine.addedSubtitles, isEmpty);
  });

  testWidgets('the overlay draws the cue in the system style', (t) async {
    final h = await _open(
      t,
      _Harness(
        system: const SystemCaptionStyle(
          enabled: true,
          fontScale: 2,
          foregroundColor: 0xFFFFFF00,
          edgeType: 0,
        ),
      ),
    );
    h.engine.showSubtitle(['Привет', 'мир']);
    await t.pump();
    expect(find.text('Привет\nмир'), findsWidgets);
    final style = _subtitleStyle(t);
    expect(style.color, const Color(0xFFFFFF00));
    final height = t.view.physicalSize.height / t.view.devicePixelRatio;
    expect(
      style.fontSize,
      closeTo(SubtitleOverlay.fontSizeFor(height, 2), 0.01),
    );

    // System captions on: full subtitles are shown even for Russian audio.
    expect(h.engine.state.value.subtitleTrack!.title, 'E1.ru.srt');

    // Custom style replaces it at once.
    h.settings.value = const SubtitleSettings(
      useSystemStyle: false,
      textColor: Color(0xFF00FFFF),
      scale: 0.5,
    );
    await t.pump();
    expect(_subtitleStyle(t).color, const Color(0xFF00FFFF));
    expect(
      _subtitleStyle(t).fontSize,
      closeTo(SubtitleOverlay.fontSizeFor(height, 0.5), 0.01),
    );

    // Subtitles off: nothing drawn.
    await h.state(t).selectSubtitle(MediaTrack.off);
    await t.pump();
    expect(find.byKey(const ValueKey('subtitle-text')), findsNothing);
  });

  testWidgets('delay buttons say which way and are remembered', (t) async {
    final h = await _open(t, _Harness());
    h.state(t).osd.openPanel(OsdPanel.subtitles);
    await t.pumpAndSettle();
    expect(find.text('In sync with the video'), findsOneWidget);

    for (var i = 0; i < 3; i++) {
      await t.tap(find.byKey(const ValueKey('sub-delay-later')));
      await t.pumpAndSettle();
    }
    expect(h.engine.state.value.subtitleDelay.inMilliseconds, 300);
    expect(find.text('Subtitles 0.3 s later'), findsOneWidget);

    for (var i = 0; i < 5; i++) {
      await t.tap(find.byKey(const ValueKey('sub-delay-earlier')));
      await t.pumpAndSettle();
    }
    expect(h.engine.calls.last, 'subdelay:-200');
    expect(find.text('Subtitles 0.2 s earlier'), findsOneWidget);
    expect(h.memories.last.$1, 's1');
    expect(h.memories.last.$2.delay, const Duration(milliseconds: -200));

    await t.tap(find.byKey(const ValueKey('sub-delay-reset')));
    await t.pumpAndSettle();
    expect(h.engine.state.value.subtitleDelay, Duration.zero);
  });

  testWidgets('a picked track is remembered for the series', (t) async {
    final h = await _open(t, _Harness());
    h.state(t).osd.openPanel(OsdPanel.subtitles);
    await t.pumpAndSettle();
    expect(find.text('Forced · External file'), findsOneWidget);
    await t.tap(find.byKey(const ValueKey('sub-ext-E1.ru.srt')));
    await t.pumpAndSettle();
    expect(h.engine.state.value.subtitleTrack!.title, 'E1.ru.srt');
    expect(h.memories.single, (
      's1',
      const SubtitleMemory(
        choice: SubtitleChoice(language: 'rus', external: true),
      ),
    ));
  });

  testWidgets('the remembered choice and delay apply on open', (t) async {
    final h = await _open(
      t,
      _Harness(
        request: const PlayRequest(
          uri: '/tv/e1.mkv',
          title: 'E1',
          subtitles: [_forced, _full],
          seriesKey: 's1',
          subtitleMemory: SubtitleMemory(
            choice: SubtitleChoice(language: 'rus'),
            delay: Duration(milliseconds: 700),
          ),
        ),
      ),
    );
    expect(h.engine.state.value.subtitleTrack!.title, 'E1.ru.srt');
    expect(h.engine.state.value.subtitleDelay.inMilliseconds, 700);
  });

  testWidgets('the next episode keeps this session\'s choice and delay', (
    t,
  ) async {
    late _Harness h;
    h = _Harness(
      loadNext: () async => UpNextItem(
        request: const PlayRequest(
          uri: '/tv/e2.mkv',
          title: 'E2',
          subtitles: [
            ExternalSubtitle(
              uri: '/tv/E2.ru.forced.srt',
              title: 'E2.ru.forced.srt',
              language: 'rus',
              forced: true,
            ),
            ExternalSubtitle(
              uri: '/tv/E2.ru.srt',
              title: 'E2.ru.srt',
              language: 'rus',
            ),
          ],
          seriesKey: 's1',
          // Stored before this session's change.
          subtitleMemory: SubtitleMemory(choice: SubtitleChoice.none),
        ),
        onProgress: (_, _) {},
      ),
    );
    await _open(t, h);
    final s = h.state(t);
    final full = h.engine.state.value.subtitleTracks.firstWhere(
      (x) => x.title == 'E1.ru.srt',
    );
    await s.selectSubtitle(full);
    await s.setSubtitleDelay(const Duration(milliseconds: 400));
    await t.pump();

    h.engine.finish();
    await t.pump();
    s.upNext.playNow();
    await t.pumpAndSettle();
    expect(s.request.title, 'E2');
    expect(h.engine.state.value.subtitleTrack!.title, 'E2.ru.srt');
    expect(h.engine.state.value.subtitleDelay.inMilliseconds, 400);
  });

  testWidgets('a movie without a series starts in sync', (t) async {
    final h = await _open(
      t,
      _Harness(
        request: const PlayRequest(
          uri: '/m/a.mkv',
          title: 'A',
          subtitles: [],
        ),
      ),
    );
    expect(h.engine.calls, contains('subdelay:0'));
    await h.state(t).setSubtitleDelay(const Duration(seconds: 1));
    expect(h.memories, isEmpty);
  });
}
