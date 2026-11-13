// SPDX-License-Identifier: MPL-2.0
//! Filename parser of `pa-recognition` (META-005) for Dart.
//!
//! The types here mirror `pa_recognition::parse` one to one. They are kept
//! separate so that the Dart API stays stable when the parser's internal
//! types change; the `From` conversions below are exhaustive, so a new
//! parser field or enum variant fails to compile until it is mirrored.

use pa_recognition::parse as p;

/// N-best parse of one relative media path, best hypothesis first.
#[derive(Debug, Clone, PartialEq)]
pub struct FilenameParse {
    /// Version of the parser rules; store it with the results.
    pub parser_version: String,
    /// Hypotheses sorted by weight, descending. Never empty.
    pub hypotheses: Vec<FilenameHypothesis>,
}

/// One weighted interpretation of a path.
#[derive(Debug, Clone, PartialEq)]
pub struct FilenameHypothesis {
    /// The interpretation.
    pub parsed: ParsedName,
    /// Relative weight in `(0, 1]`; weights of one result sum to ≤ 1.
    pub weight: f32,
    /// Machine-readable reasons for choices that differ between hypotheses.
    pub reasons: Vec<String>,
}

/// What is written in a file name and its folders (values as written).
#[derive(Debug, Clone, PartialEq)]
pub struct ParsedName {
    /// Work title without release junk.
    pub title: Option<String>,
    /// Other titles (second half of a double title, folder title).
    pub alt_titles: Vec<String>,
    /// Episode title written after the episode marker.
    pub episode_title: Option<String>,
    /// Release year of the work.
    pub year: Option<u16>,
    /// Season number.
    pub season: Option<u32>,
    /// Episode numbers within the season.
    pub episodes: Vec<EpisodeNumber>,
    /// Absolute episode numbers.
    pub absolute: Vec<EpisodeNumber>,
    /// Episode kind marker (`NCOP`, `OVA`, `SP`…).
    pub episode_kind: Option<EpisodeKind>,
    /// Air date (daily shows).
    pub date: Option<AirDate>,
    /// Part number of a split file.
    pub part: Option<u32>,
    /// Release group without brackets.
    pub release_group: Option<String>,
    /// Dubbing studio / tracker tag.
    pub dub_studio: Option<String>,
    /// CRC32 from the name, upper-case hex.
    pub crc32: Option<String>,
    /// Release version (`01v2` → 2).
    pub version: Option<u32>,
    /// Resolution as written (`1080p`, `4K`).
    pub resolution: Option<String>,
    /// Source as written (`BluRay`, `WEB-DL`).
    pub source: Option<String>,
    /// Video codec as written.
    pub video_codec: Option<String>,
    /// Audio codec as written.
    pub audio_codec: Option<String>,
    /// HDR format as written.
    pub hdr: Option<String>,
    /// Edition as written.
    pub edition: Option<String>,
    /// Languages, ISO 639-2/B codes.
    pub languages: Vec<String>,
    /// Other release flags, upper-case.
    pub other: Vec<String>,
    /// Bonus material type, if this is an extra or sample.
    pub extra: Option<ExtraKind>,
    /// Disc structure.
    pub disc: Option<DiscKind>,
    /// File extension, lower-case, without the dot.
    pub container: Option<String>,
    /// What the path most likely is.
    pub media: MediaHint,
    /// The name looks like an anime release.
    pub anime_hint: bool,
}

/// Episode number as written; `18.5` is `number: 18, fraction: Some(5)`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct EpisodeNumber {
    /// Integer part.
    pub number: u32,
    /// Decimal digit after the point, if any.
    pub fraction: Option<u8>,
}

/// Calendar date from the name.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AirDate {
    /// Year.
    pub year: u16,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
}

/// Episode kind marker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EpisodeKind {
    /// Ordinary episode, marked explicitly.
    Regular,
    /// `Special`.
    Special,
    /// `OVA`, `OAD`.
    Ova,
    /// `ONA`.
    Ona,
    /// `Recap`.
    Recap,
    /// Creditless opening.
    Ncop,
    /// Creditless ending.
    Nced,
    /// `SP1`-style special.
    Sp,
}

