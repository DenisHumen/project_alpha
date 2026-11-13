// SPDX-License-Identifier: MPL-2.0
//
// Stand-in for `package:pa_player` (R01-PLAYER) mirroring the API in
// docs/dev/release-0.1-contract.md. It does not decode media: it simulates a
// timeline so screens, progress saving and the mini player can be exercised.

import 'dart:async';
import 'dart:math' as math;

import 'package:flutter/services.dart';
import 'package:material_ui/material_ui.dart';

import '../widgets/artwork.dart';

class PlayRequest {
  PlayRequest({
    required this.uri,
    required this.title,
    this.startPositionMs = 0,
    this.subtitleUri,
  });

  String uri;
  String title;
  int startPositionMs;
  String? subtitleUri;
}

String _fmt(Duration d) {
  final h = d.inHours, m = d.inMinutes % 60, s = d.inSeconds % 60;
  String two(int v) => v.toString().padLeft(2, '0');
  return h > 0 ? '$h:${two(m)}:${two(s)}' : '$m:${two(s)}';
}

/// Fullscreen video player (fake: simulated 45-minute timeline).
class PlayerScreen extends StatefulWidget {
  const PlayerScreen({
    super.key,
    required this.request,
    required this.onProgress,
    this.onFinished,
  });

  final PlayRequest request;
  final void Function(int positionMs, int durationMs) onProgress;
  final VoidCallback? onFinished;

  @override
  State<PlayerScreen> createState() => _PlayerScreenState();
}

class _PlayerScreenState extends State<PlayerScreen> {
  static const _duration = Duration(minutes: 45);
  late Duration _pos = Duration(milliseconds: widget.request.startPositionMs);
  bool _playing = true;
  bool _osd = true;
  Timer? _tick;
  Timer? _hideOsd;
  int _sinceReport = 0;
  final _focus = FocusNode();

  @override
  void initState() {
    super.initState();
    _tick = Timer.periodic(const Duration(seconds: 1), (_) {
      if (!_playing) return;
      setState(() => _pos += const Duration(seconds: 1));
      if (++_sinceReport >= 5) _report();
      if (_pos >= _duration) {
        _report();
        widget.onFinished?.call();
        Navigator.of(context).maybePop();
      }
    });
    _poke();
  }

  void _report() {
    _sinceReport = 0;
    widget.onProgress(_pos.inMilliseconds, _duration.inMilliseconds);
  }

  void _poke() {
    setState(() => _osd = true);
    _hideOsd?.cancel();
    _hideOsd = Timer(const Duration(seconds: 4), () {
      if (mounted && _playing) setState(() => _osd = false);
    });
  }

  void _toggle() {
    setState(() => _playing = !_playing);
    _report();
    _poke();
  }

  void _seek(int seconds) {
    setState(() {
      final ms = (_pos.inMilliseconds + seconds * 1000).clamp(
        0,
        _duration.inMilliseconds,
      );
      _pos = Duration(milliseconds: ms);
    });
    _report();
    _poke();
  }

  @override
  void dispose() {
    _report();
    _tick?.cancel();
    _hideOsd?.cancel();
    _focus.dispose();
    super.dispose();
  }

  KeyEventResult _onKey(FocusNode node, KeyEvent e) {
    if (e is! KeyDownEvent && e is! KeyRepeatEvent) {
      return KeyEventResult.ignored;
    }
    final k = e.logicalKey;
    if (k == LogicalKeyboardKey.space ||
        k == LogicalKeyboardKey.select ||
        k == LogicalKeyboardKey.enter ||
        k == LogicalKeyboardKey.mediaPlayPause) {
      _toggle();
    } else if (k == LogicalKeyboardKey.arrowLeft ||
        k == LogicalKeyboardKey.mediaRewind) {
      _seek(-10);
    } else if (k == LogicalKeyboardKey.arrowRight ||
        k == LogicalKeyboardKey.mediaFastForward) {
      _seek(30);
    } else if (k == LogicalKeyboardKey.escape ||
        k == LogicalKeyboardKey.goBack) {
      Navigator.of(context).maybePop();
    } else {
      _poke();
      return KeyEventResult.ignored;
    }
    return KeyEventResult.handled;
  }

