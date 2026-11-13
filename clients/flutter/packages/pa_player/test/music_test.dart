// SPDX-License-Identifier: MPL-2.0
import 'dart:math';

import 'package:flutter/material.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player/pa_player.dart';
import 'package:pa_player/testing.dart';

List<PlayRequest> _tracks(int n) => [
  for (var i = 0; i < n; i++)
    PlayRequest(
      uri: '/music/$i.flac',
      title: 'Track $i',
      artist: 'Artist',
      album: 'Album',
    ),
];

void main() {
  group('MusicQueue', () {
    test('linear next/previous and repeat', () {
      final q = MusicQueue()..replace(_tracks(3), index: 1);
      expect(q.current!.title, 'Track 1');
      expect(q.nextPosition(), 2);
      q.moveTo(2);
      expect(q.nextPosition(), isNull);
      q.setRepeat(QueueRepeat.all);
      expect(q.nextPosition(), 0);
      q.moveTo(0);
      expect(q.previousPosition(), 2);
      q.setRepeat(QueueRepeat.one);
      expect(
        q.nextPosition(),
        1,
        reason: 'next is never trapped by repeat-one',
      );
      expect(q.cycleRepeat(), QueueRepeat.off);
    });

    test('shuffle keeps the current track first and restores order', () {
      final q = MusicQueue(random: Random(1))..replace(_tracks(10), index: 4);
      q.setShuffle(true);
      expect(q.position, 0);
      expect(q.current!.title, 'Track 4');
      expect(q.ordered.map((e) => e.title).toSet(), hasLength(10));
      q.moveTo(3);
      final cur = q.current!;
      q.setShuffle(false);
      expect(q.current, same(cur));
      expect(q.ordered.map((e) => e.title).toList(), [
        for (var i = 0; i < 10; i++) 'Track $i',
      ]);
      expect(q.upNext, hasLength(9 - q.position));
    });
  });

  group('AudioProfile', () {
    test('mpv properties of the default music profile', () {
      expect(const AudioProfile().mpvProperties, {
        'gapless-audio': 'yes',
        'prefetch-playlist': 'yes',
        'replaygain': 'track',
        'replaygain-preamp': '0.00',
        'replaygain-clip': 'no',
        'replaygain-fallback': '0.00',
        'audio-exclusive': 'no',
      });
    });

    test('clipping, pre-amp and fallback map to mpv semantics', () {
      final p = const AudioProfile(
        gapless: false,
        replayGain: ReplayGainMode.album,
        preampDb: 40,
        preventClipping: false,
        fallbackDb: -6.5,
        exclusive: true,
      ).mpvProperties;
      expect(p['gapless-audio'], 'no');
      expect(p['prefetch-playlist'], 'no');
      expect(p['replaygain'], 'album');
      expect(p['replaygain-preamp'], '15.00', reason: 'mpv range -15..15');
      expect(p['replaygain-clip'], 'yes', reason: 'yes = allow clipping');
      expect(p['replaygain-fallback'], '-6.50');
      expect(p['audio-exclusive'], 'yes');
    });

    test('auto resolves by album key and shuffle', () {
      const auto = AudioProfile();
      const a1 = PlayRequest(uri: '1', title: '1', album: 'A', albumId: '7');
      const a2 = PlayRequest(uri: '2', title: '2', album: 'A', albumId: '7');
      const sameNameOtherAlbum = PlayRequest(
        uri: '3',
        title: '3',
        album: 'A',
        albumId: '8',
      );
      const noAlbum = PlayRequest(uri: '4', title: '4');
      ReplayGainMode r(List<PlayRequest> l, {bool shuffled = false}) =>
          auto.resolveFor(l, shuffled: shuffled).replayGain;
      expect(r([a1, a2]), ReplayGainMode.album);
      expect(r([a1, a2], shuffled: true), ReplayGainMode.track);
      expect(r([a1, sameNameOtherAlbum]), ReplayGainMode.track);
      expect(r([noAlbum]), ReplayGainMode.track);
      expect(r(const []), ReplayGainMode.track);
      const fixed = AudioProfile(replayGain: ReplayGainMode.album);
      expect(
        fixed.resolveFor([a1, noAlbum], shuffled: true).replayGain,
        ReplayGainMode.album,
        reason: 'explicit modes are kept',
      );
    });
  });

  group('MusicPlayerController', () {
    late FakePlayerEngine engine;
    late MusicPlayerController c;

    setUp(() {
      engine = FakePlayerEngine(loadedDuration: const Duration(minutes: 3));
      c = MusicPlayerController(
        engineFactory: ({bool video = true}) {
          expect(video, isFalse);
          return engine;
        },
      );
    });

    test('play hands the whole queue to the engine (gapless)', () async {
      await c.play(_tracks(4), index: 2);
      expect(engine.uris, hasLength(4));
      expect(engine.calls, contains('open:4:2:0:true'));
      expect(c.current!.title, 'Track 2');
      expect(c.playing, isTrue);
    });

    test('next / previous / restart threshold', () async {
      await c.play(_tracks(3));
      c.next();
      expect(engine.calls.last, 'jump:1');
      expect(c.current!.title, 'Track 1');
      engine.advance(const Duration(seconds: 10));
      c.previous();
      expect(engine.calls.last, 'seek:0', reason: 'restart when >3 s in');
      c.previous();
      expect(engine.calls.last, 'jump:0');
      c.previous();
      expect(engine.calls.last, 'seek:0', reason: 'start of queue');
    });

    test('engine auto-advance updates the current track', () async {
      await c.play(_tracks(3));
      engine.emit((s) => s.copyWith(playlistIndex: 1));
      expect(c.current!.title, 'Track 1');
    });

    test('shuffle re-sends the playlist continuing the track', () async {
      await c.play(_tracks(5), index: 3);
      engine.advance(const Duration(seconds: 42));
      await c.toggleShuffle();
      expect(engine.calls.last, 'open:5:0:42000:true');
      expect(engine.uris.first, '/music/3.flac');
      expect(c.shuffle, isTrue);
    });

    test('ReplayGain auto: album in order, track when shuffled', () async {
      await c.play(_tracks(3));
      expect(engine.audioProfile!.replayGain, ReplayGainMode.album);
      expect(engine.audioProfile!.mpvProperties['replaygain'], 'album');
      expect(
        engine.calls.indexOf('profile:album'),
        lessThan(engine.calls.indexOf('open:3:0:0:true')),
        reason: 'gain is set before the first track starts',
      );
      await c.setShuffle(true);
      expect(engine.audioProfile!.replayGain, ReplayGainMode.track);
      await c.setShuffle(false);
      expect(engine.audioProfile!.replayGain, ReplayGainMode.album);

      final mixed = [
        ..._tracks(2),
        const PlayRequest(uri: '/x.flac', title: 'X', album: 'Other'),
      ];
      await c.play(mixed);
      expect(engine.audioProfile!.replayGain, ReplayGainMode.track);
    });

    test('audio profile changes reach the engine once', () async {
      await c.play(_tracks(2));
      final before = engine.calls.where((e) => e.startsWith('profile')).length;
      await c.setAudioProfile(
        const AudioProfile(replayGain: ReplayGainMode.off, preampDb: 3),
      );
      expect(engine.audioProfile!.mpvProperties['replaygain'], 'no');
      expect(engine.audioProfile!.preampDb, 3);
      await c.setAudioProfile(
        const AudioProfile(replayGain: ReplayGainMode.off, preampDb: 3),
      );
      expect(
        engine.calls.where((e) => e.startsWith('profile')).length,
        before + 1,
      );
    });

    test('repeat is forwarded to the engine', () async {
      await c.play(_tracks(2));
      await c.cycleRepeat();
      expect(engine.calls.last, 'repeat:all');
      await c.cycleRepeat();
      expect(engine.calls.last, 'repeat:one');
    });
  });

  group('widgets', () {
    late FakePlayerEngine engine;
    late MusicPlayerController c;

    setUp(() {
      engine = FakePlayerEngine();
      c = MusicPlayerController(engineFactory: ({bool video = true}) => engine);
    });

    testWidgets('NowPlayingBar hidden without queue, controls work', (t) async {
      await t.pumpWidget(
        MaterialApp(
          home: Scaffold(bottomNavigationBar: NowPlayingBar(controller: c)),
        ),
      );
      expect(find.byKey(const ValueKey('now-playing-bar')), findsNothing);
      await c.play(_tracks(3));
      await t.pump();
      expect(find.text('Track 0'), findsOneWidget);
      await t.tap(find.byKey(const ValueKey('bar-play')));
      await t.pump();
      expect(engine.calls.last, 'pause');
      await t.tap(find.byKey(const ValueKey('bar-next')));
      await t.pump();
      expect(find.text('Track 1'), findsOneWidget);

      // opens Now Playing
      await t.tap(find.byKey(const ValueKey('now-playing-bar')));
      await t.pumpAndSettle();
      expect(find.byType(NowPlayingScreen), findsOneWidget);
    });

    for (final size in [const Size(400, 900), const Size(1600, 900)]) {
      testWidgets('NowPlayingScreen ${size.width}px: transport and queue', (
        t,
      ) async {
        t.view.physicalSize = size;
        t.view.devicePixelRatio = 1;
        addTearDown(t.view.reset);
        await c.play(_tracks(4));
        await t.pumpWidget(MaterialApp(home: NowPlayingScreen(controller: c)));
        await t.pump(const Duration(milliseconds: 500));
        expect(find.byKey(const ValueKey('np-title')), findsOneWidget);
        expect(find.text('Track 0'), findsWidgets);

        await t.tap(find.byKey(const ValueKey('np-shuffle')));
        await t.pump();
        expect(c.shuffle, isTrue);
        await t.tap(find.byKey(const ValueKey('np-repeat')));
        await t.pump();
        expect(c.repeat, QueueRepeat.all);

        await t.tap(find.byKey(const ValueKey('queue-2')));
        await t.pump();
        expect(engine.calls.last, 'jump:2');
        await t.pump(const Duration(milliseconds: 500));
      });
    }
  });
}
