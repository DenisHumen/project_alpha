// SPDX-License-Identifier: MPL-2.0

// The files of a split movie (LIB-020): the VOBs of a DVD title set or
// `cd1`/`cd2` parts. RustLibrary reads them from the core's folder groups
// (LIB-007, LIB-017); the player joins them into one timeline. The v0
// library knows no parts: it plays the item's file.

import 'models.dart';

/// A library that knows which files make up a split movie.
abstract interface class LibraryParts {
  /// What the player opens for [item] ([LibraryItem.playbackUri]: the item
  /// itself or a version picked with [LibraryItem.withFile]), in order:
  /// every part of its split file or DVD title set as a local path or a
  /// loopback stream URL (ADR-0038), else just its own URI. Empty if the
  /// file is not in the library.
  Future<List<String>> playParts(LibraryItem item);
}