  @override
  Widget build(BuildContext context) {
    final f = _pos.inMilliseconds / _duration.inMilliseconds;
    final theme = Theme.of(context);
    return Scaffold(
      backgroundColor: Colors.black,
      body: Focus(
        focusNode: _focus,
        autofocus: true,
        onKeyEvent: _onKey,
        child: MouseRegion(
          onHover: (_) => _poke(),
          child: GestureDetector(
            onTap: _poke,
            onDoubleTap: _toggle,
            child: Stack(
              fit: StackFit.expand,
              children: [
                Opacity(
                  opacity: .55,
                  child: GeneratedArt(
                    seed: widget.request.uri,
                    style: ArtStyle.backdrop,
                  ),
                ),
                Center(
                  child: Column(
                    mainAxisSize: MainAxisSize.min,
                    children: [
                      Icon(
                        _playing ? Icons.graphic_eq : Icons.pause,
                        size: 48,
                        color: Colors.white70,
                      ),
                      const SizedBox(height: 12),
                      Text('Preview player', style: theme.textTheme.titleLarge),
                      Text(
                        'Real playback comes with pa_player (media_kit)',
                        style: theme.textTheme.bodySmall,
                      ),
                    ],
                  ),
                ),
                AnimatedOpacity(
                  opacity: _osd ? 1 : 0,
                  duration: const Duration(milliseconds: 250),
                  child: DecoratedBox(
                    decoration: const BoxDecoration(
                      gradient: LinearGradient(
                        begin: Alignment.topCenter,
                        end: Alignment.bottomCenter,
                        colors: [
                          Color(0xAA000000),
                          Colors.transparent,
                          Colors.transparent,
                          Color(0xCC000000),
                        ],
                        stops: [0, .25, .6, 1],
                      ),
                    ),
                    child: SafeArea(
                      child: Padding(
                        padding: const EdgeInsets.fromLTRB(32, 16, 32, 28),
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Row(
                              children: [
                                IconButton(
                                  tooltip: 'Back',
                                  onPressed: () =>
                                      Navigator.of(context).maybePop(),
                                  icon: const Icon(Icons.arrow_back),
                                ),
                                const SizedBox(width: 8),
                                Expanded(
                                  child: Text(
                                    widget.request.title,
                                    style: theme.textTheme.titleLarge,
                                    overflow: TextOverflow.ellipsis,
                                  ),
                                ),
                              ],
                            ),
                            const Spacer(),
                            ClipRRect(
                              borderRadius: BorderRadius.circular(4),
                              child: LinearProgressIndicator(
                                value: f.clamp(0, 1),
                                minHeight: 5,
                              ),
                            ),
                            const SizedBox(height: 10),
                            Row(
                              children: [
                                Text(
                                  _fmt(_pos),
                                  style: theme.textTheme.labelMedium,
                                ),
                                const Spacer(),
                                IconButton(
                                  tooltip: 'Back 10 s',
                                  onPressed: () => _seek(-10),
                                  icon: const Icon(Icons.replay_10),
                                ),
                                IconButton(
                                  tooltip: _playing ? 'Pause' : 'Play',
                                  iconSize: 40,
                                  onPressed: _toggle,
                                  icon: Icon(
                                    _playing
                                        ? Icons.pause_circle_filled
                                        : Icons.play_circle_filled,
                                  ),
                                ),
                                IconButton(
                                  tooltip: 'Forward 30 s',
                                  onPressed: () => _seek(30),
                                  icon: const Icon(Icons.forward_30),
                                ),
                                const Spacer(),
                                Text(
                                  '-${_fmt(_duration - _pos)}',
                                  style: theme.textTheme.labelMedium,
                                ),
                              ],
                            ),
                          ],
                        ),
                      ),
                    ),
                  ),
                ),
              ],
            ),
          ),
        ),
      ),
    );
  }
}

/// Queue state (fake: every track is 3:20 long).
class MusicQueue {
  MusicQueue(this.items, this.index);

  final List<PlayRequest> items;
  int index;
  bool shuffle = false;
  bool repeat = false;

  PlayRequest? get current => items.isEmpty ? null : items[index];
}

class MusicPlayerController extends ChangeNotifier {
  static const trackLength = Duration(seconds: 200);

  MusicQueue queue = MusicQueue(const [], 0);
  bool playing = false;
  Duration position = Duration.zero;
  Duration get duration => trackLength;
  Timer? _timer;

  PlayRequest? get current => queue.current;

  Future<void> play(List<PlayRequest> items, {int index = 0}) async {
    queue = MusicQueue(List.of(items), index);
    position = Duration(milliseconds: items[index].startPositionMs);
    resume();
  }

