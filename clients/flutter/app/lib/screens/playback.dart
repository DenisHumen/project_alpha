// SPDX-License-Identifier: MPL-2.0

import 'package:material_ui/material_ui.dart';

import '../integration/library.dart';
import '../integration/player.dart';
import '../services/app_services.dart';
import '../widgets/format.dart';

/// Progress of [itemId] if it is in "Continue watching" (the contract exposes
/// progress only through that stream).
Future<Progress?> resumeProgressOf(Library library, String itemId) async {
  final list = await library.continueWatching().first;
  for (final (item, p) in list) {
    if (item.id == itemId) return p;
  }
  return null;
}

/// Opens the fullscreen player on the root navigator and saves progress.
Future<void> playVideo(BuildContext context, LibraryItem item, {int startPositionMs = 0}) async {
  final services = Services.read(context);
  final title = item.kind == ItemKind.episode ? '${episodeCode(item)}  ${item.title}' : item.title;
  services.music.pause();
  await Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => PlayerScreen(
        request: PlayRequest(uri: item.filePath, title: title, startPositionMs: startPositionMs),
        onProgress: (pos, dur) => services.library.saveProgress(
          Progress(itemId: item.id, positionMs: pos, durationMs: dur, updatedAt: DateTime.now()),
        ),
      ),
    ),
  );
}

/// Starts [tracks] from [index] in the music player.
void playTracks(BuildContext context, List<LibraryItem> tracks, int index) {
  final services = Services.read(context);
  services.trackSubtitles
    ..clear()
    ..addAll({for (final t in tracks) t.filePath: [t.artist, t.album].whereType<String>().join(' · ')});
  services.music.play([for (final t in tracks) PlayRequest(uri: t.filePath, title: t.title)], index: index);
}

void openNowPlaying(BuildContext context) {
  final services = Services.read(context);
  Navigator.of(context, rootNavigator: true).push(
    MaterialPageRoute<void>(
      fullscreenDialog: true,
      builder: (_) => NowPlayingScreen(
        controller: services.music,
        subtitle: services.trackSubtitles[services.music.current?.uri],
      ),
    ),
  );
}
