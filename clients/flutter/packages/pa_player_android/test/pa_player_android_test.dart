// SPDX-License-Identifier: MPL-2.0
import 'dart:convert';

import 'package:flutter/services.dart';
import 'package:flutter_test/flutter_test.dart';
import 'package:pa_player_android/pa_player_android.dart';

void main() {
  TestWidgetsFlutterBinding.ensureInitialized();
  final messenger = TestDefaultBinaryMessengerBinding.instance.defaultBinaryMessenger;

  group('PaPlayerAndroid.open', () {
    test('sends the OpenRequest and returns the final state', () async {
      MethodCall? sent;
      messenger.setMockMethodCallHandler(PaPlayerAndroid.channel, (call) async {
        sent = call;
        return jsonEncode({'position_ms': 4200, 'completed': false});
      });
      addTearDown(() => messenger.setMockMethodCallHandler(PaPlayerAndroid.channel, null));

      final state = await PaPlayerAndroid.open(
        const [PlayerItem('file:///m/a.mkv', title: 'A'), PlayerItem('file:///m/b.mkv')],
        index: 1,
        startPositionMs: 1500,
        play: false,
      );

      expect(sent!.method, 'open');
      final args = (sent!.arguments as Map).cast<String, Object?>();
      expect(args['native_osd'], false);
      expect(args['osd_entrypoint'], 'osdMain');
      expect(jsonDecode(args['request']! as String), {
        'items': [
          {'uri': 'file:///m/a.mkv', 'title': 'A'},
          {'uri': 'file:///m/b.mkv'},
        ],
        'index': 1,
        'start_position_ms': 1500,
        'play': false,
      });
      expect(state, {'position_ms': 4200, 'completed': false});
    });

    test('rejects an empty playlist', () {
      expect(() => PaPlayerAndroid.open(const []), throwsArgumentError);
    });
  });

  group('PlayerOsdBridge', () {
    const commands = MethodChannel('test/osd');
    const events = EventChannel('test/events');

    test('commands map to the contract and errors keep their kind', () async {
      final calls = <MethodCall>[];
      messenger.setMockMethodCallHandler(commands, (call) async {
        calls.add(call);
        if (call.method == 'jump') throw PlatformException(code: 'invalid_argument');
        if (call.method == 'stats') return jsonEncode({'video_codec': 'hevc', 'dropped_frames': 0});
        return null;
      });
      messenger.setMockStreamHandler(events, MockStreamHandler.inline(onListen: (_, _) {}));
      addTearDown(() => messenger.setMockMethodCallHandler(commands, null));

      final osd = PlayerOsdBridge(commands: commands, events: events);
      await osd.seek(7000);
      await osd.setAudioTrack('2');
      await osd.setSubtitleTrack(null);
      await osd.addExternalSubtitle('file:///s.srt', title: 'RU');
      expect((await osd.stats())['video_codec'], 'hevc');
      await expectLater(osd.jump(5), throwsA(isA<PlatformException>().having((e) => e.code, 'code', 'invalid_argument')));
      expect(calls.map((c) => '${c.method}:${c.arguments}'), [
        'seek:7000',
        'setAudioTrack:2',
        'setSubtitleTrack:null',
        'addExternalSubtitle:{uri: file:///s.srt, title: RU}',
        'stats:null',
        'jump:5',
      ]);
      await osd.dispose();
    });

    test('events update the state in order', () async {
      late MockStreamHandlerEventSink sink;
      messenger.setMockStreamHandler(
        events,
        MockStreamHandler.inline(onListen: (_, s) => sink = s),
      );
      final osd = PlayerOsdBridge(commands: commands, events: events);
      final types = <String>[];
      osd.updates.listen((u) => types.add(u.type));
      await pumpEventQueue();

      sink.success(jsonEncode({
        'event': {'event': 'loading', 'index': 0},
        'state': {'loaded': false, 'playlist_index': 0},
      }));
      sink.success(jsonEncode({
        'event': {'event': 'position', 'position_ms': 1200, 'buffered_ms': 5000},
        'state': {'loaded': true, 'position_ms': 1200},
      }));
      await pumpEventQueue();

      expect(types, ['loading', 'position']);
      expect(osd.state.value['position_ms'], 1200);
      await osd.dispose();
    });
  });
}