  void pause() {
    playing = false;
    _timer?.cancel();
    notifyListeners();
  }

  void resume() {
    if (queue.items.isEmpty) return;
    playing = true;
    _timer?.cancel();
    _timer = Timer.periodic(const Duration(seconds: 1), (_) {
      position += const Duration(seconds: 1);
      if (position >= trackLength) {
        next();
      } else {
        notifyListeners();
      }
    });
    notifyListeners();
  }

  void next() {
    if (queue.items.isEmpty) return;
    if (queue.index < queue.items.length - 1) {
      queue.index++;
    } else if (queue.repeat) {
      queue.index = 0;
    } else {
      pause();
    }
    position = Duration.zero;
    notifyListeners();
  }

  void previous() {
    if (position > const Duration(seconds: 3) || queue.index == 0) {
      position = Duration.zero;
    } else {
      queue.index--;
      position = Duration.zero;
    }
    notifyListeners();
  }

  void seek(Duration d) {
    position = Duration(
      milliseconds: d.inMilliseconds.clamp(0, trackLength.inMilliseconds),
    );
    notifyListeners();
  }

  /// Stops playback and clears the queue (hides the mini player).
  void stop() {
    _timer?.cancel();
    playing = false;
    queue = MusicQueue(const [], 0);
    position = Duration.zero;
    notifyListeners();
  }

  @override
  void dispose() {
    _timer?.cancel();
    super.dispose();
  }
}

/// Mini player shown at the bottom of the shell (Spotify-like).
class NowPlayingBar extends StatelessWidget {
  const NowPlayingBar({
    super.key,
    required this.controller,
    this.onOpen,
    this.subtitle,
  });

  final MusicPlayerController controller;
  final VoidCallback? onOpen;
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final cur = controller.current;
        if (cur == null) return const SizedBox.shrink();
        final theme = Theme.of(context);
        final f =
            controller.position.inMilliseconds /
            controller.duration.inMilliseconds;
        return Material(
          color: const Color(0xF21F1F27),
          borderRadius: BorderRadius.circular(14),
          clipBehavior: Clip.antiAlias,
          child: InkWell(
            onTap: onOpen,
            child: Column(
              mainAxisSize: MainAxisSize.min,
              children: [
                Padding(
                  padding: const EdgeInsets.fromLTRB(8, 8, 4, 6),
                  child: Row(
                    children: [
                      ClipRRect(
                        borderRadius: BorderRadius.circular(6),
                        child: SizedBox.square(
                          dimension: 40,
                          child: GeneratedArt(
                            seed: _albumSeed(cur),
                            style: ArtStyle.album,
                          ),
                        ),
                      ),
                      const SizedBox(width: 12),
                      Expanded(
                        child: Column(
                          crossAxisAlignment: CrossAxisAlignment.start,
                          children: [
                            Text(
                              cur.title,
                              style: theme.textTheme.titleSmall,
                              maxLines: 1,
                              overflow: TextOverflow.ellipsis,
                            ),
                            if (subtitle != null)
                              Text(
                                subtitle!,
                                style: theme.textTheme.labelSmall,
                                maxLines: 1,
                                overflow: TextOverflow.ellipsis,
                              ),
                          ],
                        ),
                      ),
                      IconButton(
                        tooltip: 'Previous',
                        onPressed: controller.previous,
                        icon: const Icon(Icons.skip_previous),
                      ),
                      IconButton(
                        tooltip: controller.playing ? 'Pause' : 'Play',
                        onPressed: controller.playing
                            ? controller.pause
                            : controller.resume,
                        icon: Icon(
                          controller.playing ? Icons.pause : Icons.play_arrow,
                        ),
                      ),
                      IconButton(
                        tooltip: 'Next',
                        onPressed: controller.next,
                        icon: const Icon(Icons.skip_next),
                      ),
                    ],
                  ),
                ),
                LinearProgressIndicator(value: f.clamp(0, 1), minHeight: 2),
              ],
            ),
          ),
        );
      },
    );
  }
}

/// Full "now playing" screen.
class NowPlayingScreen extends StatelessWidget {
  const NowPlayingScreen({super.key, required this.controller, this.subtitle});

  final MusicPlayerController controller;
  final String? subtitle;

