// SPDX-License-Identifier: MPL-2.0
//
// Single import point for the media library (R01-LIB `pa_library`). The demo
// mode (lib/fakes/fake_library.dart) implements the same interface.

export 'package:pa_library/pa_library.dart'
    show
        Library,
        LibraryItem,
        ItemKind,
        MediaSource,
        SourceKind,
        Progress,
        nextEpisodeIn,
        resumePositionMs,
        OpenedLibrary,
        openAppLibrary,
        RustLibrary,
        ScanProgress,
        ScanPhase,
        SmbUnsupportedException,
        tmdbAttribution;
