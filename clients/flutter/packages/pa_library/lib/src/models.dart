// SPDX-License-Identifier: MPL-2.0

/// Kind of a media source.
/// Kind of a [MediaSource]. `webdav` exists only on the Rust core.
enum SourceKind { localFolder, smb, webdav }

/// A place the library scans for media files.
class MediaSource {
  /// Stable id. Pass an empty string to [Library.addSource] to get one generated.
  final String id;
  final SourceKind kind;

  /// Human-readable name shown in settings.
  final String name;

  /// localFolder: absolute path. smb: `smb://host/share/path`.
  final String path;

  /// SMB user name (the password is kept by the app in secure storage and
  /// passed to [Library.scan]).
  final String? username;

  const MediaSource({
    required this.id,
    required this.kind,
    required this.name,
    required this.path,
    this.username,
  });

  MediaSource copyWith({
    String? id,
    String? name,
    String? path,
    String? username,
  }) => MediaSource(
    id: id ?? this.id,
    kind: kind,
    name: name ?? this.name,
    path: path ?? this.path,
    username: username ?? this.username,
  );

  @override
  bool operator ==(Object other) =>
      other is MediaSource &&
      other.id == id &&
      other.kind == kind &&
      other.name == name &&
      other.path == path &&
      other.username == username;

  @override
  int get hashCode => Object.hash(id, kind, name, path, username);

  @override
  String toString() => 'MediaSource($id, $kind, $name, $path)';
}

/// Kind of a library item.
enum ItemKind { movie, show, episode, track }

/// Why the player cannot play an item's file (LIB-017, LIB-020). The app
/// shows the reason instead of opening the player.
enum Unplayable {
  /// A disc image (`.iso`): the LGPL player build has no disc navigation.
  discImage,

  /// A reason this app version does not know (a newer core).
  other;

  /// From the core's stable code (`disc_image`); null for none.
  static Unplayable? fromCode(String? code) => switch (code) {
    null || '' => null,
    'disc_image' => Unplayable.discImage,
    _ => Unplayable.other,
  };
}

/// One entry of the library: a movie, a show (group of episodes), an episode
/// or a music track.
class LibraryItem {
  final String id;
  final ItemKind kind;
  final String title;
  final String? originalTitle;
  final int? year;
  final String? overview;

  /// `https://image.tmdb.org/...` or a `file://` URI of a local image.
  final String? posterUrl;
  final String? backdropUrl;

  /// For episodes: id of the parent show item.
  final String? showId;
  final int? season;
  final int? episode;

  /// For tracks.
  final String? artist;
  final String? album;
  final int? trackNumber;

  /// Absolute path of the media file; for files of network sources the
  /// source address and the path inside it (`smb://nas/media/Movies/x.mkv`),
  /// for display. For shows: the show folder (or the folder of its first
  /// episode). Play [playbackUri], not this.
  final String filePath;

  /// What the player opens when it is not [filePath]: the loopback stream
  /// URL of a file on a network source (LIB-018). Contains a per-process
  /// secret: do not show or log it.
  final String? streamUri;

  /// The URI to hand to the player.
  String get playbackUri => streamUri ?? filePath;
  final int? durationMs;

  /// 0..1; how sure we are about the recognition (TMDB match or parser).
  final double? matchConfidence;

  /// TMDB id of the movie / show (null if not matched).
  final int? tmdbId;

  /// When the item was first added to the library.
  final DateTime? addedAt;

  /// Versions of a movie or episode (LIB-010): files of one work that each
  /// play it whole (4K, 1080p, Director's Cut). More than one: the app
  /// offers a version picker on play. 1 for shows and tracks.
  final int versionCount;

  /// Why the player cannot play [filePath], null if it can (LIB-020).
  final Unplayable? unplayable;

  const LibraryItem({
    required this.id,
    required this.kind,
    required this.title,
    required this.filePath,
    this.streamUri,
    this.originalTitle,
    this.year,
    this.overview,
    this.posterUrl,
    this.backdropUrl,
    this.showId,
    this.season,
    this.episode,
    this.artist,
    this.album,
    this.trackNumber,
    this.durationMs,
    this.matchConfidence,
    this.tmdbId,
    this.addedAt,
    this.versionCount = 1,
    this.unplayable,
  });

