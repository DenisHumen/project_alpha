// SPDX-License-Identifier: MPL-2.0
//! Which entries the shallow scan records and which folders it skips.
//!
//! The default rules are the release-0.1 Dart scanner's
//! (`clients/flutter/packages/pa_library/lib/src/scanner.dart`): media by
//! extension; samples, trailers and Plex-style extras skipped; NAS and OS
//! service folders and hidden entries skipped.
//!
//! Disc structures (LIB-017): the streams of a Blu-ray folder
//! (`BDMV/STREAM/*.m2ts`), the VOBs of a DVD folder (`VIDEO_TS/*.VOB`) and
//! disc images (`.iso`) are recorded, so `analyze_folders` (LIB-007) can
//! make each disc one item in `pa_library::groups` and the main title of
//! the disc reaches recognition. The rest of a disc structure
//! (`BDMV/PLAYLIST`, `BDMV/BACKUP`, `CERTIFICATE`, `AUDIO_TS`) is not
//! listed.

/// Kind of media file, decided by extension.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum MediaKind {
    /// Video container (`.mkv`, `.mp4`, `.m2ts`…).
    Video,
    /// Audio file (`.flac`, `.mp3`, `.opus`…).
    Audio,
}

/// Video extensions, lowercase, without the dot.
pub const VIDEO_EXTENSIONS: &[&str] = &[
    "mkv", "mp4", "m4v", "avi", "mov", "wmv", "mpg", "mpeg", "ts", "m2ts", "mts", "webm", "flv",
    "ogv", "3gp", "rmvb", "divx", "asf", "vob",
];

/// Disc image extensions, lowercase, without the dot. Recorded as
/// [`MediaKind::Video`]; `analyze_folders` makes each image a disc item.
pub const DISC_IMAGE_EXTENSIONS: &[&str] = &["iso"];

/// Audio extensions, lowercase, without the dot.
pub const AUDIO_EXTENSIONS: &[&str] = &[
    "mp3", "flac", "m4a", "aac", "ogg", "oga", "opus", "wav", "wma", "aiff", "aif", "ape", "wv",
    "dsf", "dff", "mka", "alac",
];

/// Folder names skipped entirely, lowercase.
pub const IGNORED_FOLDERS: &[&str] = &[
    // extras
    "sample",
    "samples",
    "extras",
    "extra",
    "featurettes",
    "featurette",
    "behind the scenes",
    "behindthescenes",
    "deleted scenes",
    "deletedscenes",
    "interviews",
    "interview",
    "scenes",
    "shorts",
    "trailers",
    "trailer",
    "other",
    "others",
    "bonus",
    "bonuses",
    "making of",
    "subs",
    "subtitles",
    "доп. материалы",
    "доп материалы",
    "дополнительные материалы",
    "бонусы",
    "трейлеры",
    "допы",
    "субтитры",
    // disc structures: `BDMV/` and `VIDEO_TS/` are entered by
    // `ScanRules::enter_folder_in` when `disc_streams` is on (LIB-017)
    "bdmv",
    "video_ts",
    "audio_ts",
    "certificate",
    // NAS / OS service folders
    "@eadir",
    "#recycle",
    "#snapshot",
    "$recycle.bin",
    "system volume information",
    "lost+found",
    ".@__thumb",
    "@recycle",
    ".trash",
    ".trashes",
];

/// Filters applied while walking a source.
///
/// [`ScanRules::default`] reproduces the release-0.1 rules. A file is
/// recorded when [`ScanRules::classify`] returns a kind; a folder is entered
/// when [`ScanRules::enter_folder`] is true and the depth limit allows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScanRules {
    /// Deepest folder level entered; the source root is level 0.
    pub max_depth: usize,
    /// Skip names starting with `.` (hidden on Unix, macOS `._` forks).
    pub skip_hidden: bool,
    /// Skip video files named as samples, trailers or Plex-style extras.
    pub skip_extras: bool,
    /// Enter `BDMV/` (only its `STREAM/` folder) and `VIDEO_TS/`, so the
    /// streams of disc folders are recorded (LIB-017). Off: disc folders
    /// are skipped like the other [`IGNORED_FOLDERS`].
    pub disc_streams: bool,
}

impl Default for ScanRules {
    fn default() -> Self {
        Self {
            max_depth: 12,
            skip_hidden: true,
            skip_extras: true,
            disc_streams: true,
        }
    }
}