/// Bonus material type.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtraKind {
    /// Trailer.
    Trailer,
    /// Teaser.
    Teaser,
    /// Featurette.
    Featurette,
    /// Behind the scenes.
    BehindTheScenes,
    /// Deleted scene.
    DeletedScene,
    /// Interview.
    Interview,
    /// Short.
    Short,
    /// Scene.
    Scene,
    /// Opening.
    Opening,
    /// Ending.
    Ending,
    /// Disc menu.
    Menu,
    /// Preview / PV / CM.
    Preview,
    /// Release sample.
    Sample,
    /// Other bonus material.
    Other,
}

/// Disc structure the path belongs to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DiscKind {
    /// Blu-ray folder.
    Bdmv,
    /// DVD folder.
    VideoTs,
    /// Disc image.
    Iso,
}

/// What the path most likely is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MediaHint {
    /// No usable signal.
    Unknown,
    /// A movie.
    Movie,
    /// An episode of a series.
    Episode,
    /// Bonus material.
    Extra,
}

/// Parses a media path relative to the library root (folders give context)
/// into N-best hypotheses, at most 5, best first.
///
/// Pure function without I/O. Async on the Dart side, so a scan can call it
/// for many files without blocking the UI isolate.
pub async fn parse_filename(path: String) -> FilenameParse {
    p::parse(&path).into()
}

/// Same as [`parse_filename`] with explicit options: at most
/// `max_hypotheses` hypotheses (at least 1); `use_folders: false` ignores
/// the folder part of the path.
pub async fn parse_filename_with(
    path: String,
    max_hypotheses: u32,
    use_folders: bool,
) -> FilenameParse {
    let opts = p::ParseOptions {
        max_hypotheses: usize::try_from(max_hypotheses.max(1)).unwrap_or(usize::MAX),
        use_folders,
    };
    p::parse_with(&path, &opts).into()
}

/// Version of the parser rules, the same as [`FilenameParse::parser_version`].
#[flutter_rust_bridge::frb(sync)]
pub fn filename_parser_version() -> String {
    p::PARSER_VERSION.to_owned()
}

impl From<p::ParseResult> for FilenameParse {
    fn from(r: p::ParseResult) -> Self {
        Self {
            parser_version: r.parser_version,
            hypotheses: r.hypotheses.into_iter().map(Into::into).collect(),
        }
    }
}

impl From<p::Hypothesis> for FilenameHypothesis {
    fn from(h: p::Hypothesis) -> Self {
        Self {
            parsed: h.parsed.into(),
            weight: h.weight,
            reasons: h.reasons,
        }
    }
}

impl From<p::ParsedName> for ParsedName {
    fn from(n: p::ParsedName) -> Self {
        // Destructure so that a new parser field is a compile error here.
        let p::ParsedName {
            title,
            alt_titles,
            episode_title,
            year,
            season,
            episodes,
            absolute,
            episode_kind,
            date,
            part,
            release_group,
            dub_studio,
            crc32,
            version,
            resolution,
            source,
            video_codec,
            audio_codec,
            hdr,
            edition,
            languages,
            other,
            extra,
            disc,
            container,
            media,
            anime_hint,
        } = n;
        Self {
            title,
            alt_titles,
            episode_title,
            year,
            season,
            episodes: episodes.into_iter().map(Into::into).collect(),
            absolute: absolute.into_iter().map(Into::into).collect(),
            episode_kind: episode_kind.map(Into::into),
            date: date.map(Into::into),
            part,
            release_group,
            dub_studio,
            crc32,
            version,
            resolution,
            source,
            video_codec,
            audio_codec,
            hdr,
            edition,
            languages,
            other,
            extra: extra.map(Into::into),
            disc: disc.map(Into::into),
            container,
            media: media.into(),
            anime_hint,
        }
    }
}

impl From<p::EpisodeNumber> for EpisodeNumber {
    fn from(e: p::EpisodeNumber) -> Self {
        Self {
            number: e.number,
            fraction: e.fraction,
        }
    }
}

