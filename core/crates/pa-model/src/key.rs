// SPDX-License-Identifier: MPL-2.0
//! [`MediaKey`]: the stable, device-independent key of a playable item.
//!
//! The same key is used for sync (`progress`, `watched`, `playback_session`),
//! so it must not depend on paths or hosts (docs/research/04 §1.8,
//! docs/research/06 §5.2). Text form is canonical: `parse(print(k)) == k`
//! and `print(parse(s)) == s` for every accepted `s`.

use std::fmt;
use std::str::FromStr;

use serde::{Deserialize, Deserializer, Serialize, Serializer};

/// Maximum length of a key in its text form.
pub const MAX_KEY_LEN: usize = 256;

/// Stable sync key of a playable item.
///
/// | Variant | Text form |
/// |---|---|
/// | [`MediaKey::File`] | `file:<fingerprint>` (lowercase hex) |
/// | [`MediaKey::TmdbMovie`] | `tmdb:movie:<id>` |
/// | [`MediaKey::TmdbEpisode`] | `tmdb:episode:<show_id>:<season>:<episode>` |
/// | [`MediaKey::MbRecording`] | `mb:recording:<mbid>` (lowercase UUID) |
/// | [`MediaKey::TwitchVod`] | `twitch:vod:<video_id>` |
/// | [`MediaKey::TwitchRecording`] | `twitch:rec:<stream_id>` |
/// | [`MediaKey::TwitchClip`] | `twitch:clip:<slug>` |
///
/// Numbers are decimal without leading zeros. A recognised file uses its
/// external key; `file:` is the fallback for unrecognised content. An online
/// Twitch VOD and its downloaded copy share `twitch:vod:<id>`.
///
/// Fields are public so the enum maps to a UniFFI enum; a value built by hand
/// can be checked with [`MediaKey::is_valid`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub enum MediaKey {
    /// Content fingerprint (hash of size + first and last 64 KiB).
    File { fingerprint: String },
    /// TMDB movie.
    TmdbMovie { id: u64 },
    /// Episode of a TMDB TV show in TMDB numbering (season 0 = specials).
    TmdbEpisode {
        show_id: u64,
        season: u32,
        episode: u32,
    },
    /// MusicBrainz recording.
    MbRecording { mbid: String },
    /// Twitch VOD, online or downloaded.
    TwitchVod { video_id: String },
    /// Live recording made by us, before it is linked to a VOD.
    TwitchRecording { stream_id: String },
    /// Twitch clip.
    TwitchClip { slug: String },
}

/// Why a string is not a valid [`MediaKey`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ParseKeyError {
    /// Empty input or longer than [`MAX_KEY_LEN`].
    BadLength,
    /// Unknown `scheme:kind` prefix.
    UnknownKind,
    /// Wrong number of `:`-separated parts for this kind.
    WrongArity,
    /// A part is not in its canonical form.
    InvalidPart,
}

impl fmt::Display for ParseKeyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            Self::BadLength => "media key is empty or too long",
            Self::UnknownKind => "unknown media key kind",
            Self::WrongArity => "wrong number of media key parts",
            Self::InvalidPart => "media key part is not canonical",
        })
    }
}

impl std::error::Error for ParseKeyError {}

impl MediaKey {
    /// True if the key prints to a string that parses back to itself.
    pub fn is_valid(&self) -> bool {
        self.to_string().parse::<MediaKey>().as_ref() == Ok(self)
    }

    /// True for keys of the Twitch module.
    pub fn is_twitch(&self) -> bool {
        matches!(
            self,
            Self::TwitchVod { .. } | Self::TwitchRecording { .. } | Self::TwitchClip { .. }
        )
    }
}

impl fmt::Display for MediaKey {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::File { fingerprint } => write!(f, "file:{fingerprint}"),
            Self::TmdbMovie { id } => write!(f, "tmdb:movie:{id}"),
            Self::TmdbEpisode {
                show_id,
                season,
                episode,
            } => write!(f, "tmdb:episode:{show_id}:{season}:{episode}"),
            Self::MbRecording { mbid } => write!(f, "mb:recording:{mbid}"),
            Self::TwitchVod { video_id } => write!(f, "twitch:vod:{video_id}"),
            Self::TwitchRecording { stream_id } => write!(f, "twitch:rec:{stream_id}"),
            Self::TwitchClip { slug } => write!(f, "twitch:clip:{slug}"),
        }
    }
}

