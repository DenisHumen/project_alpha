// SPDX-License-Identifier: MPL-2.0

import '../integration/library.dart';

String formatRuntime(int? ms) {
  if (ms == null || ms <= 0) return '';
  final m = (ms / 60000).round();
  if (m < 60) return '$m min';
  return '${m ~/ 60} h ${m % 60} min';
}

String formatClock(int ms) {
  final d = Duration(milliseconds: ms);
  final s = d.inSeconds % 60;
  return '${d.inMinutes}:${s.toString().padLeft(2, '0')}';
}

String formatRemaining(Progress p) {
  final left = ((p.durationMs - p.positionMs) / 60000).round();
  return left >= 60
      ? '${left ~/ 60} h ${left % 60} min left'
      : '$left min left';
}

String episodeCode(LibraryItem e) => 'S${e.season ?? 0} · E${e.episode ?? 0}';

/// Seed for generated album art: the folder of the track (shared by an album).
String albumSeedFor(LibraryItem track) {
  final i = track.filePath.lastIndexOf('/');
  return i > 0 ? track.filePath.substring(0, i) : (track.album ?? track.title);
}

/// Groups tracks into albums, preserving first-seen order.
List<Album> groupAlbums(List<LibraryItem> tracks) {
  final map = <String, Album>{};
  for (final t in tracks) {
    final key = '${t.artist ?? ''}\u0000${t.album ?? 'Unknown album'}';
    map
        .putIfAbsent(
          key,
          () => Album(
            t.album ?? 'Unknown album',
            t.artist ?? 'Unknown artist',
            t.year,
            albumSeedFor(t),
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
  Album(this.title, this.artist, this.year, this.artSeed);
  final String title;
  final String artist;
  final int? year;
  final String artSeed;
  final List<LibraryItem> tracks = [];
}
