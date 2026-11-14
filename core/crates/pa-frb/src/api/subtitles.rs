// SPDX-License-Identifier: MPL-2.0
//! Sidecar subtitles of a file (PLAY-007) for Dart. The call is
//! [`CoreLibrary::sidecar_subtitles`](crate::api::library::CoreLibrary::sidecar_subtitles).

use pa_catalog as c;

/// A subtitle file next to a video, found by the folder analysis (LIB-007).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreSidecarSubtitle {
    /// Path relative to the source root; network files play through the
    /// stream proxy like the video.
    pub rel_path: String,
    /// Absolute path for local sources.
    pub local_path: Option<String>,
    /// ISO 639-2 code (`rus`, `eng`), if the file name tells.
    pub lang: Option<String>,
    /// Forced subtitles: only the foreign-language lines.
    pub forced: bool,
    /// SDH / hearing impaired / closed captions.
    pub hearing_impaired: bool,
    /// Marked as the default track in its name.
    pub is_default: bool,
}

impl From<c::Sidecar> for CoreSidecarSubtitle {
    fn from(s: c::Sidecar) -> Self {
        Self {
            rel_path: s.rel_path,
            local_path: s.local_path,
            lang: s.lang,
            forced: s.forced,
            hearing_impaired: s.hearing_impaired,
            is_default: s.default,
        }
    }
}
