// SPDX-License-Identifier: MPL-2.0
import 'dart:async';

import '../engine/player_engine.dart';

typedef ProgressCallback = void Function(int positionMs, int durationMs);

/// Calls [onProgress] every [interval] while playing, and on pause, seek and
/// exit (contract: "every 5 s, on pause, seek and exit").
class ProgressReporter {
  ProgressReporter({
    required this.engine,
    required this.onProgress,
    this.interval = const Duration(seconds: 5),
  }) {
    _wasPlaying = engine.state.value.playing;
    engine.state.addListener(_onState);
    _timer = Timer.periodic(interval, (_) {
      if (engine.state.value.playing) report();
    });
  }

  final PlayerEngine engine;
  final ProgressCallback onProgress;
  final Duration interval;
  Timer? _timer;
  late bool _wasPlaying;
  int? _lastPos;
  int? _lastDur;

  void _onState() {
    final playing = engine.state.value.playing;
    if (_wasPlaying && !playing) report();
    _wasPlaying = playing;
  }

  /// Reports the current position (no-op until the duration is known).
  void report({Duration? position}) {
    final s = engine.state.value;
    final dur = s.duration.inMilliseconds;
    if (dur <= 0) return;
    final pos = (position ?? s.position).inMilliseconds.clamp(0, dur);
    if (pos == _lastPos && dur == _lastDur) return;
    _lastPos = pos;
    _lastDur = dur;
    onProgress(pos, dur);
  }

  /// Call after a seek with the target position.
  void onSeek(Duration target) => report(position: target);

  /// Final report on exit; stops the timer.
  void close() {
    _timer?.cancel();
    _timer = null;
    engine.state.removeListener(_onState);
    report();
  }
}
