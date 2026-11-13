// SPDX-License-Identifier: MPL-2.0
import 'dart:async';
import 'dart:io' show Platform;

import 'package:flutter/foundation.dart';
import 'package:flutter/material.dart';
import 'package:flutter/services.dart';
import 'package:media_kit_video/media_kit_video.dart' as mkv;

import '../engine/media_kit_engine.dart';
import '../engine/player_engine.dart';
import '../l10n/pa_player_strings.dart';
import '../play_request.dart';
import '../ui/focus_button.dart';
import '../ui/scrubber.dart';
import '../ui/tokens.dart';
import 'osd_controller.dart';
import 'osd_panels.dart';
import 'progress_reporter.dart';

/// Fullscreen video player with a tvOS-style OSD.
///
/// Texts come from [PaPlayerStrings.of] (English and Russian).
///
/// Controls: mouse/touch; keyboard (Space play/pause, ←/→ ±10 s, ↑/↓ show
/// OSD and move focus, F fullscreen, I info, Esc back); Android TV remote
/// (Select play/pause or activate, ←/→ seek, Back closes panel / exits).
class PlayerScreen extends StatefulWidget {
  const PlayerScreen({
    super.key,
    required this.request,
    required this.onProgress,
    this.onFinished,
    this.engineFactory,
    this.progressInterval = const Duration(seconds: 5),
    this.seekStep = const Duration(seconds: 10),
  });

  final PlayRequest request;

  /// Called every [progressInterval] while playing, and on pause, seek, exit.
  final void Function(int positionMs, int durationMs) onProgress;

  /// Called once when playback reaches the end.
  final VoidCallback? onFinished;

  /// Engine override (tests). Defaults to [MediaKitEngine].
  final PlayerEngineFactory? engineFactory;
  final Duration progressInterval;
  final Duration seekStep;

  @override
  State<PlayerScreen> createState() => PlayerScreenState();
}

/// Public for tests.
class PlayerScreenState extends State<PlayerScreen> {
  late final PlayerEngine engine;
  late final ProgressReporter _progress;
  final OsdController osd = OsdController(autoHide: PaTokens.osdAutoHide);
  final FocusNode _rootFocus = FocusNode(debugLabel: 'player-root');
  final FocusNode _scrubberFocus = FocusNode(debugLabel: 'player-scrubber');
  final FocusNode _playFocus = FocusNode(debugLabel: 'player-play');
  bool _finished = false;
  bool _subtitleAdded = false;
  bool _fullscreen = false;
  bool _closing = false;
  Offset? _doubleTapAt;

  bool get _isDesktop =>
      !kIsWeb && (Platform.isLinux || Platform.isWindows || Platform.isMacOS);

  @override
  void initState() {
    super.initState();
    final factory = widget.engineFactory ?? MediaKitEngine.create;
    engine = factory(video: true);
    _progress = ProgressReporter(
      engine: engine,
      onProgress: widget.onProgress,
      interval: widget.progressInterval,
    );
    engine.state.addListener(_onEngine);
    osd.addListener(_onOsd);
    engine.open([
      widget.request.uri,
    ], start: Duration(milliseconds: widget.request.startPositionMs));
    if (!kIsWeb && (Platform.isAndroid || Platform.isIOS)) {
      SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
    }
  }

  void _onEngine() {
    final s = engine.state.value;
    osd.playing = s.playing;
    if (!_subtitleAdded &&
        widget.request.subtitleUri != null &&
        s.duration > Duration.zero) {
      _subtitleAdded = true;
      engine.addExternalSubtitle(widget.request.subtitleUri!);
    }
    if (s.completed && !_finished && s.duration > Duration.zero) {
      _finished = true;
      _progress.report(position: s.duration);
      osd.show();
      widget.onFinished?.call();
    }
  }

  void _onOsd() {
    if (!mounted) return;
    final primary = FocusManager.instance.primaryFocus;
    if (!osd.visible) {
      // Never keep focus on invisible buttons.
      if (primary != _rootFocus) _rootFocus.requestFocus();
    } else if (osd.panel == OsdPanel.none &&
        (primary == _rootFocus || primary == null)) {
      _scrubberFocus.requestFocus();
    }
  }

  @override
  void dispose() {
    engine.state.removeListener(_onEngine);
    osd.removeListener(_onOsd);
    _progress.close();
    osd.dispose();
    _rootFocus.dispose();
    _scrubberFocus.dispose();
    _playFocus.dispose();
    engine.dispose();
    if (!kIsWeb && (Platform.isAndroid || Platform.isIOS)) {
      SystemChrome.setEnabledSystemUIMode(SystemUiMode.edgeToEdge);
    }
    if (_fullscreen) mkv.defaultExitNativeFullscreen();
    super.dispose();
  }

  // ---- actions -------------------------------------------------------------

