// SPDX-License-Identifier: MPL-2.0

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
    this.artworkUri,
    this.durationMs,
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

  /// Music: cover art (file path or http(s) URL).
  final String? artworkUri;

  /// Known duration (e.g. from the library) for display before the file opens.
  final int? durationMs;

  @override
  String toString() => 'PlayRequest($title, $uri)';
}
