// SPDX-License-Identifier: MPL-2.0
import 'dart:ui' show ImageFilter;

import 'package:flutter/material.dart';
import 'package:flutter/services.dart';

import '../engine/player_engine.dart';
import '../l10n/pa_player_strings.dart';
import '../ui/focus_button.dart';
import '../ui/scrubber.dart';
import '../ui/tokens.dart';
import 'artwork.dart';
import 'music_player_controller.dart';

/// Fullscreen "Now Playing": artwork, title/artist, scrubber, transport
/// (shuffle, previous, play/pause, next, repeat) and the queue.
/// Wide layouts (desktop/TV) put the artwork left and the queue right.
class NowPlayingScreen extends StatelessWidget {
  const NowPlayingScreen({super.key, required this.controller});

  final MusicPlayerController controller;

  KeyEventResult _onKey(FocusNode _, KeyEvent e) {
    if (e is! KeyDownEvent) return KeyEventResult.ignored;
    final k = e.logicalKey;
    if (k == LogicalKeyboardKey.space ||
        k == LogicalKeyboardKey.mediaPlayPause) {
      controller.togglePlay();
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.mediaTrackNext) {
      controller.next();
      return KeyEventResult.handled;
    }
    if (k == LogicalKeyboardKey.mediaTrackPrevious) {
      controller.previous();
      return KeyEventResult.handled;
    }
    return KeyEventResult.ignored;
  }

  @override
  Widget build(BuildContext context) {
    return Theme(
      data: ThemeData.dark(useMaterial3: true),
      child: Focus(
        onKeyEvent: _onKey,
        child: Scaffold(
          backgroundColor: PaTokens.bg,
          body: ListenableBuilder(
            listenable: controller,
            builder: (context, _) {
              final t = controller.current;
              return Stack(
                fit: StackFit.expand,
                children: [
                  // Blurred artwork backdrop.
                  if (t != null)
                    ImageFiltered(
                      imageFilter: ImageFilter.blur(sigmaX: 60, sigmaY: 60),
                      child: Opacity(
                        opacity: 0.45,
                        child: Artwork(request: t, radius: 0),
                      ),
                    ),
                  const DecoratedBox(
                    decoration: BoxDecoration(
                      gradient: LinearGradient(
                        begin: Alignment.topCenter,
                        end: Alignment.bottomCenter,
                        colors: [Color(0x66000000), Color(0xEE0B0B0F)],
                      ),
                    ),
                  ),
                  SafeArea(
                    child: LayoutBuilder(
                      builder: (context, c) => c.maxWidth >= 900
                          ? _Wide(controller: controller)
                          : _Narrow(controller: controller),
                    ),
                  ),
                ],
              );
            },
          ),
        ),
      ),
    );
  }
}

class _Wide extends StatelessWidget {
  const _Wide({required this.controller});
  final MusicPlayerController controller;

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.fromLTRB(64, 24, 64, 40),
      child: Column(
        children: [
          const _TopBar(),
          Expanded(
            child: Row(
              crossAxisAlignment: CrossAxisAlignment.center,
              children: [
                Expanded(
                  flex: 5,
                  child: Column(
                    mainAxisAlignment: MainAxisAlignment.center,
                    children: [
                      Flexible(child: _PulsingArtwork(controller: controller)),
                      const SizedBox(height: 32),
                      _Info(controller: controller),
                      const SizedBox(height: 16),
                      _Transport(controller: controller),
                    ],
                  ),
                ),
                const SizedBox(width: 64),
                Expanded(flex: 4, child: _Queue(controller: controller)),
              ],
            ),
          ),
        ],
      ),
    );
  }
}

class _Narrow extends StatelessWidget {
  const _Narrow({required this.controller});
  final MusicPlayerController controller;

  @override
  Widget build(BuildContext context) {
    return CustomScrollView(
      slivers: [
        const SliverToBoxAdapter(child: _TopBar()),
        SliverPadding(
          padding: const EdgeInsets.symmetric(horizontal: 24),
          sliver: SliverList.list(
            children: [
              const SizedBox(height: 8),
              Center(
                child: ConstrainedBox(
                  constraints: const BoxConstraints(maxWidth: 420),
                  child: _PulsingArtwork(controller: controller),
                ),
              ),
              const SizedBox(height: 28),
              _Info(controller: controller),
              const SizedBox(height: 12),
              _Transport(controller: controller),
              const SizedBox(height: 28),
              SizedBox(height: 480, child: _Queue(controller: controller)),
            ],
          ),
        ),
      ],
    );
  }
}

class _TopBar extends StatelessWidget {
  const _TopBar();

  @override
  Widget build(BuildContext context) {
    return Padding(
      padding: const EdgeInsets.all(8),
      child: Row(
        children: [
          FocusIconButton(
            icon: Icons.keyboard_arrow_down_rounded,
            tooltip: PaPlayerStrings.of(context).close,
            onPressed: () => Navigator.of(context).maybePop(),
          ),
          Expanded(
            child: Text(
              PaPlayerStrings.of(context).nowPlaying,
              textAlign: TextAlign.center,
              style: const TextStyle(
                color: PaTokens.textSecondary,
                fontSize: 13,
                letterSpacing: 1.2,
                fontWeight: FontWeight.w600,
              ),
            ),
          ),
          const SizedBox(width: 48),
        ],
      ),
    );
  }
}

