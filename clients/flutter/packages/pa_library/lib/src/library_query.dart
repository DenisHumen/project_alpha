// SPDX-License-Identifier: MPL-2.0

// Library queries (LIB-009): filters, sorts, pages, the alphabet index,
// facets for filter chips, folder entries, per-view preferences and smart
// shelves. RustLibrary answers them on the Rust core.

import 'models.dart';

/// Watch state of a movie or show (default profile).
enum WatchState { unwatched, inProgress, watched }

/// Resolution class of the main video stream: SD, 720p, 1080p, 4K. Scope
/// films count by width (1920×800 is Full HD).
enum VideoResolution { sd, hd, fullHd, uhd }

/// Dynamic range filter. A Dolby Vision 8.1 file is also HDR10.
enum HdrFilter { sdr, anyHdr, hdr10, hdr10Plus, dolbyVision, hlg }

/// Sort field. Missing values sort last in both directions; ties go by
/// title A→Z.
enum SortField { title, year, added, duration, lastWatched }

/// Filters of a query: AND between fields, OR inside one; empty fields do
/// not filter. File filters (resolution, HDR, audio, source) match an item
/// if one of its files matches all of them; unprobed files match none.
class LibraryFilter {
  /// Genre names, case-insensitive.
  final List<String> genres;
  final int? yearMin;
  final int? yearMax;
  final Set<WatchState> watched;
  final Set<VideoResolution> resolutions;
  final Set<HdrFilter> hdr;

  /// Audio format slugs: `aac`, `ac3`, `eac3`, `truehd`, `dts`,
  /// `dts_hd_ma`, `flac`, `opus`, `pcm`, `atmos`, `dts_x`…
  final List<String> audioFormats;

  /// Audio languages as stored by the probe (`rus`, `eng`).
  final List<String> audioLanguages;

  /// Source ids ([MediaSource.id]).
  final List<String> sources;

  const LibraryFilter({
    this.genres = const [],
    this.yearMin,
    this.yearMax,
    this.watched = const {},
    this.resolutions = const {},
    this.hdr = const {},
    this.audioFormats = const [],
    this.audioLanguages = const [],
    this.sources = const [],
  });

  bool get isEmpty =>
      genres.isEmpty &&
      yearMin == null &&
      yearMax == null &&
      watched.isEmpty &&
      resolutions.isEmpty &&
      hdr.isEmpty &&
      audioFormats.isEmpty &&
      audioLanguages.isEmpty &&
      sources.isEmpty;

  @override
  bool operator ==(Object other) =>
      other is LibraryFilter &&
      _listEq(other.genres, genres) &&
      other.yearMin == yearMin &&
      other.yearMax == yearMax &&
      _setEq(other.watched, watched) &&
      _setEq(other.resolutions, resolutions) &&
      _setEq(other.hdr, hdr) &&
      _listEq(other.audioFormats, audioFormats) &&
      _listEq(other.audioLanguages, audioLanguages) &&
      _listEq(other.sources, sources);

  @override
  int get hashCode => Object.hash(
    Object.hashAll(genres),
    yearMin,
    yearMax,
    Object.hashAllUnordered(watched),
    Object.hashAllUnordered(resolutions),
    Object.hashAllUnordered(hdr),
    Object.hashAll(audioFormats),
    Object.hashAll(audioLanguages),
    Object.hashAll(sources),
  );

  @override
  String toString() =>
      'LibraryFilter(genres: $genres, years: $yearMin–$yearMax, '
      'watched: $watched, resolutions: $resolutions, hdr: $hdr, '
      'audio: $audioFormats, languages: $audioLanguages, sources: $sources)';
}

bool _listEq<T>(List<T> a, List<T> b) {
  if (a.length != b.length) return false;
  for (var i = 0; i < a.length; i++) {
    if (a[i] != b[i]) return false;
  }
  return true;
}

bool _setEq<T>(Set<T> a, Set<T> b) => a.length == b.length && a.containsAll(b);

/// Sort of a query.
class LibrarySort {
  final SortField field;
  final bool descending;

  const LibrarySort({this.field = SortField.title, this.descending = false});

  @override
  bool operator ==(Object other) =>
      other is LibrarySort &&
      other.field == field &&
      other.descending == descending;

  @override
  int get hashCode => Object.hash(field, descending);

  @override
  String toString() => 'LibrarySort(${field.name}${descending ? ' ↓' : ''})';
}

/// Movies or shows ([ItemKind.movie] / [ItemKind.show]), filtered and
/// sorted.
class LibraryQuery {
  final ItemKind kind;
  final LibraryFilter filter;
  final LibrarySort sort;

