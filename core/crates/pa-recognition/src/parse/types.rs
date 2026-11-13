// SPDX-License-Identifier: MPL-2.0
//! Public result types of the parser.
//!
//! Field names and value formats follow the `parse` block of the recognition
//! corpus (`tools/recognition-eval/schema/corpus-record.v1.schema.json`), so a
//! serialized [`ParsedName`] can be compared with a corpus label directly.

use std::fmt;

use serde::de::{self, Deserializer, Visitor};
use serde::{Deserialize, Serialize, Serializer};

/// Episode (or absolute) number as written in the name; fractional numbers
/// such as `18.5` are kept exactly.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct EpisodeNumber {
    /// Integer part.
    pub number: u32,
    /// Decimal digit after the point (`18.5` → `Some(5)`), if any.
    pub fraction: Option<u8>,
}

impl EpisodeNumber {
    /// A whole episode number.
    pub const fn whole(number: u32) -> Self {
        Self {
            number,
            fraction: None,
        }
    }

    /// Value as `f64` (`18.5`).
    pub fn as_f64(self) -> f64 {
        match self.fraction {
            Some(f) => f64::from(self.number) + f64::from(f) / 10.0,
            None => f64::from(self.number),
        }
    }
}

impl From<u32> for EpisodeNumber {
    fn from(n: u32) -> Self {
        Self::whole(n)
    }
}

impl fmt::Display for EpisodeNumber {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self.fraction {
            Some(d) => write!(f, "{}.{}", self.number, d),
            None => write!(f, "{}", self.number),
        }
    }
}

impl Serialize for EpisodeNumber {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        match self.fraction {
            None => s.serialize_u32(self.number),
            Some(_) => s.serialize_f64(self.as_f64()),
        }
    }
}

impl<'de> Deserialize<'de> for EpisodeNumber {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        struct V;
        impl Visitor<'_> for V {
            type Value = EpisodeNumber;
            fn expecting(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
                f.write_str("a non-negative episode number")
            }
            fn visit_u64<E: de::Error>(self, v: u64) -> Result<EpisodeNumber, E> {
                u32::try_from(v)
                    .map(EpisodeNumber::whole)
                    .map_err(|_| E::custom("episode number too large"))
            }
            fn visit_i64<E: de::Error>(self, v: i64) -> Result<EpisodeNumber, E> {
                u32::try_from(v)
                    .map(EpisodeNumber::whole)
                    .map_err(|_| E::custom("episode number out of range"))
            }
            fn visit_f64<E: de::Error>(self, v: f64) -> Result<EpisodeNumber, E> {
                if !(0.0..=f64::from(u32::MAX)).contains(&v) {
                    return Err(E::custom("episode number out of range"));
                }
                let tenths = (v * 10.0).round() as u64;
                let number = u32::try_from(tenths / 10).map_err(E::custom)?;
                let frac = (tenths % 10) as u8;
                Ok(EpisodeNumber {
                    number,
                    fraction: (frac != 0).then_some(frac),
                })
            }
        }
        d.deserialize_any(V)
    }
}

/// Calendar date written in the name (daily shows, A11).
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Date {
    /// Year, e.g. 2025.
    pub year: u16,
    /// Month 1–12.
    pub month: u8,
    /// Day 1–31.
    pub day: u8,
}

impl fmt::Display for Date {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:04}-{:02}-{:02}", self.year, self.month, self.day)
    }
}

impl Serialize for Date {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for Date {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        let mut it = s.split('-');
        let mut num = |max: u32| -> Option<u32> {
            it.next()
                .and_then(|p| p.parse::<u32>().ok())
                .filter(|v| *v <= max)
        };
        match (num(9999), num(12), num(31)) {
            (Some(y), Some(m), Some(dd)) if m >= 1 && dd >= 1 => Ok(Date {
                year: y as u16,
                month: m as u8,
                day: dd as u8,
            }),
            _ => Err(de::Error::custom("expected YYYY-MM-DD")),
        }
    }
}

/// Kind of episode as marked in the name (corpus `episode_kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EpisodeKind {
    /// Ordinary episode (only set when a name says so explicitly).
    Regular,
    /// `Special`, `Specials`.
    Special,
    /// `OVA`, `OAD`.
    Ova,
    /// `ONA`.
    Ona,
    /// `Recap`.
    Recap,
    /// Creditless opening (`NCOP`).
    Ncop,
    /// Creditless ending (`NCED`).
    Nced,
    /// `SP1`-style special.
    Sp,
}