  void togglePlay() {
    final s = engine.state.value;
    if (s.playing) {
      engine.pause();
    } else {
      if (s.completed) engine.seek(Duration.zero);
      engine.play();
    }
    osd.show();
  }

  void seekBy(Duration delta) {
    final s = engine.state.value;
    var target = s.position + delta;
    if (target < Duration.zero) target = Duration.zero;
    if (s.duration > Duration.zero && target > s.duration) target = s.duration;
    seekTo(target);
  }

  void seekTo(Duration target) {
    engine.seek(target);
    _progress.onSeek(target);
    osd.show();
  }

  Future<void> toggleFullscreen() async {
    if (!_isDesktop) return;
    _fullscreen = !_fullscreen;
    _fullscreen
        ? await mkv.defaultEnterNativeFullscreen()
        : await mkv.defaultExitNativeFullscreen();
  }

  void closePanel() {
    osd.closePanel();
    _scrubberFocus.requestFocus();
  }

  /// Back/Esc: close panel, else leave fullscreen (desktop), else exit.
  void handleBack() {
    if (osd.panel != OsdPanel.none) {
      closePanel();
      return;
    }
    if (_fullscreen) {
      toggleFullscreen();
      return;
    }
    exit();
  }

  void exit() {
    if (_closing) return;
    _closing = true;
    _progress.report();
    final nav = Navigator.maybeOf(context);
    if (nav != null && nav.canPop()) nav.pop();
  }

  // ---- input ---------------------------------------------------------------

  bool get _seekFocus {
    final p = FocusManager.instance.primaryFocus;
    return !osd.visible || p == _rootFocus || p == _scrubberFocus;
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is KeyUpEvent) return KeyEventResult.ignored;
    final k = e.logicalKey;
    final wasVisible = osd.visible;
    final repeat = e is KeyRepeatEvent;

