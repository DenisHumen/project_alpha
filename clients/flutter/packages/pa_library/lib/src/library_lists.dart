// SPDX-License-Identifier: MPL-2.0

// Versions, favorites, collections and video playlists (LIB-010).
// RustLibrary implements them on the Rust core; the demo library of the app
// in memory. The v0 library has none.

import 'models.dart';

/// One version of a movie or episode: a file (a disc, a split file) that
/// plays the whole item — 4K and 1080p copies, a Director's Cut.
class MediaVersion {
  /// Id for [LibraryLists.setDefaultVersion].
  final String id;

  /// `4K · HDR10 · Director's Cut`, else the release source or the file
  /// name. Written in the release names, not translated.
  final String label;

  /// `4K`, `1080p`, `720p`, `SD`, or as written in the name.
  final String? resolution;
  final String? hdr;
  final String? edition;

  /// Display path, as [LibraryItem.filePath].
  final String filePath;

  /// Loopback stream URL of a file on a network source, as
  /// [LibraryItem.streamUri].
  final String? streamUri;
  final int sizeBytes;
  final int? durationMs;

  /// The version the item plays.
  final bool isDefault;

  const MediaVersion({
    required this.id,
    required this.label,
    required this.filePath,
    this.resolution,
    this.hdr,
    this.edition,
    this.streamUri,
    this.sizeBytes = 0,
    this.durationMs,
    this.isDefault = false,
  });

  /// The URI to hand to the player.
  String get playbackUri => streamUri ?? filePath;

  @override
  bool operator ==(Object other) =>
      other is MediaVersion &&
      other.id == id &&
      other.label == label &&
      other.filePath == filePath &&
      other.streamUri == streamUri &&
      other.isDefault == isDefault;

  @override
  int get hashCode => Object.hash(id, label, filePath, isDefault);

  @override
  String toString() => 'MediaVersion($id "$label"${isDefault ? ' *' : ''})';
}

/// A collection: the user's own or a TMDB one ([external]).
class MediaCollection {
  final String id;
  final String name;

  /// From provider metadata (a TMDB collection): not editable.
  final bool external;
  final int itemCount;

  const MediaCollection({
    required this.id,
    required this.name,
    this.external = false,
    this.itemCount = 0,
  });

  @override
  bool operator ==(Object other) =>
      other is MediaCollection &&
      other.id == id &&
      other.name == name &&
      other.external == external &&
      other.itemCount == itemCount;

  @override
  int get hashCode => Object.hash(id, name, external, itemCount);
}

/// A playlist of movies and episodes.
class VideoPlaylist {
  final String id;
  final String name;
  final int itemCount;
  final DateTime? updatedAt;

  const VideoPlaylist({
    required this.id,
    required this.name,
    this.itemCount = 0,
    this.updatedAt,
  });

  @override
  bool operator ==(Object other) =>
      other is VideoPlaylist &&
      other.id == id &&
      other.name == name &&
      other.itemCount == itemCount;

  @override
  int get hashCode => Object.hash(id, name, itemCount);
}

/// Versions, favorites, collections and playlists of a library.
abstract interface class LibraryLists {
  /// Versions of a movie or episode, best resolution first.
  Future<List<MediaVersion>> versions(String itemId);

  /// Makes [versionId] the default version of a movie ([MediaVersion.id]),
  /// or goes back to its first file (null).
  Future<void> setDefaultVersion(String itemId, String? versionId);

  /// Favorite items, newest first, now and after every change;
  /// [pinnedOnly]: those shown on Home.
  Stream<List<LibraryItem>> favorites({bool pinnedOnly = false});

  Future<bool> isFavorite(String itemId);

  /// Adds an item to the favorites or removes it.
  Future<void> setFavorite(String itemId, bool favorite);

  /// Shows a favorite in the Favorites row of Home or hides it there.
  Future<void> setFavoritePinned(String itemId, bool pinned);

  /// The user's collections and the TMDB ones, by name.
  Future<List<MediaCollection>> collections();

  /// Collections that hold an item.
  Future<List<MediaCollection>> itemCollections(String itemId);

  Future<List<LibraryItem>> collectionItems(String id);
  Future<MediaCollection> createCollection(String name);
  Future<void> renameCollection(String id, String name);
  Future<void> deleteCollection(String id);

  /// `false` if the item already was in the collection.
  Future<bool> addToCollection(String id, String itemId);
  Future<bool> removeFromCollection(String id, String itemId);

  /// Video playlists, most recently changed first.
  Future<List<VideoPlaylist>> playlists();
  Future<VideoPlaylist> createPlaylist(String name);
  Future<void> renamePlaylist(String id, String name);
  Future<void> deletePlaylist(String id);

  /// Items of a playlist in order.
  Future<List<LibraryItem>> playlistItems(String id);

  /// Appends a movie, an episode or every episode of a show; returns how
  /// many were added (items already there are skipped).
  Future<int> addToPlaylist(String id, String itemId);
  Future<int> removeFromPlaylist(String id, String itemId);

  /// Orders a playlist as [itemIds]; the others follow.
  Future<void> reorderPlaylist(String id, List<String> itemIds);
}
