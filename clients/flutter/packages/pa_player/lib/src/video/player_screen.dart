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
import '../subtitles/subtitle_overlay.dart';
import '../subtitles/subtitle_policy.dart';
import '../subtitles/subtitle_style.dart';
import '../ui/focus_button.dart';
import '../ui/scrubber.dart';
import '../ui/tokens.dart';
import 'osd_controller.dart';
import 'osd_panels.dart';
import 'progress_reporter.dart';
import 'up_next.dart';

/// Fullscreen video player with a tvOS-style OSD.
///
/// Texts come from [PaPlayerStrings.of] (English and Russian).
///
/// Controls: mouse/touch; keyboard (Space play/pause, ←/→ ±10 s, ↑/↓ show
/// OSD and move focus, F fullscreen, I info, Esc back); Android TV remote
/// (Select play/pause or activate, ←/→ seek, Back closes panel / exits).
///
/// Playback session (PLAY-005): progress is reported every
/// [progressInterval] while playing, on pause, seek, exit and when the app
/// goes to the background. With [loadNext] the end of the video shows an
/// Up Next card that counts [upNextCountdown] down and plays the next item
/// in the same player ([continuousPlayback]); Enter plays it now, Esc
/// cancels.
///
/// Subtitles (PLAY-007): the sidecars of [PlayRequest.subtitles] are loaded
/// on open and a track is picked by [pickSubtitle] (the series' remembered
/// choice, else the user's [subtitleLanguages] and forced tracks); the
/// overlay draws them in the [subtitleSettings] style, by default the
/// system caption style. A choice or delay made in the subtitles panel is
/// reported to [onSubtitleMemory] for the request's series and carries over
/// to the next episode.
class PlayerScreen extends StatefulWidget {
  const PlayerScreen({
    super.key,
    required this.request,
    required this.onProgress,
    this.onFinished,
    this.engineFactory,
    this.progressInterval = const Duration(seconds: 5),
    this.seekStep = const Duration(seconds: 10),
    this.loadNext,
    this.continuousPlayback = true,
    this.upNextCountdown = const Duration(seconds: 10),
    this.subtitleSettings,
    this.subtitleLanguages = const [],
    this.captionStyleLoader,
    this.onSubtitleMemory,
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

  /// Loads what plays after [request] (Up Next); null result: nothing.
  final Future<UpNextItem?> Function()? loadNext;

  /// Play the next item by itself after the countdown; off: the Up Next
  /// card waits for "Play now".
  final bool continuousPlayback;
  final Duration upNextCountdown;

  /// Subtitle style settings; null: the defaults (system style).
  final ValueListenable<SubtitleSettings>? subtitleSettings;

  /// Languages the user reads, best first (ISO 639 codes, e.g. the UI
  /// language). The system caption language comes first if set.
  final List<String> subtitleLanguages;

  /// Reads the system caption style; default [SystemCaptionStyle.read].
  final Future<SystemCaptionStyle?> Function()? captionStyleLoader;

  /// Called when the user picks a subtitle track or changes the delay of
  /// an item with a [PlayRequest.seriesKey].
  final void Function(String seriesKey, SubtitleMemory memory)?
  onSubtitleMemory;

  @override
  State<PlayerScreen> createState() => PlayerScreenState();
}

/// Public for tests.
class PlayerScreenState extends State<PlayerScreen>
    with WidgetsBindingObserver {
  late final PlayerEngine engine;
  ProgressReporter? _progress;
  late final UpNextController upNext;

  /// What is playing now: [PlayerScreen.request] or an Up Next item.
  late PlayRequest request;
  late ProgressCallback _onProgress;
  Future<UpNextItem?> Function()? _loadNext;
  UpNextItem? _next;
  int _nextGeneration = 0;
  bool _endHandled = false;
  final OsdController osd = OsdController(autoHide: PaTokens.osdAutoHide);
  final FocusNode _rootFocus = FocusNode(debugLabel: 'player-root');
  final FocusNode _scrubberFocus = FocusNode(debugLabel: 'player-scrubber');
  final FocusNode _playFocus = FocusNode(debugLabel: 'player-play');
  bool _finished = false;
  bool _subtitlesLoaded = false;

  /// Auto-selection of the subtitle track runs once per file, after its
  /// sidecars are in the track list (or after [_subtitleWait]).
  bool _autoSelectPending = false;
  Timer? _autoSelectTimer;
  int _subtitleGeneration = 0;
  static const _subtitleWait = Duration(milliseconds: 1500);

  /// System caption style (null: none on this platform).
  SystemCaptionStyle? systemCaptions;
  late Future<void> _captionsLoaded;

  /// Choice and delay made in this session, and the series they belong to:
  /// the next episode of the same series starts with them.
  SubtitleMemory? _sessionMemory;
  String? _sessionSeries;
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
    request = widget.request;
    _onProgress = widget.onProgress;
    _loadNext = widget.loadNext;
    upNext = UpNextController(
      onPlay: playNext,
      countdown: widget.upNextCountdown,
      continuous: widget.continuousPlayback,
    );
    _startProgress();
    engine.state.addListener(_onEngine);
    osd.addListener(_onOsd);
    widget.subtitleSettings?.addListener(_onSubtitleSettings);
    WidgetsBinding.instance.addObserver(this);
    _captionsLoaded = _loadCaptionStyle();
    _openCurrent();
    _fetchNext();
    if (!kIsWeb && (Platform.isAndroid || Platform.isIOS)) {
      SystemChrome.setEnabledSystemUIMode(SystemUiMode.immersiveSticky);
    }
  }

  /// Opens [request] in the engine with its subtitle delay.
  Future<void> _openCurrent() {
    _subtitleGeneration++;
    _autoSelectTimer?.cancel();
    _autoSelectPending = false;
    // sub-delay is an mpv option: set before the file loads, it applies to
    // it from the first cue.
    engine.setSubtitleDelay(_memoryFor(request)?.delay ?? Duration.zero);
    return engine.open(
      [request.uri],
      start: Duration(milliseconds: request.startPositionMs),
      autoLoadSubtitles: request.subtitles == null,
    );
  }

  Future<void> _loadCaptionStyle() async {
    final loader = widget.captionStyleLoader ?? SystemCaptionStyle.read;
    SystemCaptionStyle? style;
    try {
      style = await loader();
    } catch (_) {
      style = null;
    }
    if (!mounted) return;
    if (style != systemCaptions) setState(() => systemCaptions = style);
  }

  void _onSubtitleSettings() {
    if (mounted) setState(() {});
  }

  /// The subtitle look now.
  SubtitleAppearance get subtitleAppearance =>
      (widget.subtitleSettings?.value ?? const SubtitleSettings()).resolve(
        systemCaptions,
      );

  /// What is remembered for [r]'s series: this session's choice if it is
  /// the same series, else what the app stored.
  SubtitleMemory? _memoryFor(PlayRequest r) {
    final key = r.seriesKey;
    if (key != null && key == _sessionSeries) return _sessionMemory;
    return r.subtitleMemory;
  }

  SubtitlePreferences get subtitlePreferences => SubtitlePreferences(
    languages: [?systemCaptions?.locale, ...widget.subtitleLanguages],
    captionsOn: systemCaptions?.enabled ?? false,
  );

  /// Loads the sidecars of the file just opened and schedules the
  /// auto-selection of a track.
  Future<void> _loadSubtitles() async {
    final generation = _subtitleGeneration;
    final r = request;
    for (final sub in r.subtitles ?? const <ExternalSubtitle>[]) {
      await engine.addSubtitle(sub);
      if (generation != _subtitleGeneration) return;
    }
    if (r.subtitleUri != null) {
      // An explicit file of the request: shown as is.
      await engine.addExternalSubtitle(r.subtitleUri!);
      return;
    }
    if (!mounted || generation != _subtitleGeneration) return;
    _autoSelectPending = true;
    _autoSelectTimer?.cancel();
    _autoSelectTimer = Timer(
      (r.subtitles ?? const []).isEmpty
          ? const Duration(milliseconds: 300)
          : _subtitleWait,
      _autoSelect,
    );
    _checkSidecarsListed();
  }

  /// Auto-selects as soon as every sidecar is in the track list.
  void _checkSidecarsListed() {
    final subs = request.subtitles;
    if (!_autoSelectPending || subs == null || subs.isEmpty) return;
    final titles = {for (final t in engine.state.value.subtitleTracks) t.title};
    if (subs.every((s) => titles.contains(s.title))) _autoSelect();
  }

  Future<void> _autoSelect() async {
    if (!_autoSelectPending) return;
    _autoSelectPending = false;
    _autoSelectTimer?.cancel();
    final generation = _subtitleGeneration;
    await _captionsLoaded;
    if (!mounted || _closing || generation != _subtitleGeneration) return;
    final s = engine.state.value;
    final pick = pickSubtitle(
      s.subtitleTracks,
      preferences: subtitlePreferences,
      remembered: _memoryFor(request)?.choice,
      audioLanguage: s.audioTrack?.language,
    );
    if (pick != null) await engine.setSubtitleTrack(pick);
  }

  /// The user picked a subtitle track (or off) in the panel.
  Future<void> selectSubtitle(MediaTrack track) async {
    _autoSelectPending = false;
    _autoSelectTimer?.cancel();
    await engine.setSubtitleTrack(track);
    _remember(
      (_memoryFor(request) ?? const SubtitleMemory()).copyWith(
        choice: SubtitleChoice.of(track),
      ),
    );
  }

  /// The user loaded a subtitle file from the panel.
  Future<void> addSubtitleFile(String path) async {
    _autoSelectPending = false;
    _autoSelectTimer?.cancel();
    await engine.addExternalSubtitle(path);
  }

  /// Shifts subtitles; positive: later. Limited to
  /// ±[SubtitleMemory.maxDelayMs].
  Future<void> setSubtitleDelay(Duration delay) async {
    final ms = delay.inMilliseconds.clamp(
      -SubtitleMemory.maxDelayMs,
      SubtitleMemory.maxDelayMs,
    );
    final d = Duration(milliseconds: ms);
    await engine.setSubtitleDelay(d);
    _remember(
      (_memoryFor(request) ?? const SubtitleMemory()).copyWith(delay: d),
    );
  }

  void _remember(SubtitleMemory memory) {
    final key = request.seriesKey;
    _sessionMemory = memory;
    _sessionSeries = key;
    if (key != null) widget.onSubtitleMemory?.call(key, memory);
  }

  void _startProgress() {
    _progress = ProgressReporter(
      engine: engine,
      onProgress: _onProgress,
      interval: widget.progressInterval,
    );
  }

  /// Asks for the item after the current one; a late answer for an item
  /// that is no longer playing is dropped.
  Future<void> _fetchNext() async {
    _next = null;
    final load = _loadNext;
    if (load == null) return;
    final generation = ++_nextGeneration;
    UpNextItem? next;
    try {
      next = await load();
    } catch (_) {
      next = null;
    }
    if (!mounted || generation != _nextGeneration) return;
    _next = next;
    if (next != null && _endHandled && !upNext.visible && !_closing) {
      upNext.show(next);
    }
  }

  void _onEngine() {
    final s = engine.state.value;
    osd.playing = s.playing;
    if (!_subtitlesLoaded && s.duration > Duration.zero) {
      _subtitlesLoaded = true;
      _loadSubtitles();
    }
    _checkSidecarsListed();
    if (s.completed && !_finished && s.duration > Duration.zero) {
      _finished = true;
      _progress?.report(position: s.duration);
      osd.show();
      widget.onFinished?.call();
    }
    final ended = s.completed && s.duration > Duration.zero;
    if (ended && !_endHandled) {
      _endHandled = true;
      final next = _next;
      if (next != null && !_closing) upNext.show(next);
    } else if (!ended && _endHandled) {
      // Played again or seeking back: the card is no longer relevant.
      _endHandled = false;
      upNext.hide();
    }
  }

  /// Switches the player to [next] (Up Next): the old item gets its final
  /// report, the new one its own progress callback.
  void playNext(UpNextItem next) {
    if (_closing || !mounted) return;
    _progress?.close();
    _progress = null;
    setState(() {
      request = next.request;
      _onProgress = next.onProgress;
      _loadNext = next.loadNext;
      _finished = false;
      _subtitlesLoaded = false;
      _endHandled = false;
    });
    final generation = ++_nextGeneration;
    _openCurrent().then((_) {
      if (mounted && !_closing && generation == _nextGeneration) {
        _startProgress();
        _fetchNext();
      }
    });
    osd.show();
  }

  @override
  void didChangeAppLifecycleState(AppLifecycleState state) {
    if (state == AppLifecycleState.resumed) {
      // The caption settings may have changed while the app was away.
      _captionsLoaded = _loadCaptionStyle();
      return;
    }
    // Backgrounded, hidden or about to be killed: save now.
    _progress?.report();
    if (state != AppLifecycleState.inactive) upNext.stopCountdown();
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
    WidgetsBinding.instance.removeObserver(this);
    engine.state.removeListener(_onEngine);
    osd.removeListener(_onOsd);
    widget.subtitleSettings?.removeListener(_onSubtitleSettings);
    _autoSelectTimer?.cancel();
    _progress?.close();
    upNext.dispose();
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
    _progress?.onSeek(target);
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
    upNext.hide();
    _progress?.report();
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

    if (upNext.visible) {
      if (k == LogicalKeyboardKey.escape ||
          k == LogicalKeyboardKey.goBack ||
          k == LogicalKeyboardKey.browserBack) {
        if (!repeat) upNext.hide();
        return KeyEventResult.handled;
      }
      if (k == LogicalKeyboardKey.select ||
          k == LogicalKeyboardKey.enter ||
          k == LogicalKeyboardKey.numpadEnter ||
          k == LogicalKeyboardKey.gameButtonA ||
          k == LogicalKeyboardKey.mediaPlay ||
          k == LogicalKeyboardKey.mediaPlayPause) {
        if (!repeat) upNext.playNow();
        return KeyEventResult.handled;
      }
    }
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
                  ValueListenableBuilder<EngineState>(
                    valueListenable: engine.state,
                    builder: (context, s, _) => SubtitleOverlay(
                      lines: s.subtitleTrack?.isOff == true
                          ? const []
                          : s.subtitleLines,
                      appearance: subtitleAppearance,
                    ),
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
                  ListenableBuilder(
                    listenable: upNext,
                    builder: (context, _) => upNext.visible
                        ? _UpNextCard(controller: upNext)
                        : const SizedBox.shrink(),
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

/// The Up Next card, bottom right above the OSD bar.
class _UpNextCard extends StatelessWidget {
  const _UpNextCard({required this.controller});
  final UpNextController controller;

  @override
  Widget build(BuildContext context) {
    final l = PaPlayerStrings.of(context);
    final item = controller.item;
    if (item == null) return const SizedBox.shrink();
    final width = MediaQuery.sizeOf(context).width;
    final compact = _isCompactOsd(width);
    final seconds = (controller.remaining.inMilliseconds / 1000).ceil();
    return Positioned(
      right: compact ? 16 : 48,
      bottom: compact ? 168 : 132,
      child: SafeArea(
        child: Container(
          key: const ValueKey('up-next'),
          width: (width - 32).clamp(0.0, 360.0),
          padding: const EdgeInsets.all(16),
          decoration: BoxDecoration(
            color: PaTokens.glassStrong,
            borderRadius: BorderRadius.circular(PaTokens.radius),
          ),
          child: Column(
            mainAxisSize: MainAxisSize.min,
            crossAxisAlignment: CrossAxisAlignment.start,
            children: [
              Text(
                l.upNext,
                style: const TextStyle(
                  color: PaTokens.textSecondary,
                  fontSize: 13,
                ),
              ),
              const SizedBox(height: 4),
              Text(
                item.request.title,
                key: const ValueKey('up-next-title'),
                maxLines: 2,
                overflow: TextOverflow.ellipsis,
                style: const TextStyle(
                  color: PaTokens.text,
                  fontSize: 18,
                  fontWeight: FontWeight.w600,
                ),
              ),
              if (controller.phase == UpNextPhase.countdown) ...[
                const SizedBox(height: 4),
                Text(
                  l.upNextIn(seconds),
                  key: const ValueKey('up-next-countdown'),
                  style: const TextStyle(
                    color: PaTokens.textSecondary,
                    fontSize: 13,
                  ),
                ),
              ],
              const SizedBox(height: 12),
              Wrap(
                spacing: 8,
                runSpacing: 8,
                children: [
                  FilledButton.icon(
                    key: const ValueKey('up-next-play'),
                    onPressed: controller.playNow,
                    icon: const Icon(Icons.play_arrow_rounded),
                    label: Text(l.playNow),
                  ),
                  TextButton(
                    key: const ValueKey('up-next-cancel'),
                    onPressed: controller.hide,
                    child: Text(l.cancel),
                  ),
                ],
              ),
            ],
          ),
        ),
      ),
    );
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
    final req = state.request;
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
                onSelectSubtitle: state.selectSubtitle,
                onSubtitleFile: state.addSubtitleFile,
                onSubtitleDelay: state.setSubtitleDelay,
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
        final left = s.duration - s.position;
        final remaining = left.isNegative ? Duration.zero : left;
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
