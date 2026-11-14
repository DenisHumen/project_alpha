// SPDX-License-Identifier: MPL-2.0

import '../integration/library.dart';
import '../l10n/l10n.dart';

String formatRuntime(AppLocalizations l, int? ms) {
  if (ms == null || ms <= 0) return '';
  final m = (ms / 60000).round();
  if (m < 60) return l.formatMinutes(m);
  return l.formatHoursMinutes(m ~/ 60, m % 60);
}

String formatClock(int ms) {
  final d = Duration(milliseconds: ms);
  final s = d.inSeconds % 60;
  return '${d.inMinutes}:${s.toString().padLeft(2, '0')}';
}

/// `1:23:45` from an hour on, `41:07` below.
String formatTimecode(int ms) {
  final d = Duration(milliseconds: ms < 0 ? 0 : ms);
  String two(int v) => v.toString().padLeft(2, '0');
  final s = two(d.inSeconds % 60);
  return d.inHours > 0
      ? '${d.inHours}:${two(d.inMinutes % 60)}:$s'
      : '${d.inMinutes}:$s';
}

String formatRemaining(AppLocalizations l, Progress p) {
  final left = ((p.durationMs - p.positionMs) / 60000).round();
  return left >= 60
      ? l.formatHoursMinutesLeft(left ~/ 60, left % 60)
      : l.formatMinutesLeft(left);
}

String episodeCode(AppLocalizations l, LibraryItem e) =>
    l.showEpisodeCode(e.season ?? 0, e.episode ?? 0);

/// Seed for generated album art: the folder of the track (shared by an album).
String albumSeedFor(LibraryItem track) {
  final i = track.filePath.lastIndexOf('/');
  return i > 0 ? track.filePath.substring(0, i) : (track.album ?? track.title);
}

/// Groups tracks into albums, preserving first-seen order.
List<Album> groupAlbums(AppLocalizations l, List<LibraryItem> tracks) {
  final map = <String, Album>{};
  for (final t in tracks) {
    final key = '${t.artist ?? ''}\u0000${t.album ?? ''}';
    map
        .putIfAbsent(
          key,
          () => Album(
            t.album ?? l.musicUnknownAlbum,
            t.artist ?? l.musicUnknownArtist,
            t.year,
            albumSeedFor(t),
            t.posterUrl,
          ),
        )
        .tracks
        .add(t);
  }
  for (final a in map.values) {
    a.tracks.sort((x, y) => (x.trackNumber ?? 0).compareTo(y.trackNumber ?? 0));
  }
  return map.values.toList();
}

class Album {
  Album(this.title, this.artist, this.year, this.artSeed, [this.artUrl]);
  final String title;
  final String? artUrl;
  final String artist;
  final int? year;
  final String artSeed;
  final List<LibraryItem> tracks = [];
}
