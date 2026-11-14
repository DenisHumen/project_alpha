// SPDX-License-Identifier: MPL-2.0
import 'dart:async';

import 'package:flutter/foundation.dart';

import '../play_request.dart';
import 'progress_reporter.dart';

/// What plays after the current video (the next episode), with its own
/// progress callback and the loader of the item after it, so continuous
/// playback can go on through a season.
@immutable
class UpNextItem {
  const UpNextItem({
    required this.request,
    required this.onProgress,
    this.loadNext,
  });

  final PlayRequest request;
  final ProgressCallback onProgress;
  final Future<UpNextItem?> Function()? loadNext;
}

/// State of the Up Next card.
enum UpNextPhase {
  hidden,

  /// Counting down; plays the item at zero.
  countdown,

  /// Shown without a countdown; waits for "Play now".
  offered,
}

/// The Up Next card at the end of a video (PLAY-005). Mirrors `UpNext` of
/// the core playback session (`pa-playback` `session`): with continuous
/// playback it counts [countdown] down and calls [onPlay]; "Play now" skips
/// the wait, "Cancel" hides the card; going to the background stops the
/// countdown and keeps the card.
class UpNextController extends ChangeNotifier {
  UpNextController({
    required this.onPlay,
    this.countdown = const Duration(seconds: 10),
    this.continuous = true,
  });

  final void Function(UpNextItem item) onPlay;
  final Duration countdown;
  final bool continuous;

  UpNextItem? _item;
  UpNextPhase _phase = UpNextPhase.hidden;
  Duration _remaining = Duration.zero;
  Timer? _timer;

  UpNextItem? get item => _item;
  UpNextPhase get phase => _phase;
  bool get visible => _phase != UpNextPhase.hidden;

  /// Time left in [UpNextPhase.countdown].
  Duration get remaining => _remaining;

  /// Shows the card for [item] (restarts the countdown).
  void show(UpNextItem item) {
    _timer?.cancel();
    _item = item;
    if (continuous) {
      _phase = UpNextPhase.countdown;
      _remaining = countdown;
      _timer = Timer.periodic(const Duration(seconds: 1), (_) => _tick());
    } else {
      _phase = UpNextPhase.offered;
    }
    notifyListeners();
  }

  void _tick() {
    _remaining -= const Duration(seconds: 1);
    if (_remaining <= Duration.zero) {
      playNow();
    } else {
      notifyListeners();
    }
  }

  /// "Play now".
  void playNow() {
    final item = _item;
    if (item == null || !visible) return;
    hide();
    onPlay(item);
  }

  /// "Cancel" (or the card is no longer relevant).
  void hide() {
    _timer?.cancel();
    _timer = null;
    if (_phase == UpNextPhase.hidden) return;
    _phase = UpNextPhase.hidden;
    notifyListeners();
  }

  /// The app went to the background: no autoplay while nobody watches.
  void stopCountdown() {
    if (_phase != UpNextPhase.countdown) return;
    _timer?.cancel();
    _timer = null;
    _phase = UpNextPhase.offered;
    notifyListeners();
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}
