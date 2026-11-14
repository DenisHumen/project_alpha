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
///
/// Subtitles (PLAY-007): the sidecar files the library knows for the file
/// are passed to the player (network ones through the stream proxy), and
/// the subtitle track and delay are remembered per show.
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
  final request = await _request(services, l, item, startPositionMs);
  if (!context.mounted) return;
  await Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => PlayerScreen(
        request: request,
        engineFactory: services.playerEngineFactory,
        onProgress: _saver(library, item),
        loadNext: _nextLoader(services, l, item),
        subtitleSettings: services.subtitleSettings,
        subtitleLanguages: services.subtitleLanguages,
        onSubtitleMemory: services.saveSubtitleMemory,
      ),
    ),
  );
}

Future<PlayRequest> _request(
  AppServices services,
  AppLocalizations l,
  LibraryItem item,
  int startMs,
) async {
  final series = item.kind == ItemKind.episode ? item.showId : null;
  return PlayRequest(
    uri: item.playbackUri,
    title: item.kind == ItemKind.episode
        ? '${episodeCode(l, item)}  ${item.title}'
        : item.title,
    startPositionMs: startMs,
    artworkUri: item.backdropUrl ?? item.posterUrl,
    durationMs: item.durationMs,
    subtitles: await sidecarSubtitles(services.library, item),
    seriesKey: series,
    subtitleMemory: series == null
        ? null
        : await services.subtitleMemory(series),
  );
}

/// Subtitle files of [item]'s file from the library, for the player; null
/// if the library cannot tell (the player then looks next to local files
/// itself).
Future<List<ExternalSubtitle>?> sidecarSubtitles(
  Library library,
  LibraryItem item,
) async {
  if (library is! LibrarySubtitles) return null;
  try {
    final subs = await library.sidecarSubtitles(item);
    return [
      for (final s in subs)
        ExternalSubtitle(
          uri: s.uri,
          title: s.name,
          language: s.language,
          forced: s.forced,
          hearingImpaired: s.hearingImpaired,
          isDefault: s.isDefault,
        ),
    ];
  } catch (e) {
    debugPrint('Sidecar subtitles of ${item.id} not read: $e');
    return null;
  }
}

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
  AppServices services,
  AppLocalizations l,
  LibraryItem item,
) {
  if (item.kind != ItemKind.episode) return null;
  final library = services.library;
  return () async {
    final next = await library.nextEpisode(item.id);
    if (next == null) return null;
    final start = resumePositionMs(await library.progress(next.id));
    return UpNextItem(
      request: await _request(services, l, next, start),
      onProgress: _saver(library, next),
      loadNext: _nextLoader(services, l, next),
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
