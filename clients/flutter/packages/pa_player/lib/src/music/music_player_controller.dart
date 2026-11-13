// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/foundation.dart';

import '../engine/media_kit_engine.dart';
import '../engine/player_engine.dart';
import '../play_request.dart';
import 'music_queue.dart';

/// App-wide music player. The whole queue is handed to libmpv as a playlist
/// so track changes are gapless (`gapless-audio=yes`, `prefetch-playlist`).
class MusicPlayerController extends ChangeNotifier {
  MusicPlayerController({PlayerEngineFactory? engineFactory, MusicQueue? queue})
    : _factory = engineFactory ?? MediaKitEngine.create,
      queue = queue ?? MusicQueue();

  final PlayerEngineFactory _factory;
  final MusicQueue queue;
  PlayerEngine? _engine;
  bool _disposed = false;

  /// Restart the track instead of going back when past this point.
  static const restartThreshold = Duration(seconds: 3);

  PlayerEngine get _e {
    final e = _engine;
    if (e != null) return e;
    final created = _factory(video: false);
    created.state.addListener(_onEngine);
    return _engine = created;
  }

  EngineState get _s => _engine?.state.value ?? const EngineState();

  PlayRequest? get current => queue.current;
  bool get playing => _s.playing;
  bool get buffering => _s.buffering;
  Duration get position => _s.position;
  Duration get duration {
    final d = _s.duration;
    if (d > Duration.zero) return d;
    final known = current?.durationMs;
    return known == null ? Duration.zero : Duration(milliseconds: known);
  }

  bool get shuffle => queue.shuffle;
  QueueRepeat get repeat => queue.repeat;
  bool get hasQueue => !queue.isEmpty;

  void _onEngine() {
    final s = _s;
    if (s.playlistIndex != queue.position &&
        s.playlistIndex >= 0 &&
        s.playlistIndex < queue.length) {
      queue.moveTo(s.playlistIndex);
    }
    if (!_disposed) notifyListeners();
  }

  Future<void> _load({Duration start = Duration.zero, bool play = true}) {
    return _e.open(
      [for (final t in queue.ordered) t.uri],
      index: queue.position,
      start: start,
      play: play,
    );
  }

  /// Replaces the queue with [queue] and starts playing at [index].
  Future<void> play(List<PlayRequest> queue, {int index = 0}) async {
    this.queue.replace(queue, index: index);
    if (this.queue.isEmpty) return;
    final first = this.queue.current!;
    await _e.setRepeat(this.queue.repeat);
    await _load(start: Duration(milliseconds: first.startPositionMs));
    notifyListeners();
  }

  void pause() => _engine?.pause();

  void resume() => _engine?.play();

  void togglePlay() => playing ? pause() : resume();

  void next() {
    final p = queue.nextPosition();
    if (p == null) return;
    queue.moveTo(p);
    _e.jump(p);
    notifyListeners();
  }

  void previous() {
    if (position > restartThreshold) {
      seek(Duration.zero);
      return;
    }
    final p = queue.previousPosition();
    if (p == null) {
      seek(Duration.zero);
      return;
    }
    queue.moveTo(p);
    _e.jump(p);
    notifyListeners();
  }

  void seek(Duration d) => _engine?.seek(d);

  /// Plays the track at [position] of the play order.
  void jumpTo(int position) {
    if (position < 0 || position >= queue.length) return;
    queue.moveTo(position);
    _e.jump(position);
    notifyListeners();
  }

  Future<void> setShuffle(bool on) async {
    if (on == queue.shuffle) return;
    queue.setShuffle(on);
    if (_engine != null && !queue.isEmpty) {
      // Re-send the playlist in the new order, continuing the current track.
      await _load(start: position, play: playing);
    }
    notifyListeners();
  }

  Future<void> toggleShuffle() => setShuffle(!queue.shuffle);

  Future<void> cycleRepeat() async {
    final m = queue.cycleRepeat();
    await _engine?.setRepeat(m);
    notifyListeners();
  }

  Future<void> setRepeat(QueueRepeat m) async {
    queue.setRepeat(m);
    await _engine?.setRepeat(m);
    notifyListeners();
  }

  @override
  void dispose() {
    _disposed = true;
    _engine?.state.removeListener(_onEngine);
    _engine?.dispose();
    super.dispose();
  }
}