/// Type of an extra (bonus material), aligned with the corpus `extra_type`
/// plus `sample`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExtraKind {
    /// Trailer.
    Trailer,
    /// Teaser.
    Teaser,
    /// Featurette.
    Featurette,
    /// Behind the scenes / making-of.
    BehindTheScenes,
    /// Deleted scene.
    DeletedScene,
    /// Interview.
    Interview,
    /// Short.
    Short,
    /// Scene.
    Scene,
    /// Opening (OP, NCOP).
    Opening,
    /// Ending (ED, NCED).
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

/// Disc structure the path belongs to (A12).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DiscKind {
    /// Blu-ray folder (`BDMV/`).
    Bdmv,
    /// DVD folder (`VIDEO_TS/`).
    VideoTs,
    /// Disc image (`.iso`, `.img`).
    Iso,
}

/// What the path most likely is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaHint {
    /// No usable signal.
    #[default]
    Unknown,
    /// A movie (year, no episode markers, or a movie library folder).
    Movie,
    /// An episode of a series (season/episode/absolute/date markers).
    Episode,
    /// Bonus material, sample, opening/ending.
    Extra,
}

/// One interpretation of a path: what is written in the name and its folders.
///
/// Values are *as written* (release numbering, not provider numbering).
/// Empty and `None` fields are omitted when serialized.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct ParsedName {
    /// Work title without release junk; digits that belong to the title stay.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// Other titles: second half of a double title, folder title in another language.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub alt_titles: Vec<String>,
    /// Episode title written after the episode marker.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode_title: Option<String>,
    /// Release year of the work.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// Season number.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub season: Option<u32>,
    /// Episode numbers within the season (several for multi-episode files).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub episodes: Vec<EpisodeNumber>,
    /// Absolute episode numbers.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub absolute: Vec<EpisodeNumber>,
    /// Episode kind marker (`NCOP`, `OVA`, `SP`…).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub episode_kind: Option<EpisodeKind>,
    /// Air date (daily shows).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub date: Option<Date>,
    /// Part number of a split file (`cd2`, `Part 2`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub part: Option<u32>,
    /// Release group without brackets.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub release_group: Option<String>,
    /// Dubbing studio / tracker tag (`LostFilm`, `AniLibria`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dub_studio: Option<String>,
    /// CRC32 written in the name, upper-case hex.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub crc32: Option<String>,
    /// Release version (`01v2` → 2).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<u32>,
    /// Resolution as written (`1080p`, `4K`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resolution: Option<String>,
    /// Source as written (`BluRay`, `WEB-DL`, `UHD BluRay`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source: Option<String>,
    /// Video codec as written (`x265`, `HEVC`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub video_codec: Option<String>,
    /// Audio codec as written (`AAC`, `DDP5.1`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub audio_codec: Option<String>,
    /// HDR format as written (`HDR10`, `DV`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hdr: Option<String>,
    /// Edition as written (`Director's Cut`, `Extended`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub edition: Option<String>,
    /// Audio/subtitle languages, ISO 639-2/B codes (`rus`, `eng`, `jpn`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub languages: Vec<String>,
    /// Other release flags (`PROPER`, `REPACK`, `Batch`…), normalized upper-case.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub other: Vec<String>,
    /// Bonus material type, if this is an extra or sample.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extra: Option<ExtraKind>,
    /// Disc structure.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub disc: Option<DiscKind>,
    /// File extension, lower-case, without the dot.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// What the path most likely is.
    pub media: MediaHint,
    /// The name looks like an anime release (fansub group, CRC, `- 05`, CJK…).
    pub anime_hint: bool,
}

/// One weighted interpretation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Hypothesis {
    /// The interpretation.
    pub parsed: ParsedName,
    /// Relative weight in `(0, 1]`; weights of all hypotheses of a result sum to ≤ 1.
    pub weight: f32,
    /// Short machine-readable reasons for the choices that differ between
    /// hypotheses (`"number:episode"`, `"title:folder"`…).
    pub reasons: Vec<String>,
}

/// N-best parse of one path, best hypothesis first.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ParseResult {
    /// Version of the parser rules (stored with results, see 03 §5 `ParseResult`).
    pub parser_version: String,
    /// Hypotheses sorted by weight, descending. Never empty.
    pub hypotheses: Vec<Hypothesis>,
}

impl ParseResult {
    /// The most likely hypothesis.
    pub fn best(&self) -> &Hypothesis {
        &self.hypotheses[0]
    }
}

/// Parser options.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseOptions {
    /// Maximum number of hypotheses to return (at least 1).
    pub max_hypotheses: usize,
    /// Use folder names (season folders, show folder, extras folders) as context.
    pub use_folders: bool,
}

impl Default for ParseOptions {
    fn default() -> Self {
        Self {
            max_hypotheses: 5,
            use_folders: true,
        }
    }
}
