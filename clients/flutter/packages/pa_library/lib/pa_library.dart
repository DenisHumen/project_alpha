// SPDX-License-Identifier: MPL-2.0

/// Kinolune media library for release 0.1: sources, scanner, filename
/// parser v0, TMDB metadata, SQLite storage and playback progress.
/// See docs/dev/release-0.1-contract.md.
library;

export 'src/library.dart'
    show Library, continueWatchingMin, continueWatchingMax, showIdFor;
export 'src/models.dart';
export 'src/music_tags.dart' show TrackInfo, trackInfoFromPath, readTags;
export 'src/parser/filename_parser.dart'
    show ParsedName, parseName, parsePath, normalizeTitle, seasonFromFolder;
export 'src/scanner.dart'
    show MediaType, ScannedFile, mediaTypeOf, isExtraFile, scanFolder;
export 'src/smb.dart'
    show SmbUrl, SmbResolver, SmbUnsupportedException, SmbMountException;
export 'src/tmdb.dart'
    show
        tmdbAttribution,
        tmdbImageUrl,
        TmdbClient,
        TmdbException,
        TmdbSearchResult;
