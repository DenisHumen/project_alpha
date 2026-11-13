// SPDX-License-Identifier: MPL-2.0
//
// Stand-in for `package:pa_library` (R01-LIB) mirroring the API in
// docs/dev/release-0.1-contract.md. Used only until the real package lands
// on main; the app imports it through lib/integration/library.dart.

import 'fake_library.dart';

enum SourceKind { localFolder, smb }

class MediaSource {
  MediaSource({
    required this.id,
    required this.kind,
    required this.name,
    required this.path,
    this.username,
  });

  String id;
  SourceKind kind;
  String name;

  /// localFolder: absolute path; smb: smb://host/share/path
  String path;
  String? username;
}

enum ItemKind { movie, show, episode, track }

class LibraryItem {
  LibraryItem({
    required this.id,
    required this.kind,
    required this.title,
    required this.filePath,
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
    this.durationMs,
    this.matchConfidence,
  });

  String id;
  ItemKind kind;
  String title;
  String? originalTitle;
  int? year;
  String? overview;
  String? posterUrl;
  String? backdropUrl;
  String? showId;
  int? season;
  int? episode;
  String? artist;
  String? album;
  String filePath;
  int? durationMs;
  double? matchConfidence;
}

class Progress {
  Progress({
    required this.itemId,
    required this.positionMs,
    required this.durationMs,
    required this.updatedAt,
    this.watched = false,
  });

  String itemId;
  int positionMs;
  int durationMs;
  DateTime updatedAt;
  bool watched;
}

/// Not spelled out in the contract; the real package defines its own shape.
class ScanProgress {
  ScanProgress({
    required this.filesFound,
    required this.filesProcessed,
    this.currentPath,
    this.done = false,
    this.error,
  });

  int filesFound;
  int filesProcessed;
  String? currentPath;
  bool done;
  String? error;
}

abstract class Library {
  static Future<Library> open(String dbPath) async => FakeLibrary();

  Future<List<MediaSource>> sources();
  Future<MediaSource> addSource(MediaSource s);
  Future<void> removeSource(String id);
  Stream<ScanProgress> scan({String? sourceId, String? smbPassword});
  Future<void> setTmdbApiKey(String? key);
  Stream<List<LibraryItem>> movies();
  Stream<List<LibraryItem>> shows();
  Stream<List<LibraryItem>> episodes(String showId);
  Stream<List<LibraryItem>> tracks();
  Stream<List<(LibraryItem, Progress)>> continueWatching();
  Future<void> saveProgress(Progress p);
  Future<void> fixMatch(String itemId, {int? tmdbId, String? title, int? year});
}