    if (k == LogicalKeyboardKey.escape ||
        k == LogicalKeyboardKey.goBack ||
        k == LogicalKeyboardKey.browserBack) {
      if (!repeat) handleBack();
      return KeyEventResult.handled;
    }
    if (osd.panel != OsdPanel.none) {
      osd.show();
      return KeyEventResult.ignored; // let the panel traverse/activate
    }
    if (k == LogicalKeyboardKey.space ||
        k == LogicalKeyboardKey.mediaPlayPause ||
        k == LogicalKeyboardKey.mediaPlay ||
        k == LogicalKeyboardKey.mediaPause) {
      if (repeat) return KeyEventResult.handled;
      if (k == LogicalKeyboardKey.mediaPlay) {
        engine.play();
      } else if (k == LogicalKeyboardKey.mediaPause) {
        engine.pause();
      } else {
        togglePlay();
      }
      osd.show();
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.select ||
        k == LogicalKeyboardKey.enter ||
        k == LogicalKeyboardKey.numpadEnter ||
        k == LogicalKeyboardKey.gameButtonA) {
      if (_seekFocus) {
        if (!repeat) togglePlay();
        return KeyEventResult.handled;
      }
      osd.show();
      return KeyEventResult.ignored; // activates the focused button
    }
    if (k == LogicalKeyboardKey.mediaFastForward ||
        k == LogicalKeyboardKey.mediaSkipForward) {
      seekBy(widget.seekStep);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.mediaRewind ||
        k == LogicalKeyboardKey.mediaSkipBackward) {
      seekBy(-widget.seekStep);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.arrowLeft ||
        k == LogicalKeyboardKey.arrowRight) {
      if (_seekFocus) {
        seekBy(
          k == LogicalKeyboardKey.arrowLeft
              ? -widget.seekStep
              : widget.seekStep,
        );
        return KeyEventResult.handled;
      }
      osd.show();
      return KeyEventResult.ignored; // move between OSD buttons
    }
    if (k == LogicalKeyboardKey.arrowUp || k == LogicalKeyboardKey.arrowDown) {
      osd.show();
      if (!wasVisible) {
        _scrubberFocus.requestFocus();
        return KeyEventResult.handled;
      }
      final p = FocusManager.instance.primaryFocus;
      if (k == LogicalKeyboardKey.arrowDown &&
          (p == _scrubberFocus || p == _rootFocus)) {
        _playFocus.requestFocus();
        return KeyEventResult.handled;
      }
      if (k == LogicalKeyboardKey.arrowUp && p != _scrubberFocus) {
        _scrubberFocus.requestFocus();
        return KeyEventResult.handled;
      }
      return KeyEventResult.handled;
    }
    if (repeat) return KeyEventResult.ignored;
    if (k == LogicalKeyboardKey.keyF) {
      toggleFullscreen();
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.keyI || k == LogicalKeyboardKey.info) {
      osd.openPanel(osd.panel == OsdPanel.info ? OsdPanel.none : OsdPanel.info);
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.keyM) {
      final v = engine.state.value.volume;
      engine.setVolume(v > 0 ? 0 : 100);
      return KeyEventResult.handled;
    }
    osd.show();
    return KeyEventResult.ignored;
  }

  // ---- build ---------------------------------------------------------------

  @override
  Widget build(BuildContext context) {
    return PopScope(
      canPop: false,
      onPopInvokedWithResult: (didPop, _) {
        if (!didPop) handleBack();
      },
      child: Theme(
        data: ThemeData.dark(useMaterial3: true),
        child: Focus(
          focusNode: _rootFocus,
          autofocus: true,
          onKeyEvent: _onKey,
          child: Scaffold(
            backgroundColor: const Color(0xFF000000),
            body: MouseRegion(
              onHover: (_) => osd.show(),
              cursor: SystemMouseCursors.basic,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  GestureDetector(
                    behavior: HitTestBehavior.opaque,
                    onTap: osd.toggle,
                    onDoubleTapDown: (d) => _doubleTapAt = d.localPosition,
                    onDoubleTap: _onDoubleTap,
                    child: engine.buildVideo(context),
                  ),
                  _BufferingIndicator(engine: engine),
                  ListenableBuilder(
                    listenable: osd,
                    builder: (context, _) => _Osd(
                      state: this,
                      visible: osd.visible,
                      panel: osd.panel,
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
      ),
    );
  }

  void _onDoubleTap() {
    final at = _doubleTapAt;
    final w = MediaQuery.sizeOf(context).width;
    if (at == null || w <= 0) return;
    if (at.dx < w / 3) {
      seekBy(-widget.seekStep);
    } else if (at.dx > w * 2 / 3) {
      seekBy(widget.seekStep);
    } else {
      toggleFullscreen();
    }
  }
}

class _BufferingIndicator extends StatelessWidget {
  const _BufferingIndicator({required this.engine});
  final PlayerEngine engine;

  @override
  Widget build(BuildContext context) {
    return IgnorePointer(
      child: ValueListenableBuilder<EngineState>(
        valueListenable: engine.state,
        builder: (context, s, _) => s.buffering
            ? const Center(
                child: SizedBox(
                  width: 56,
                  height: 56,
                  child: CircularProgressIndicator(
                    strokeWidth: 3,
                    color: PaTokens.text,
                  ),
                ),
              )
            : const SizedBox.shrink(),
      ),
    );
  }
}

class _Osd extends StatelessWidget {
  const _Osd({required this.state, required this.visible, required this.panel});

  final PlayerScreenState state;
  final bool visible;
  final OsdPanel panel;

  @override
  Widget build(BuildContext context) {
    final engine = state.engine;
    final req = state.widget.request;
    final l = PaPlayerStrings.of(context);
    final compact = _isCompactOsd(MediaQuery.sizeOf(context).width);
    final side = compact ? 16.0 : 48.0;
    return Stack(
      fit: StackFit.expand,
      children: [
        IgnorePointer(
          ignoring: !visible,
          child: AnimatedOpacity(
            key: const ValueKey('osd'),
            opacity: visible ? 1 : 0,
            duration: PaTokens.osdFade,
            child: AnimatedSlide(
              offset: visible ? Offset.zero : const Offset(0, 0.02),
              duration: PaTokens.osdFade,
              child: Stack(
                fit: StackFit.expand,
                children: [
                  // scrims
                  const Positioned(
                    left: 0,
                    right: 0,
                    top: 0,
                    height: 160,
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          begin: Alignment.topCenter,
                          end: Alignment.bottomCenter,
                          colors: [Color(0xAA000000), Color(0x00000000)],
                        ),
                      ),
                    ),
                  ),
                  const Positioned(
                    left: 0,
                    right: 0,
                    bottom: 0,
                    height: 260,
                    child: DecoratedBox(
                      decoration: BoxDecoration(
                        gradient: LinearGradient(
                          begin: Alignment.bottomCenter,
                          end: Alignment.topCenter,
                          colors: [Color(0xCC000000), Color(0x00000000)],
                        ),
                      ),
                    ),
                  ),
                  Positioned(
                    left: compact ? 8 : 24,
                    right: compact ? 8 : 24,
                    top: 16,
                    child: SafeArea(
                      bottom: false,
                      child: Row(
                        children: [
                          FocusIconButton(
                            icon: Icons.arrow_back_rounded,
                            tooltip: l.back,
                            onPressed: state.exit,
                          ),
                          const SizedBox(width: 12),
                          Expanded(
                            child: Text(
                              req.title,
                              key: const ValueKey('osd-title'),
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                              style: const TextStyle(
                                color: PaTokens.text,
                                fontSize: 24,
                                fontWeight: FontWeight.w600,
                              ),
                            ),
                          ),
                        ],
                      ),
                    ),
                  ),
                  Positioned(
                    left: 0,
                    right: 0,
                    bottom: 0,
                    child: SafeArea(
                      top: false,
                      child: Padding(
                        padding: EdgeInsets.fromLTRB(side, 0, side, 28),
                        child: _BottomBar(
                          state: state,
                          engine: engine,
                          compact: compact,
                        ),
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        ),
        if (panel != OsdPanel.none)
          Positioned(
            top: 0,
            bottom: 0,
            right: 0,
            child: SafeArea(
              child: OsdSidePanel(
                panel: panel,
                engine: engine,
                request: req,
                osd: state.osd,
                onClose: state.closePanel,
              ),
            ),
          ),
      ],
    );
  }
}

/// Narrow windows (phones in portrait): the OSD bottom bar puts transport
/// and panel buttons on two centred rows instead of one.
bool _isCompactOsd(double width) => width < 600;

class _BottomBar extends StatelessWidget {
  const _BottomBar({
    required this.state,
    required this.engine,
    required this.compact,
  });
  final PlayerScreenState state;
  final PlayerEngine engine;
  final bool compact;

  @override
  Widget build(BuildContext context) {
    return ValueListenableBuilder<EngineState>(
      valueListenable: engine.state,
      builder: (context, s, _) {
        final remaining = s.duration - s.position;
        final rate = s.rate <= 0 ? 1.0 : s.rate;
        final endsAt = DateTime.now().add(
          Duration(milliseconds: (remaining.inMilliseconds / rate).round()),
        );
        final endsLabel =
            '${endsAt.hour.toString().padLeft(2, '0')}:${endsAt.minute.toString().padLeft(2, '0')}';
        final l = PaPlayerStrings.of(context);
        const gap = SizedBox(width: 8);
        final transport = <Widget>[
          FocusIconButton(
            key: const ValueKey('osd-back10'),
            icon: Icons.replay_10_rounded,
            tooltip: l.back10,
            onPressed: () => state.seekBy(-state.widget.seekStep),
          ),
          gap,
          FocusIconButton(
            key: const ValueKey('osd-play'),
            focusNode: state._playFocus,
            icon: s.playing ? Icons.pause_rounded : Icons.play_arrow_rounded,
            tooltip: s.playing ? l.pause : l.play,
            size: 60,
            onPressed: state.togglePlay,
          ),
          gap,
          FocusIconButton(
            key: const ValueKey('osd-fwd10'),
            icon: Icons.forward_10_rounded,
            tooltip: l.forward10,
            onPressed: () => state.seekBy(state.widget.seekStep),
          ),
        ];
        final panels = <Widget>[
          FocusIconButton(
            key: const ValueKey('osd-audio'),
            icon: Icons.graphic_eq_rounded,
            tooltip: l.audio,
            onPressed: () => state.osd.openPanel(OsdPanel.audio),
          ),
          gap,
          FocusIconButton(
            key: const ValueKey('osd-subtitles'),
            icon: Icons.subtitles_rounded,
            tooltip: l.subtitles,
            active: s.subtitleTrack != null && !s.subtitleTrack!.isOff,
            onPressed: () => state.osd.openPanel(OsdPanel.subtitles),
          ),
          gap,
          FocusIconButton(
            key: const ValueKey('osd-speed'),
            icon: Icons.speed_rounded,
            tooltip: l.speedWithRate(formatRate(s.rate)),
            active: s.rate != 1.0,
            onPressed: () => state.osd.openPanel(OsdPanel.speed),
          ),
          gap,
          FocusIconButton(
            key: const ValueKey('osd-info'),
            icon: Icons.info_outline_rounded,
            tooltip: l.info,
            onPressed: () => state.osd.openPanel(OsdPanel.info),
          ),
          if (state._isDesktop) ...[
            gap,
            FocusIconButton(
              icon: Icons.fullscreen_rounded,
              tooltip: l.fullscreen,
              onPressed: state.toggleFullscreen,
            ),
          ],
        ];
        return FocusTraversalGroup(
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.stretch,
            children: [
              Scrubber(
                key: const ValueKey('osd-scrubber'),
                focusNode: state._scrubberFocus,
                position: s.position,
                duration: s.duration,
                buffer: s.buffer,
                onSeek: state.seekTo,
                endLabel: s.duration > Duration.zero
                    ? l.remainingEndsAt(formatDuration(remaining), endsLabel)
                    : '',
              ),
              const SizedBox(height: 8),
              if (compact) ...[
                Row(
                  key: const ValueKey('osd-transport'),
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: transport,
                ),
                const SizedBox(height: 4),
                Row(
                  key: const ValueKey('osd-panels'),
                  mainAxisAlignment: MainAxisAlignment.center,
                  children: panels,
                ),
              ] else
                Row(children: [...transport, const Spacer(), ...panels]),
            ],
          ),
        );
      },
    );
  }
}