  /// This item playing another file: one of its versions (LIB-010), with
  /// that file's [unplayable] reason.
  LibraryItem withFile(
    String filePath, {
    String? streamUri,
    int? durationMs,
    Unplayable? unplayable,
  }) => LibraryItem(
    id: id,
    kind: kind,
    title: title,
    filePath: filePath,
    streamUri: streamUri,
    originalTitle: originalTitle,
    year: year,
    overview: overview,
    posterUrl: posterUrl,
    backdropUrl: backdropUrl,
    showId: showId,
    season: season,
    episode: episode,
    artist: artist,
    album: album,
    trackNumber: trackNumber,
    durationMs: durationMs ?? this.durationMs,
    matchConfidence: matchConfidence,
    tmdbId: tmdbId,
    addedAt: addedAt,
    versionCount: versionCount,
    unplayable: unplayable,
  );

  @override
  bool operator ==(Object other) =>
      other is LibraryItem &&
      other.id == id &&
      other.kind == kind &&
      other.title == title &&
      other.originalTitle == originalTitle &&
      other.year == year &&
      other.overview == overview &&
      other.posterUrl == posterUrl &&
      other.backdropUrl == backdropUrl &&
      other.showId == showId &&
      other.season == season &&
      other.episode == episode &&
      other.artist == artist &&
      other.album == album &&
      other.trackNumber == trackNumber &&
      other.filePath == filePath &&
      other.streamUri == streamUri &&
      other.durationMs == durationMs &&
      other.matchConfidence == matchConfidence &&
      other.tmdbId == tmdbId &&
      other.versionCount == versionCount &&
      other.unplayable == unplayable;

  @override
  int get hashCode => Object.hash(
    id,
    kind,
    title,
    year,
    posterUrl,
    showId,
    season,
    episode,
    filePath,
    tmdbId,
    matchConfidence,
  );

  @override
  String toString() {
    final se = kind == ItemKind.episode ? ' S${season}E$episode' : '';
    final y = year != null ? ' ($year)' : '';
    return 'LibraryItem(${kind.name} "$title"$y$se)';
  }
}

/// Playback progress of an item.
class Progress {
  final String itemId;
  final int positionMs;
  final int durationMs;
  final DateTime updatedAt;
  final bool watched;

  const Progress({
    required this.itemId,
    required this.positionMs,
    required this.durationMs,
    required this.updatedAt,
    this.watched = false,
  });

  /// Fraction 0..1 (0 when the duration is unknown).
  double get fraction =>
      durationMs <= 0 ? 0 : (positionMs / durationMs).clamp(0.0, 1.0);

  @override
  bool operator ==(Object other) =>
      other is Progress &&
      other.itemId == itemId &&
      other.positionMs == positionMs &&
      other.durationMs == durationMs &&
      other.updatedAt == updatedAt &&
      other.watched == watched;

  @override
  int get hashCode =>
      Object.hash(itemId, positionMs, durationMs, updatedAt, watched);

  @override
  String toString() =>
      'Progress($itemId, $positionMs/$durationMs, watched: $watched)';
}

/// Phase of a running scan.
enum ScanPhase {
  /// Resolving the source (e.g. mounting SMB).
  connecting,

  /// Walking folders and recognising file names.
  scanning,

  /// Fetching posters and descriptions from TMDB.
  metadata,

  /// Finished (check [ScanProgress.error] / [ScanProgress.errors]).
  done,
}

/// Progress events emitted by [Library.scan].
class ScanProgress {
  final ScanPhase phase;

  /// Source currently being processed (null for the metadata phase / done).
  final String? sourceId;

  /// Media files found so far (all sources).
  final int filesFound;

  /// Items processed in the current phase.
  final int processed;

  /// Total items in the current phase, if known.
  final int? total;
  final String? currentPath;

  /// Last error, if any (scan continues with other sources).
  final String? error;

  /// All errors collected so far (per source / per TMDB request).
  final List<String> errors;

  const ScanProgress({
    required this.phase,
    this.sourceId,
    this.filesFound = 0,
    this.processed = 0,
    this.total,
    this.currentPath,
    this.error,
    this.errors = const [],
  });

  bool get isDone => phase == ScanPhase.done;

  @override
  String toString() =>
      'ScanProgress(${phase.name}, found: $filesFound, $processed/${total ?? '?'}'
      '${error != null ? ', error: $error' : ''})';
}