  @override
  Widget build(BuildContext context) {
    final theme = Theme.of(context);
    return Scaffold(
      body: ListenableBuilder(
        listenable: controller,
        builder: (context, _) {
          final cur = controller.current;
          if (cur == null) {
            return const Center(child: Text('Nothing is playing'));
          }
          final seed = _albumSeed(cur);
          return Stack(
            fit: StackFit.expand,
            children: [
              Opacity(
                opacity: .35,
                child: GeneratedArt(seed: seed, style: ArtStyle.album),
              ),
              const DecoratedBox(
                decoration: BoxDecoration(color: Color(0xB30B0B0F)),
              ),
              SafeArea(
                child: LayoutBuilder(
                  builder: (context, c) {
                    final art = math
                        .min(c.maxWidth * .7, c.maxHeight * .45)
                        .clamp(120.0, 420.0);
                    return Column(
                      children: [
                        Align(
                          alignment: Alignment.centerLeft,
                          child: IconButton(
                            tooltip: 'Close',
                            onPressed: () => Navigator.of(context).maybePop(),
                            icon: const Icon(
                              Icons.keyboard_arrow_down,
                              size: 32,
                            ),
                          ),
                        ),
                        const Spacer(),
                        ClipRRect(
                          borderRadius: BorderRadius.circular(16),
                          child: SizedBox.square(
                            dimension: art,
                            child: GeneratedArt(
                              seed: seed,
                              style: ArtStyle.album,
                            ),
                          ),
                        ),
                        const SizedBox(height: 28),
                        Text(
                          cur.title,
                          style: theme.textTheme.headlineMedium,
                          textAlign: TextAlign.center,
                        ),
                        if (subtitle != null)
                          Text(subtitle!, style: theme.textTheme.bodySmall),
                        const SizedBox(height: 20),
                        SizedBox(
                          width: math.min(520, c.maxWidth - 48),
                          child: Column(
                            children: [
                              Slider(
                                value: controller.position.inMilliseconds
                                    .toDouble(),
                                max: controller.duration.inMilliseconds
                                    .toDouble(),
                                onChanged: (v) => controller.seek(
                                  Duration(milliseconds: v.round()),
                                ),
                              ),
                              Row(
                                children: [
                                  Text(
                                    _fmt(controller.position),
                                    style: theme.textTheme.labelSmall,
                                  ),
                                  const Spacer(),
                                  Text(
                                    _fmt(controller.duration),
                                    style: theme.textTheme.labelSmall,
                                  ),
                                ],
                              ),
                            ],
                          ),
                        ),
                        Row(
                          mainAxisAlignment: MainAxisAlignment.center,
                          children: [
                            IconButton(
                              tooltip: 'Shuffle',
                              onPressed: () {
                                controller.queue.shuffle =
                                    !controller.queue.shuffle;
                                controller.seek(controller.position);
                              },
                              icon: Icon(
                                Icons.shuffle,
                                color: controller.queue.shuffle
                                    ? theme.colorScheme.primary
                                    : null,
                              ),
                            ),
                            IconButton(
                              iconSize: 36,
                              tooltip: 'Previous',
                              onPressed: controller.previous,
                              icon: const Icon(Icons.skip_previous),
                            ),
                            IconButton(
                              iconSize: 64,
                              autofocus: true,
                              tooltip: controller.playing ? 'Pause' : 'Play',
                              onPressed: controller.playing
                                  ? controller.pause
                                  : controller.resume,
                              icon: Icon(
                                controller.playing
                                    ? Icons.pause_circle_filled
                                    : Icons.play_circle_filled,
                              ),
                            ),
                            IconButton(
                              iconSize: 36,
                              tooltip: 'Next',
                              onPressed: controller.next,
                              icon: const Icon(Icons.skip_next),
                            ),
                            IconButton(
                              tooltip: 'Repeat',
                              onPressed: () {
                                controller.queue.repeat =
                                    !controller.queue.repeat;
                                controller.seek(controller.position);
                              },
                              icon: Icon(
                                Icons.repeat,
                                color: controller.queue.repeat
                                    ? theme.colorScheme.primary
                                    : null,
                              ),
                            ),
                          ],
                        ),
                        const Spacer(flex: 2),
                      ],
                    );
                  },
                ),
              ),
            ],
          );
        },
      ),
    );
  }
}

/// Album art seed: tracks of one album share the folder part of the path.
String _albumSeed(PlayRequest r) {
  final i = r.uri.lastIndexOf('/');
  return i > 0 ? r.uri.substring(0, i) : r.uri;
}
