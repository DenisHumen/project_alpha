// SPDX-License-Identifier: MPL-2.0
//
// Library side of the playback session (PLAY-005): where to resume and what
// plays next. The rules mirror the core: `start_position` of `pa-playback`
// and `next_episode` of `pa-library` (LIB-011).

import 'library.dart' show continueWatchingMax, continueWatchingMin;
import 'models.dart';

/// Where "Resume" opens an item: the saved position while the item is in
/// progress (2 %..95 % of a known duration), else the beginning. With an
/// unknown duration a saved position is trusted. "Start over" always passes 0.
int resumePositionMs(Progress? p) {
  if (p == null || p.positionMs <= 0) return 0;
  if (p.durationMs <= 0) return p.positionMs;
  final f = p.positionMs / p.durationMs;
  return f >= continueWatchingMin && f <= continueWatchingMax
      ? p.positionMs
      : 0;
}

const int _noNumber = 1 << 30;

bool _isRegular(LibraryItem e) => (e.season ?? 1) >= 1;

int _compare(LibraryItem a, LibraryItem b) {
  final s = (a.season ?? 1).compareTo(b.season ?? 1);
  if (s != 0) return s;
  final e = (a.episode ?? _noNumber).compareTo(b.episode ?? _noNumber);
  if (e != 0) return e;
  final f = a.filePath.compareTo(b.filePath);
  return f != 0 ? f : a.id.compareTo(b.id);
}

/// The episode after [current] among [episodes] of its show (Up Next):
/// after a regular episode the next regular one across seasons (S1E10 →
/// S2E1), skipping specials; after a special (season 0) the next special.
/// Episodes of the same file (multi-episode files) are skipped.
LibraryItem? nextEpisodeIn(Iterable<LibraryItem> episodes, LibraryItem current) {
  final regular = _isRegular(current);
  final candidates =
      episodes
          .where(
            (e) =>
                e.kind == ItemKind.episode &&
                e.id != current.id &&
                e.filePath != current.filePath &&
                _isRegular(e) == regular &&
                _compare(e, current) > 0,
          )
          .toList()
        ..sort(_compare);
  return candidates.isEmpty ? null : candidates.first;
}
