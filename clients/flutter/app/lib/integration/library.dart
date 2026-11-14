// SPDX-License-Identifier: MPL-2.0
//
// Single import point for the media library (R01-LIB `pa_library`). The demo
// mode (lib/fakes/fake_library.dart) implements the same interface.

export 'package:pa_library/pa_library.dart'
    show
        ArtworkCache,
        Library,
        LibraryItem,
        ItemKind,
        MediaSource,
        SourceKind,
        Progress,
        nextEpisodeIn,
        resumePositionMs,
        OpenedLibrary,
        coreDbFileName,
        openAppLibrary,
        RustArtworkCache,
        RustLibrary,
        ScanProgress,
        ScanPhase,
        SmbUnsupportedException,
        tmdbAttribution,
        // LIB-009 queries (Home rows, UI-007)
        FacetCount,
        LibraryFacets,
        LibraryFilter,
        LibraryPage,
        LibraryQuery,
        LibrarySort,
        SmartShelf,
        SortField,
        WatchState,
        continueWatchingMax,
        // LIB-010 versions, favorites, collections, playlists
        LibraryLists,
        MediaCollection,
        MediaVersion,
        VideoPlaylist,
        // UI-011 search
        SearchGroup,
        groupSearchResults,
        searchGroupOrder,
        searchPerGroupDefault,
        // PLAY-007 sidecar subtitles
        LibrarySubtitles,
        SidecarSubtitle,
        // UI-009 detail
        CreditPerson,
        ExternalId,
        ExternalRating,
        HdrFormat,
        ItemDetails,
        LibraryDetails,
        MediaExtra,
        MediaExtraKind,
        TechInfo;