impl FromStr for MediaKey {
    type Err = ParseKeyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if s.is_empty() || s.len() > MAX_KEY_LEN {
            return Err(ParseKeyError::BadLength);
        }
        let parts: Vec<&str> = s.split(':').collect();
        let arity = |n: usize| {
            if parts.len() == n {
                Ok(())
            } else {
                Err(ParseKeyError::WrongArity)
            }
        };
        let check = |ok: bool, v: &str| {
            if ok {
                Ok(v.to_owned())
            } else {
                Err(ParseKeyError::InvalidPart)
            }
        };
        match parts.as_slice() {
            ["file", ..] => {
                arity(2)?;
                let fp = parts[1];
                Ok(Self::File {
                    fingerprint: check(is_lower_hex(fp), fp)?,
                })
            }
            ["tmdb", "movie", ..] => {
                arity(3)?;
                Ok(Self::TmdbMovie {
                    id: positive(parts[2])?,
                })
            }
            ["tmdb", "episode", ..] => {
                arity(5)?;
                Ok(Self::TmdbEpisode {
                    show_id: positive(parts[2])?,
                    season: number(parts[3])?,
                    episode: number(parts[4])?,
                })
            }
            ["mb", "recording", ..] => {
                arity(3)?;
                let id = parts[2];
                Ok(Self::MbRecording {
                    mbid: check(is_lower_uuid(id), id)?,
                })
            }
            ["twitch", "vod", ..] => {
                arity(3)?;
                let id = parts[2];
                Ok(Self::TwitchVod {
                    video_id: check(is_twitch_id(id), id)?,
                })
            }
            ["twitch", "rec", ..] => {
                arity(3)?;
                let id = parts[2];
                Ok(Self::TwitchRecording {
                    stream_id: check(is_twitch_id(id), id)?,
                })
            }
            ["twitch", "clip", ..] => {
                arity(3)?;
                let slug = parts[2];
                Ok(Self::TwitchClip {
                    slug: check(is_clip_slug(slug), slug)?,
                })
            }
            _ => Err(ParseKeyError::UnknownKind),
        }
    }
}

impl Serialize for MediaKey {
    fn serialize<S: Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.collect_str(self)
    }
}

impl<'de> Deserialize<'de> for MediaKey {
    fn deserialize<D: Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        s.parse().map_err(serde::de::Error::custom)
    }
}

/// Canonical decimal: digits only, no sign, no leading zeros.
fn is_canonical_decimal(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) && (s == "0" || !s.starts_with('0'))
}

fn number<T: FromStr>(s: &str) -> Result<T, ParseKeyError> {
    if !is_canonical_decimal(s) {
        return Err(ParseKeyError::InvalidPart);
    }
    s.parse().map_err(|_| ParseKeyError::InvalidPart)
}

fn positive(s: &str) -> Result<u64, ParseKeyError> {
    match number(s)? {
        0 => Err(ParseKeyError::InvalidPart),
        n => Ok(n),
    }
}

fn is_lower_hex(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| matches!(b, b'0'..=b'9' | b'a'..=b'f'))
}

fn is_lower_uuid(s: &str) -> bool {
    let groups: Vec<&str> = s.split('-').collect();
    groups.len() == 5
        && groups
            .iter()
            .zip([8, 4, 4, 4, 12])
            .all(|(g, n)| g.len() == n && is_lower_hex(g))
}

/// Twitch ids are opaque strings that are numeric today; we keep them as
/// strings but require the canonical numeric form so keys stay comparable.
fn is_twitch_id(s: &str) -> bool {
    is_canonical_decimal(s) && s != "0"
}

fn is_clip_slug(s: &str) -> bool {
    !s.is_empty()
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn canonical_decimal() {
        assert!(is_canonical_decimal("0"));
        assert!(is_canonical_decimal("10"));
        assert!(!is_canonical_decimal("01"));
        assert!(!is_canonical_decimal("+1"));
        assert!(!is_canonical_decimal(""));
    }

    #[test]
    fn uuid_shape() {
        assert!(is_lower_uuid("b1a9c0e9-d987-4042-ae91-78d6a3267d69"));
        assert!(!is_lower_uuid("B1A9C0E9-D987-4042-AE91-78D6A3267D69"));
        assert!(!is_lower_uuid("b1a9c0e9d9874042ae9178d6a3267d69"));
    }
}