/// Artwork that shrinks slightly while paused (Apple Music style).
class _PulsingArtwork extends StatelessWidget {
  const _PulsingArtwork({required this.controller});
  final MusicPlayerController controller;

  @override
  Widget build(BuildContext context) {
    return AnimatedScale(
      scale: controller.playing ? 1.0 : 0.86,
      duration: const Duration(milliseconds: 350),
      curve: Curves.easeOutBack,
      child: Container(
        decoration: BoxDecoration(
          borderRadius: BorderRadius.circular(12),
          boxShadow: const [
            BoxShadow(
              color: Color(0x80000000),
              blurRadius: 40,
              offset: Offset(0, 16),
            ),
          ],
        ),
        child: Artwork(request: controller.current, radius: 12),
      ),
    );
  }
}

class _Info extends StatelessWidget {
  const _Info({required this.controller});
  final MusicPlayerController controller;

  @override
  Widget build(BuildContext context) {
    final t = controller.current;
    final sub = [t?.artist, t?.album].whereType<String>().join(' — ');
    return Column(
      crossAxisAlignment: CrossAxisAlignment.stretch,
      children: [
        Text(
          t?.title ?? PaPlayerStrings.of(context).nothingPlaying,
          key: const ValueKey('np-title'),
          maxLines: 1,
          overflow: TextOverflow.ellipsis,
          style: const TextStyle(
            color: PaTokens.text,
            fontSize: 24,
            fontWeight: FontWeight.w700,
          ),
        ),
        if (sub.isNotEmpty)
          Text(
            sub,
            maxLines: 1,
            overflow: TextOverflow.ellipsis,
            style: const TextStyle(color: PaTokens.textSecondary, fontSize: 16),
          ),
        const SizedBox(height: 16),
        Scrubber(
          position: controller.position,
          duration: controller.duration,
          onSeek: controller.seek,
        ),
      ],
    );
  }
}

class _Transport extends StatelessWidget {
  const _Transport({required this.controller});
  final MusicPlayerController controller;

  @override
  Widget build(BuildContext context) {
    final l = PaPlayerStrings.of(context);
    final repeatIcon = controller.repeat == QueueRepeat.one
        ? Icons.repeat_one_rounded
        : Icons.repeat_rounded;
    return FocusTraversalGroup(
      child: Row(
        mainAxisAlignment: MainAxisAlignment.spaceBetween,
        children: [
          FocusIconButton(
            key: const ValueKey('np-shuffle'),
            icon: Icons.shuffle_rounded,
            tooltip: controller.shuffle ? l.shuffleOn : l.shuffleOff,
            active: controller.shuffle,
            onPressed: controller.toggleShuffle,
          ),
          FocusIconButton(
            key: const ValueKey('np-prev'),
            icon: Icons.skip_previous_rounded,
            tooltip: l.previous,
            size: 56,
            onPressed: controller.previous,
          ),
          FocusIconButton(
            key: const ValueKey('np-play'),
            icon: controller.playing
                ? Icons.pause_rounded
                : Icons.play_arrow_rounded,
            tooltip: controller.playing ? l.pause : l.play,
            size: 72,
            filled: true,
            autofocus: true,
            onPressed: controller.togglePlay,
          ),
          FocusIconButton(
            key: const ValueKey('np-next'),
            icon: Icons.skip_next_rounded,
            tooltip: l.next,
            size: 56,
            onPressed: controller.queue.nextPosition() == null
                ? null
                : controller.next,
          ),
          FocusIconButton(
            key: const ValueKey('np-repeat'),
            icon: repeatIcon,
            tooltip: switch (controller.repeat) {
              QueueRepeat.off => l.repeatOff,
              QueueRepeat.all => l.repeatAll,
              QueueRepeat.one => l.repeatOne,
            },
            active: controller.repeat != QueueRepeat.off,
            onPressed: controller.cycleRepeat,
          ),
        ],
      ),
    );
  }
}

class _Queue extends StatelessWidget {
  const _Queue({required this.controller});
  final MusicPlayerController controller;

  @override
  Widget build(BuildContext context) {
    final q = controller.queue;
    final ordered = q.ordered;
    final pos = q.position;
    final l = PaPlayerStrings.of(context);
    return FocusTraversalGroup(
      child: Column(
        crossAxisAlignment: CrossAxisAlignment.stretch,
        children: [
          Padding(
            padding: const EdgeInsets.fromLTRB(14, 0, 14, 8),
            child: Text(
              l.upNext,
              style: const TextStyle(
                color: PaTokens.text,
                fontSize: 18,
                fontWeight: FontWeight.w700,
              ),
            ),
          ),
          Expanded(
            child: ordered.length <= pos + 1
                ? Padding(
                    padding: const EdgeInsets.all(14),
                    child: Text(
                      l.endOfQueue,
                      style: const TextStyle(color: PaTokens.textSecondary),
                    ),
                  )
                : ListView.builder(
                    key: const ValueKey('np-queue'),
                    itemCount: ordered.length - pos - 1,
                    itemBuilder: (context, i) {
                      final index = pos + 1 + i;
                      final t = ordered[index];
                      return FocusListRow(
                        key: ValueKey('queue-$index'),
                        title: t.title,
                        subtitle: t.artist,
                        leading: Artwork(request: t, size: 40, radius: 4),
                        trailing: const SizedBox.shrink(),
                        onPressed: () => controller.jumpTo(index),
                      );
                    },
                  ),
          ),
        ],
      ),
    );
  }
}
