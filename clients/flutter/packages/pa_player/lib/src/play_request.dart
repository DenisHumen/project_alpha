// SPDX-License-Identifier: MPL-2.0

import 'subtitles/subtitle_policy.dart';

/// What to play. The first four fields are the release 0.1 contract
/// (docs/dev/release-0.1-contract.md); the rest are optional extras used by
/// the music UI.
class PlayRequest {
  const PlayRequest({
    required this.uri,
    required this.title,
    this.startPositionMs = 0,
    this.subtitleUri,
    this.artist,
    this.album,
    this.albumId,
    this.artworkUri,
    this.durationMs,
    this.subtitles,
    this.seriesKey,
    this.subtitleMemory,
  });

  /// File path, `file://` URI or any URI libmpv understands.
  final String uri;
  final String title;

  /// Resume position. `0` starts from the beginning.
  final int startPositionMs;

  /// External subtitle (SRT/ASS/VTT) to load on open.
  final String? subtitleUri;

  /// Music: track artist (shown under the title).
  final String? artist;

  /// Music: album name.
  final String? album;

  /// Music: library album id (MUS-001 `music_albums.id`). Groups tracks for
  /// album ReplayGain ([ReplayGainMode.auto]); falls back to [album].
  final String? albumId;

  /// Music: cover art (file path or http(s) URL).
  final String? artworkUri;

  /// Known duration (e.g. from the library) for display before the file opens.
  final int? durationMs;

  /// Sidecar subtitles from the library (PLAY-007), loaded on open. Null:
  /// unknown, the engine looks for subtitle files next to a local file by
  /// itself; empty: the library knows there are none.
  final List<ExternalSubtitle>? subtitles;

  /// Series the item belongs to: the subtitle choice and delay are kept
  /// per series ([PlayerScreen.onSubtitleMemory]). Null: not remembered.
  final String? seriesKey;

  /// What was remembered for [seriesKey].
  final SubtitleMemory? subtitleMemory;

  @override
  String toString() => 'PlayRequest($title, $uri)';
}

/// A subtitle file for the current video (a sidecar from the library).
class ExternalSubtitle {
  const ExternalSubtitle({
    required this.uri,
    required this.title,
    this.language,
    this.forced = false,
    this.hearingImpaired = false,
    this.isDefault = false,
  });

  /// Local path or URL (loopback stream proxy for network sources).
  final String uri;

  /// Track title; also identifies the loaded track, so it should be unique
  /// among the subtitles of a file (the file name is).
  final String title;

  /// ISO 639 code, if known.
  final String? language;
  final bool forced;
  final bool hearingImpaired;
  final bool isDefault;

  @override
  String toString() => 'ExternalSubtitle($title)';
}
