// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import '../l10n/l10n.dart';
import '../services/app_services.dart';
import '../widgets/format.dart';
import 'library_lists_ui.dart';

/// Opens the fullscreen player (pa_player) on the root navigator and saves
/// progress to the library (PLAY-005: every 5 s, on pause, seek, exit and in
/// the background). Pass the saved position to resume, 0 to start over.
/// Episodes get Up Next: the next episode of the show plays after a
/// countdown, resuming where it was left.
Future<void> playVideo(
  BuildContext context,
  LibraryItem original, {
  int startPositionMs = 0,
}) async {
  // LIB-010: a movie with several versions asks which one to play.
  final item = await chooseVersion(context, original);
  if (item == null || !context.mounted) return;
  final services = Services.read(context);
  final l = context.l10n;
  final library = services.library;
  services.music.pause();
  await Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => PlayerScreen(
        request: _request(l, item, startPositionMs),
        engineFactory: services.playerEngineFactory,
        onProgress: _saver(library, item),
        loadNext: _nextLoader(l, library, item),
      ),
    ),
  );
}

PlayRequest _request(AppLocalizations l, LibraryItem item, int startMs) =>
    PlayRequest(
      uri: item.playbackUri,
      title: item.kind == ItemKind.episode
          ? '${episodeCode(l, item)}  ${item.title}'
          : item.title,
      startPositionMs: startMs,
      artworkUri: item.backdropUrl ?? item.posterUrl,
      durationMs: item.durationMs,
    );

void Function(int, int) _saver(Library library, LibraryItem item) =>
    (pos, dur) => library.saveProgress(
      Progress(
        itemId: item.id,
        positionMs: pos,
        durationMs: dur,
        updatedAt: DateTime.now(),
      ),
    );

/// Up Next for an episode: the next one of its show, from its saved
/// position if it is in progress.
Future<UpNextItem?> Function()? _nextLoader(
  AppLocalizations l,
  Library library,
  LibraryItem item,
) {
  if (item.kind != ItemKind.episode) return null;
  return () async {
    final next = await library.nextEpisode(item.id);
    if (next == null) return null;
    final start = resumePositionMs(await library.progress(next.id));
    return UpNextItem(
      request: _request(l, next, start),
      onProgress: _saver(library, next),
      loadNext: _nextLoader(l, library, next),
    );
  };
}

/// Starts [tracks] from [index] in the music player.
void playTracks(BuildContext context, List<LibraryItem> tracks, int index) {
  Services.read(context).music.play([
    for (final t in tracks)
      PlayRequest(
        uri: t.playbackUri,
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