impl ScanRules {
    /// Whether the walker lists a folder called `name`, without looking at
    /// its parent: disc folders (`BDMV`, `VIDEO_TS`) are not entered. The
    /// walk uses [`enter_folder_in`](Self::enter_folder_in).
    #[must_use]
    pub fn enter_folder(&self, name: &str) -> bool {
        if self.skip_hidden && name.starts_with('.') {
            return false;
        }
        !is_ignored_folder(name)
    }

    /// Whether the walker lists the folder `name` inside the folder
    /// `parent` (a path or a folder name). With
    /// [`disc_streams`](Self::disc_streams), `BDMV` and `VIDEO_TS` are
    /// entered and inside `BDMV` only `STREAM`.
    #[must_use]
    pub fn enter_folder_in(&self, parent: &str, name: &str) -> bool {
        if self.skip_hidden && name.starts_with('.') {
            return false;
        }
        if self.disc_streams {
            let parent_name = parent.rsplit('/').next().unwrap_or(parent);
            if parent_name.eq_ignore_ascii_case("bdmv") {
                return name.eq_ignore_ascii_case("stream");
            }
            if name.eq_ignore_ascii_case("bdmv") || name.eq_ignore_ascii_case("video_ts") {
                return true;
            }
        }
        !is_ignored_folder(name)
    }

    /// The media kind of a file called `name`, or `None` if the scan
    /// does not record it.
    #[must_use]
    pub fn classify(&self, name: &str) -> Option<MediaKind> {
        if self.skip_hidden && name.starts_with('.') {
            return None;
        }
        let kind = media_kind(name)?;
        if self.skip_extras && kind == MediaKind::Video && is_extra_file(name) {
            return None;
        }
        Some(kind)
    }
}

/// Media kind of `name` by its extension (case-insensitive).
#[must_use]
pub fn media_kind(name: &str) -> Option<MediaKind> {
    let (_, ext) = split_ext(name);
    let ext = ext?.to_ascii_lowercase();
    if VIDEO_EXTENSIONS.contains(&ext.as_str()) || DISC_IMAGE_EXTENSIONS.contains(&ext.as_str()) {
        Some(MediaKind::Video)
    } else if AUDIO_EXTENSIONS.contains(&ext.as_str()) {
        Some(MediaKind::Audio)
    } else {
        None
    }
}

/// True for folder names in [`IGNORED_FOLDERS`] (case-insensitive).
#[must_use]
pub fn is_ignored_folder(name: &str) -> bool {
    let lower = name.to_lowercase();
    IGNORED_FOLDERS.contains(&lower.as_str())
}

/// True for samples, trailers and Plex-style extras (`Movie-trailer.mkv`).
///
/// Same patterns as the Dart scanner, on the name without its extension:
/// - starts with `sample`, `trailer` or `teaser` followed by a separator or the end;
/// - ends with one of them after a separator (` . _ - [ (`), optionally closed by `]` or `)`;
/// - ends with `-featurette`, `-deleted`, `-behindthescenes`, `-interview`,
///   `-scene`, `-short`, `-other` or `-extra`.
#[must_use]
pub fn is_extra_file(name: &str) -> bool {
    const WORDS: [&str; 3] = ["sample", "trailer", "teaser"];
    const PLEX: [&str; 8] = [
        "featurette",
        "deleted",
        "behindthescenes",
        "interview",
        "scene",
        "short",
        "other",
        "extra",
    ];
    let (stem, _) = split_ext(name);
    let base = stem.to_lowercase();

    for w in WORDS {
        if let Some(rest) = base.strip_prefix(w)
            && rest.chars().next().is_none_or(is_lead_separator)
        {
            return true;
        }
    }

    let trimmed = base
        .strip_suffix(']')
        .or_else(|| base.strip_suffix(')'))
        .unwrap_or(&base);
    for candidate in [base.as_str(), trimmed] {
        for w in WORDS {
            if let Some(head) = candidate.strip_suffix(w)
                && head.chars().next_back().is_some_and(is_tail_separator)
            {
                return true;
            }
        }
    }

    PLEX.iter()
        .any(|w| base.strip_suffix(w).is_some_and(|head| head.ends_with('-')))
}

/// `[\s._-]`
fn is_lead_separator(c: char) -> bool {
    c.is_whitespace() || matches!(c, '.' | '_' | '-')
}

