// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import '../services/app_services.dart';
import '../widgets/format.dart';

/// Opens the fullscreen player (pa_player) on the root navigator and saves
/// progress to the library.
Future<void> playVideo(
  BuildContext context,
  LibraryItem item, {
  int startPositionMs = 0,
}) async {
  final services = Services.read(context);
  final title = item.kind == ItemKind.episode
      ? '${episodeCode(item)}  ${item.title}'
      : item.title;
  services.music.pause();
  await Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => PlayerScreen(
        request: PlayRequest(
          uri: item.filePath,
          title: title,
          startPositionMs: startPositionMs,
          artworkUri: item.backdropUrl ?? item.posterUrl,
          durationMs: item.durationMs,
        ),
        engineFactory: services.playerEngineFactory,
        onProgress: (pos, dur) => services.library.saveProgress(
          Progress(
            itemId: item.id,
            positionMs: pos,
            durationMs: dur,
            updatedAt: DateTime.now(),
          ),
        ),
      ),
    ),
  );
}

/// Starts [tracks] from [index] in the music player.
void playTracks(BuildContext context, List<LibraryItem> tracks, int index) {
  Services.read(context).music.play([
    for (final t in tracks)
      PlayRequest(
        uri: t.filePath,
        title: t.title,
        artist: t.artist,
        album: t.album,
        artworkUri: t.posterUrl,
        durationMs: t.durationMs,
      ),
  ], index: index);
}

void openNowPlaying(BuildContext context) {
  final services = Services.read(context);
  Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => NowPlayingScreen(controller: services.music),
    ),
  );
}
