// SPDX-License-Identifier: MPL-2.0

// Sidecar subtitles of a played file (PLAY-007). RustLibrary finds them in
// the core's folder groups (LIB-007); the v0 library has none (the player
// then lets libmpv pick up subtitles next to local files by itself).

import 'models.dart';

/// A subtitle file next to a video.
class SidecarSubtitle {
  /// What to hand to the player: a local path, or the loopback stream URL
  /// of a file on a network source (ADR-0038).
  final String uri;

  /// File name, for the track list.
  final String name;

  /// ISO 639-2 code (`rus`, `eng`) if the file name tells.
  final String? language;

  /// Forced subtitles: only the lines in a foreign language.
  final bool forced;

  /// SDH / hearing impaired / closed captions.
  final bool hearingImpaired;

  /// Marked as the default track in its name.
  final bool isDefault;

  const SidecarSubtitle({
    required this.uri,
    required this.name,
    this.language,
    this.forced = false,
    this.hearingImpaired = false,
    this.isDefault = false,
  });

  @override
  bool operator ==(Object other) =>
      other is SidecarSubtitle &&
      other.uri == uri &&
      other.name == name &&
      other.language == language &&
      other.forced == forced &&
      other.hearingImpaired == hearingImpaired &&
      other.isDefault == isDefault;

  @override
  int get hashCode =>
      Object.hash(uri, name, language, forced, hearingImpaired, isDefault);

  @override
  String toString() =>
      'SidecarSubtitle($name, $language${forced ? ', forced' : ''}'
      '${hearingImpaired ? ', sdh' : ''})';
}

/// A library that knows the subtitle files of its videos.
abstract interface class LibrarySubtitles {
  /// Subtitle files of the file [item] plays ([LibraryItem.playbackUri]:
  /// the item itself or a version picked with [LibraryItem.withFile]):
  /// those named after it first, then those of its movie folder. Empty if
  /// the file is not in the library.
  Future<List<SidecarSubtitle>> sidecarSubtitles(LibraryItem item);
}
