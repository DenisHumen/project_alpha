// SPDX-License-Identifier: MPL-2.0

/// Test helpers: a scriptable [FakePlayerEngine] that needs no libmpv.
library;

import 'package:flutter/foundation.dart';
import 'package:flutter/widgets.dart';

import 'src/engine/audio_profile.dart';
import 'src/engine/player_engine.dart';

/// In-memory engine. Tests drive it with [emit] / [advance] / [finish] and
/// inspect [calls].
class FakePlayerEngine implements PlayerEngine {
  FakePlayerEngine({this.loadedDuration = const Duration(minutes: 10)});

  /// Duration reported right after [open].
  Duration loadedDuration;
  final ValueNotifier<EngineState> _state = ValueNotifier(const EngineState());
  final List<String> calls = [];
  List<String> uris = const [];

  /// Last profile passed to [setAudioProfile].
  AudioProfile? audioProfile;
  bool disposed = false;

  @override
  ValueListenable<EngineState> get state => _state;

  /// Replaces parts of the state.
  void emit(EngineState Function(EngineState s) f) =>
      _state.value = f(_state.value);

  /// Moves the playhead forward by [d].
  void advance(Duration d) => emit((s) => s.copyWith(position: s.position + d));

  /// Simulates end of media.
  void finish() => emit(
    (s) => s.copyWith(position: s.duration, playing: false, completed: true),
  );

  @override
  Future<void> open(
    List<String> uris, {
    int index = 0,
    Duration start = Duration.zero,
    bool play = true,
  }) async {
    this.uris = uris;
    calls.add('open:${uris.length}:$index:${start.inMilliseconds}:$play');
    emit(
      (s) => s.copyWith(
        position: start,
        duration: loadedDuration,
        playing: play,
        completed: false,
        playlistIndex: index,
      ),
    );
  }

  @override
  Future<void> play() async {
    calls.add('play');
    emit((s) => s.copyWith(playing: true, completed: false));
  }

  @override
  Future<void> pause() async {
    calls.add('pause');
    emit((s) => s.copyWith(playing: false));
  }

  @override
  Future<void> seek(Duration position) async {
    calls.add('seek:${position.inMilliseconds}');
    emit((s) => s.copyWith(position: position));
  }

  @override
  Future<void> setRate(double rate) async {
    calls.add('rate:$rate');
    emit((s) => s.copyWith(rate: rate));
  }

  @override
  Future<void> setVolume(double volume) async {
    calls.add('volume:$volume');
    emit((s) => s.copyWith(volume: volume));
  }

  @override
  Future<void> jump(int index) async {
    calls.add('jump:$index');
    emit((s) => s.copyWith(playlistIndex: index, position: Duration.zero));
  }

  @override
  Future<void> setRepeat(QueueRepeat mode) async =>
      calls.add('repeat:${mode.name}');

  @override
  Future<void> setAudioProfile(AudioProfile profile) async {
    calls.add('profile:${profile.replayGain.name}');
    audioProfile = profile;
  }

  @override
  Future<void> setAudioTrack(MediaTrack track) async {
    calls.add('audio:${track.id}');
    emit((s) => s.copyWith(audioTrack: track));
  }

  @override
  Future<void> setSubtitleTrack(MediaTrack track) async {
    calls.add('sub:${track.id}');
    emit((s) => s.copyWith(subtitleTrack: track));
  }

  @override
  Future<void> addExternalSubtitle(String uri, {String? title}) async {
    calls.add('subfile:$uri');
    final t = MediaTrack(
      id: 'ext${uris.length}',
      title: title ?? uri,
      external: true,
    );
    emit(
      (s) => s.copyWith(
        subtitleTracks: [...s.subtitleTracks, t],
        subtitleTrack: t,
      ),
    );
  }

  @override
  Widget buildVideo(BuildContext context, {BoxFit fit = BoxFit.contain}) =>
      const ColoredBox(color: Color(0xFF000000), child: SizedBox.expand());

  @override
  Future<void> dispose() async {
    calls.add('dispose');
    disposed = true;
  }
}
