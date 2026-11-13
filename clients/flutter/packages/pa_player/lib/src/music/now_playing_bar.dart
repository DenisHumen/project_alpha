// SPDX-License-Identifier: MPL-2.0
import 'package:flutter/material.dart';

import '../l10n/pa_player_strings.dart';
import '../ui/focus_button.dart';
import '../ui/tokens.dart';
import 'artwork.dart';
import 'music_player_controller.dart';
import 'now_playing_screen.dart';

/// Spotify-style mini player for the bottom of the app. Hidden while the
/// queue is empty. Tapping it opens [NowPlayingScreen] (or [onOpen]).
class NowPlayingBar extends StatelessWidget {
  const NowPlayingBar({super.key, required this.controller, this.onOpen});

  final MusicPlayerController controller;
  final VoidCallback? onOpen;

  void _open(BuildContext context) {
    if (onOpen != null) {
      onOpen!();
      return;
    }
    Navigator.of(context).push(
      MaterialPageRoute<void>(
        fullscreenDialog: true,
        builder: (_) => NowPlayingScreen(controller: controller),
      ),
    );
  }

  @override
  Widget build(BuildContext context) {
    return ListenableBuilder(
      listenable: controller,
      builder: (context, _) {
        final t = controller.current;
        if (t == null) return const SizedBox.shrink();
        final l = PaPlayerStrings.of(context);
        final d = controller.duration.inMilliseconds;
        final f = d <= 0
            ? 0.0
            : (controller.position.inMilliseconds / d).clamp(0.0, 1.0);
        return Material(
          color: PaTokens.surface,
          child: InkWell(
            key: const ValueKey('now-playing-bar'),
            onTap: () => _open(context),
            child: SizedBox(
              height: 68,
              child: Column(
                children: [
                  LinearProgressIndicator(
                    value: f,
                    minHeight: 2,
                    backgroundColor: PaTokens.track,
                    color: PaTokens.text,
                  ),
                  Expanded(
                    child: Padding(
                      padding: const EdgeInsets.symmetric(horizontal: 12),
                      child: Row(
                        children: [
                          Artwork(request: t, size: 48, radius: 6),
                          const SizedBox(width: 12),
                          Expanded(
                            child: Column(
                              mainAxisAlignment: MainAxisAlignment.center,
                              crossAxisAlignment: CrossAxisAlignment.start,
                              children: [
                                Text(
                                  t.title,
                                  maxLines: 1,
                                  overflow: TextOverflow.ellipsis,
                                  style: const TextStyle(
                                    color: PaTokens.text,
                                    fontSize: 15,
                                    fontWeight: FontWeight.w600,
                                  ),
                                ),
                                if (t.artist != null)
                                  Text(
                                    t.artist!,
                                    maxLines: 1,
                                    overflow: TextOverflow.ellipsis,
                                    style: const TextStyle(
                                      color: PaTokens.textSecondary,
                                      fontSize: 13,
                                    ),
                                  ),
                              ],
                            ),
                          ),
                          FocusIconButton(
                            key: const ValueKey('bar-play'),
                            icon: controller.playing
                                ? Icons.pause_rounded
                                : Icons.play_arrow_rounded,
                            tooltip: controller.playing ? l.pause : l.play,
                            size: 44,
                            onPressed: controller.togglePlay,
                          ),
                          FocusIconButton(
                            key: const ValueKey('bar-next'),
                            icon: Icons.skip_next_rounded,
                            tooltip: l.next,
                            size: 44,
                            onPressed: controller.queue.nextPosition() == null
                                ? null
                                : controller.next,
                          ),
                        ],
                      ),
                    ),
                  ),
                ],
              ),
            ),
          ),
        );
      },
    );
  }
}
