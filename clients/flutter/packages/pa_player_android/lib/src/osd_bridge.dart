// SPDX-License-Identifier: MPL-2.0
import 'dart:async';
import 'dart:convert';

import 'package:flutter/foundation.dart';
import 'package:flutter/services.dart';

/// An engine event with the state after it, both as the contract's JSON
/// (`{"event": "position", "position_ms": …}`; state fields as in
/// `EngineState` of pa-playback, times in milliseconds).
@immutable
class EngineUpdate {
  const EngineUpdate(this.event, this.state);

  factory EngineUpdate.fromJson(String json) {
    final map = jsonDecode(json) as Map<String, Object?>;
    return EngineUpdate(
      (map['event']! as Map).cast<String, Object?>(),
      (map['state']! as Map).cast<String, Object?>(),
    );
  }

  final Map<String, Object?> event;
  final Map<String, Object?> state;

  /// Event tag: `loading`, `loaded`, `position`, `playing_changed`, `ended`, `error`, `closed`…
  String get type => event['event']! as String;
}

/// The OSD side of the player: lives in the OSD Flutter engine above the
/// video and talks to the Media3 engine of the same Activity.
///
/// Commands follow the `PlayerEngine` contract: they return once accepted;
/// a rejected one throws [PlatformException] with the `EngineError` kind as
/// its code (`invalid_state`, `invalid_argument`, `unsupported`, `closed`…).
/// Results arrive through [updates] and [state].
class PlayerOsdBridge {
  PlayerOsdBridge({
    this.commands = const MethodChannel('pa_player_android/osd'),
    EventChannel events = const EventChannel('pa_player_android/events'),
  }) {
    _subscription = events.receiveBroadcastStream().listen((raw) {
      final update = EngineUpdate.fromJson(raw as String);
      state.value = update.state;
      _updates.add(update);
    });
  }

  final MethodChannel commands;
  late final StreamSubscription<Object?> _subscription;
  final _updates = StreamController<EngineUpdate>.broadcast();

  /// Every event of the engine, in order.
  Stream<EngineUpdate> get updates => _updates.stream;

  /// The engine state (fold of all events); empty until the first event.
  final ValueNotifier<Map<String, Object?>> state = ValueNotifier(const {});

  Future<void> play() => _call('play');
  Future<void> pause() => _call('pause');
  Future<void> seek(int positionMs) => _call('seek', positionMs);
  Future<void> setRate(double rate) => _call('setRate', rate);
  Future<void> setVolume(double percent) => _call('setVolume', percent);

  /// `off`, `all` or `one`.
  Future<void> setRepeat(String mode) => _call('setRepeat', mode);
  Future<void> jump(int index) => _call('jump', index);

  /// `null` turns audio off.
  Future<void> setAudioTrack(String? id) => _call('setAudioTrack', id);

  /// `null` turns subtitles off.
  Future<void> setSubtitleTrack(String? id) => _call('setSubtitleTrack', id);
  Future<void> addExternalSubtitle(String uri, {String? title}) =>
      _call('addExternalSubtitle', {'uri': uri, 'title': title});

  /// `PlaybackStats` as JSON (codecs, decoder, dropped frames, bitrate).
  Future<Map<String, Object?>> stats() async {
    final json = await commands.invokeMethod<String>('stats');
    return (jsonDecode(json!) as Map).cast<String, Object?>();
  }

  /// Closes the player Activity.
  Future<void> close() => _call('close');

  Future<void> _call(String method, [Object? args]) => commands.invokeMethod<void>(method, args);

  Future<void> dispose() async {
    await _subscription.cancel();
    await _updates.close();
    state.dispose();
  }
}