  const LibraryQuery(
    this.kind, {
    this.filter = const LibraryFilter(),
    this.sort = const LibrarySort(),
  });

  @override
  bool operator ==(Object other) =>
      other is LibraryQuery &&
      other.kind == kind &&
      other.filter == filter &&
      other.sort == sort;

  @override
  int get hashCode => Object.hash(kind, filter, sort);

  @override
  String toString() => 'LibraryQuery(${kind.name}, $filter, $sort)';
}

/// One page of a query.
class LibraryPage {
  final List<LibraryItem> items;

  /// Matching items in total (all pages).
  final int total;
  final int offset;

  const LibraryPage({
    required this.items,
    required this.total,
    required this.offset,
  });
}

/// A letter of the alphabet index: jump to [offset] in the title-sorted
/// list.
class AlphabetEntry {
  /// Upper-case letter, `#` for digits and symbols.
  final String letter;
  final int offset;
  final int count;

  const AlphabetEntry({
    required this.letter,
    required this.offset,
    required this.count,
  });

  @override
  bool operator ==(Object other) =>
      other is AlphabetEntry &&
      other.letter == letter &&
      other.offset == offset &&
      other.count == count;

  @override
  int get hashCode => Object.hash(letter, offset, count);

  @override
  String toString() => '$letter@$offset×$count';
}

/// A filter value with the number of items that have it.
class FacetCount<T> {
  final T value;
  final int count;

  const FacetCount(this.value, this.count);

  @override
  bool operator ==(Object other) =>
      other is FacetCount<T> && other.value == value && other.count == count;

  @override
  int get hashCode => Object.hash(value, count);

  @override
  String toString() => '$value: $count';
}

/// Filter values present among movies or shows (unfiltered counts), for
/// filter chips.
class LibraryFacets {
  final List<FacetCount<String>> genres;
  final int? yearMin;
  final int? yearMax;
  final List<FacetCount<VideoResolution>> resolutions;
  final List<FacetCount<HdrFilter>> hdr;

  /// Most common first.
  final List<FacetCount<String>> audioFormats;

  /// Most common first.
  final List<FacetCount<String>> audioLanguages;

  /// By source id.
  final List<FacetCount<String>> sources;

  const LibraryFacets({
    required this.genres,
    required this.yearMin,
    required this.yearMax,
    required this.resolutions,
    required this.hdr,
    required this.audioFormats,
    required this.audioLanguages,
    required this.sources,
  });
}

/// An entry of a folder listing.
class FolderEntry {
  final String name;

  /// Relative to the source root.
  final String relPath;
  final bool isFolder;

  /// Files: the movie, episode or track ([LibraryItem.id]), if recognized.
  final String? itemId;
  final int sizeBytes;

  /// Folders: media files inside at any depth; files: 1.
  final int fileCount;
  final DateTime addedAt;
  final int? durationMs;

  const FolderEntry({
    required this.name,
    required this.relPath,
    required this.isFolder,
    required this.itemId,
    required this.sizeBytes,
    required this.fileCount,
    required this.addedAt,
    required this.durationMs,
  });

  @override
  String toString() => isFolder ? '$name/' : name;
}

/// Saved presentation of a view (`movies`, `shows`, `folder:<source>`…):
/// its own sort, "folders first" and, optionally, its last filter.
class ViewPrefs {
  final String view;
  final LibrarySort sort;
  final bool foldersFirst;
  final LibraryFilter? filter;

  const ViewPrefs(
    this.view, {
    this.sort = const LibrarySort(),
    this.foldersFirst = true,
    this.filter,
  });

  @override
  bool operator ==(Object other) =>
      other is ViewPrefs &&
      other.view == view &&
      other.sort == sort &&
      other.foldersFirst == foldersFirst &&
      other.filter == filter;

  @override
  int get hashCode => Object.hash(view, sort, foldersFirst, filter);

  @override
  String toString() =>
      'ViewPrefs($view, $sort, foldersFirst: $foldersFirst, $filter)';
}

/// A smart shelf: a saved query, optionally pinned to Home.
class SmartShelf {
  final String id;
  final String name;
  final LibraryQuery query;
  final bool pinnedHome;
  final int position;

  const SmartShelf({
    required this.id,
    required this.name,
    required this.query,
    required this.pinnedHome,
    required this.position,
  });

  @override
  String toString() => 'SmartShelf($id, $name, pinned: $pinnedHome)';
}