/// `[\s._\-\[(]`
fn is_tail_separator(c: char) -> bool {
    is_lead_separator(c) || matches!(c, '[' | '(')
}

/// Splits `name` into the stem and the extension after the last `.`
/// (none for dotfiles like `.mkv` or names without a dot).
pub(crate) fn split_ext(name: &str) -> (&str, Option<&str>) {
    match name.rfind('.') {
        Some(0) | None => (name, None),
        Some(i) => (&name[..i], Some(&name[i + 1..])),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn media_kind_by_extension() {
        assert_eq!(media_kind("Dune (2021).MKV"), Some(MediaKind::Video));
        assert_eq!(media_kind("01 - Track.flac"), Some(MediaKind::Audio));
        assert_eq!(media_kind("movie.nfo"), None);
        assert_eq!(
            media_kind("Spirited Away (2001).ISO"),
            Some(MediaKind::Video)
        );
        assert_eq!(media_kind("VTS_01_1.VOB"), Some(MediaKind::Video));
        assert_eq!(media_kind("mkv"), None);
        assert_eq!(media_kind(".mkv"), None);
    }

    #[test]
    fn extras_match_dart_rules() {
        for extra in [
            "sample.mkv",
            "Sample-dune.mkv",
            "trailer.mp4",
            "teaser 2.mp4",
            "Dune.2021.sample.mkv",
            "Dune (2021) [Trailer].mkv",
            "Dune (trailer).mkv",
            "Dune-trailer.mkv",
            "Dune-featurette.mkv",
            "Dune-behindthescenes.mkv",
            "Dune-deleted.mkv",
            "Dune-extra.mkv",
        ] {
            assert!(is_extra_file(extra), "{extra}");
        }
        for movie in [
            "Dune (2021).mkv",
            "Samples of Life (2019).mkv",
            "The Trailer Park Boys S01E01.mkv",
            "Teasers.mkv",
            "Resampled.mkv",
            "Other People (2016).mkv",
            "Short Circuit (1986).mkv",
            "Behind the Scenes.mkv",
        ] {
            assert!(!is_extra_file(movie), "{movie}");
        }
    }

    #[test]
    fn rules_classify_and_folders() {
        let r = ScanRules::default();
        assert_eq!(r.classify("Dune.mkv"), Some(MediaKind::Video));
        assert_eq!(r.classify("._Dune.mkv"), None);
        assert_eq!(r.classify("Dune-trailer.mkv"), None);
        // Extras rules apply to video only.
        assert_eq!(r.classify("sample.flac"), Some(MediaKind::Audio));
        for skip in [
            "BDMV",
            "VIDEO_TS",
            "@eaDir",
            "#recycle",
            "$RECYCLE.BIN",
            "System Volume Information",
            "Featurettes",
            "Доп. материалы",
            ".git",
        ] {
            assert!(!r.enter_folder(skip), "{skip}");
        }
        assert!(r.enter_folder("Season 01"));
        // Disc structures: BDMV (STREAM only) and VIDEO_TS are walked.
        assert!(r.enter_folder_in("/Movies/Inception (2010)", "BDMV"));
        assert!(r.enter_folder_in("/Movies/Inception (2010)/BDMV", "STREAM"));
        assert!(!r.enter_folder_in("/Movies/Inception (2010)/BDMV", "PLAYLIST"));
        assert!(!r.enter_folder_in("/Movies/Inception (2010)/BDMV", "BACKUP"));
        assert!(r.enter_folder_in("/Movies/Heat (1995)", "VIDEO_TS"));
        assert!(!r.enter_folder_in("/Movies/Heat (1995)", "AUDIO_TS"));
        assert!(!r.enter_folder_in("/Movies/Inception (2010)", "CERTIFICATE"));
        assert!(!r.enter_folder_in("/Movies", "Featurettes"));
        assert!(r.enter_folder_in("/Shows/Show", "Season 01"));
        let no_discs = ScanRules {
            disc_streams: false,
            ..ScanRules::default()
        };
        assert!(!no_discs.enter_folder_in("/Movies/Inception (2010)", "BDMV"));
        assert!(!no_discs.enter_folder_in("/Movies/Heat (1995)", "VIDEO_TS"));
        let all = ScanRules {
            skip_hidden: false,
            skip_extras: false,
            ..ScanRules::default()
        };
        assert_eq!(all.classify("Dune-trailer.mkv"), Some(MediaKind::Video));
        assert!(all.enter_folder(".hidden"));
    }
}