impl From<p::Date> for AirDate {
    fn from(d: p::Date) -> Self {
        Self {
            year: d.year,
            month: d.month,
            day: d.day,
        }
    }
}

impl From<p::EpisodeKind> for EpisodeKind {
    fn from(k: p::EpisodeKind) -> Self {
        match k {
            p::EpisodeKind::Regular => Self::Regular,
            p::EpisodeKind::Special => Self::Special,
            p::EpisodeKind::Ova => Self::Ova,
            p::EpisodeKind::Ona => Self::Ona,
            p::EpisodeKind::Recap => Self::Recap,
            p::EpisodeKind::Ncop => Self::Ncop,
            p::EpisodeKind::Nced => Self::Nced,
            p::EpisodeKind::Sp => Self::Sp,
        }
    }
}

impl From<p::ExtraKind> for ExtraKind {
    fn from(k: p::ExtraKind) -> Self {
        match k {
            p::ExtraKind::Trailer => Self::Trailer,
            p::ExtraKind::Teaser => Self::Teaser,
            p::ExtraKind::Featurette => Self::Featurette,
            p::ExtraKind::BehindTheScenes => Self::BehindTheScenes,
            p::ExtraKind::DeletedScene => Self::DeletedScene,
            p::ExtraKind::Interview => Self::Interview,
            p::ExtraKind::Short => Self::Short,
            p::ExtraKind::Scene => Self::Scene,
            p::ExtraKind::Opening => Self::Opening,
            p::ExtraKind::Ending => Self::Ending,
            p::ExtraKind::Menu => Self::Menu,
            p::ExtraKind::Preview => Self::Preview,
            p::ExtraKind::Sample => Self::Sample,
            p::ExtraKind::Other => Self::Other,
        }
    }
}

impl From<p::DiscKind> for DiscKind {
    fn from(k: p::DiscKind) -> Self {
        match k {
            p::DiscKind::Bdmv => Self::Bdmv,
            p::DiscKind::VideoTs => Self::VideoTs,
            p::DiscKind::Iso => Self::Iso,
        }
    }
}

impl From<p::MediaHint> for MediaHint {
    fn from(k: p::MediaHint) -> Self {
        match k {
            p::MediaHint::Unknown => Self::Unknown,
            p::MediaHint::Movie => Self::Movie,
            p::MediaHint::Episode => Self::Episode,
            p::MediaHint::Extra => Self::Extra,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block_on<F: std::future::Future>(f: F) -> F::Output {
        flutter_rust_bridge::for_generated::futures::executor::block_on(f)
    }

    #[test]
    fn anime_release_maps_all_fields() {
        let r = block_on(parse_filename(
            "Anime/One Piece/[SubsPlease] One Piece - 1085 (1080p) [ABCD1234].mkv".into(),
        ));
        assert_eq!(r.parser_version, filename_parser_version());
        let best = &r.hypotheses[0].parsed;
        assert_eq!(best.title.as_deref(), Some("One Piece"));
        assert_eq!(best.release_group.as_deref(), Some("SubsPlease"));
        assert_eq!(best.crc32.as_deref(), Some("ABCD1234"));
        assert_eq!(best.container.as_deref(), Some("mkv"));
        assert!(best.anime_hint);
    }

    #[test]
    fn same_as_parser_crate() {
        let path = "Shows/Dark/Season 1/Dark.S01E02.Lies.1080p.WEB-DL.mkv";
        let ours = block_on(parse_filename(path.into()));
        let theirs = p::parse(path);
        assert_eq!(ours.hypotheses.len(), theirs.hypotheses.len());
        let best = &ours.hypotheses[0].parsed;
        assert_eq!(best.season, Some(1));
        assert_eq!(
            best.episodes,
            vec![EpisodeNumber {
                number: 2,
                fraction: None
            }]
        );
        assert_eq!(best.media, MediaHint::Episode);
    }

    #[test]
    fn options_limit_hypotheses() {
        let r = block_on(parse_filename_with("Movie 2019.mkv".into(), 0, false));
        assert_eq!(r.hypotheses.len(), 1);
    }
}
