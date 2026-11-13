// SPDX-License-Identifier: MPL-2.0
import 'dart:async';

import 'package:flutter/foundation.dart';

/// Side panels of the OSD.
enum OsdPanel { none, audio, subtitles, subtitleFile, speed, info }

/// Visibility logic of the player OSD: shown on interaction, auto-hidden
/// after [autoHide] while playing; stays up while paused or a panel is open.
class OsdController extends ChangeNotifier {
  OsdController({this.autoHide = const Duration(seconds: 4)});

  final Duration autoHide;
  bool _visible = true;
  bool _playing = false;
  OsdPanel _panel = OsdPanel.none;
  Timer? _timer;

  bool get visible => _visible;
  OsdPanel get panel => _panel;

  /// Shows the OSD and restarts the auto-hide timer.
  void show() {
    final changed = !_visible;
    _visible = true;
    _restartTimer();
    if (changed) notifyListeners();
  }

  void hide() {
    if (_panel != OsdPanel.none) return;
    _timer?.cancel();
    if (_visible) {
      _visible = false;
      notifyListeners();
    }
  }

  void toggle() => _visible ? hide() : show();

  /// Feed the engine's playing flag.
  set playing(bool v) {
    if (v == _playing) return;
    _playing = v;
    if (!v) {
      show();
    } else {
      _restartTimer();
    }
  }

  void openPanel(OsdPanel p) {
    _panel = p;
    _visible = true;
    _timer?.cancel();
    notifyListeners();
    if (p == OsdPanel.none) _restartTimer();
  }

  void closePanel() => openPanel(OsdPanel.none);

  void _restartTimer() {
    _timer?.cancel();
    if (!_playing || _panel != OsdPanel.none) return;
    _timer = Timer(autoHide, hide);
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}
