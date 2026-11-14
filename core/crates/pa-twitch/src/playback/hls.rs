// SPDX-License-Identifier: MPL-2.0
//! Twitch master playlist variants (06 §2.1 step 3), full builds only.
//!
//! Only the master playlist is read: media playlists, ad segments and
//! `EXT-X-DATERANGE` markers go to the player untouched (TWI-006).

use std::collections::HashMap;

use super::PlaybackError;

/// One rendition of the master playlist.
#[derive(Debug, Clone, PartialEq)]
pub struct Variant {
    /// `VIDEO` group id (`chunked`, `720p60`, `audio_only`, …).
    pub group_id: String,
    /// Quality label without the `(source)` suffix (`1080p60`, `audio_only`).
    pub quality: String,
    /// Original quality (Twitch group `chunked`).
    pub is_source: bool,
    /// Audio-only rendition.
    pub audio_only: bool,
    /// Peak bits per second.
    pub bandwidth: u64,
    /// Width × height, when given.
    pub resolution: Option<(u32, u32)>,
    /// Frames per second, when given.
    pub frame_rate: Option<f32>,
    /// RFC 6381 codec strings (`avc1.64002A`, `mp4a.40.2`, …).
    pub codecs: Vec<String>,
    /// Media playlist URL.
    pub url: String,
}

impl Variant {
    /// Picture height (0 for audio-only or unknown).
    pub fn height(&self) -> u32 {
        self.resolution.map_or(0, |(_, h)| h)
    }
}

/// Which variant to pick.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum QualityPreference {
    /// The original (source) quality.
    Source,
    /// The best video variant not taller than this many lines.
    AtMost(u32),
    /// Audio only.
    AudioOnly,
}

/// A parsed master playlist; variants keep the playlist order.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct MasterPlaylist {
    /// All renditions.
    pub variants: Vec<Variant>,
}

impl MasterPlaylist {
    /// Parses usher's master playlist.
    pub fn parse(text: &str) -> Result<Self, PlaybackError> {
        let mut lines = text.lines().map(str::trim).filter(|l| !l.is_empty());
        if lines.next() != Some("#EXTM3U") {
            return Err(PlaybackError::Decode("not an HLS playlist".into()));
        }
        let mut names: HashMap<String, String> = HashMap::new();
        let mut pending: Option<HashMap<String, String>> = None;
        let mut variants = Vec::new();
        for line in lines {
            if let Some(attrs) = line.strip_prefix("#EXT-X-MEDIA:") {
                let attrs = attributes(attrs);
                if attrs.get("TYPE").map(String::as_str) == Some("VIDEO")
                    && let (Some(group), Some(name)) = (attrs.get("GROUP-ID"), attrs.get("NAME"))
                {
                    names.insert(group.clone(), name.clone());
                }
            } else if let Some(attrs) = line.strip_prefix("#EXT-X-STREAM-INF:") {
                pending = Some(attributes(attrs));
            } else if line.starts_with('#') {
                // Other tags and comments.
            } else if let Some(attrs) = pending.take() {
                variants.push(variant(&attrs, &names, line)?);
            }
        }
        if variants.is_empty() {
            return Err(PlaybackError::Decode("master playlist has no variants".into()));
        }
        Ok(Self { variants })
    }

    /// The source variant, else the video variant with the most bandwidth.
    pub fn source(&self) -> Option<&Variant> {
        self.variants.iter().find(|v| v.is_source).or_else(|| {
            self.video()
                .max_by_key(|v| (v.height(), v.bandwidth))
        })
    }

    /// The audio-only variant, if Twitch offers one.
    pub fn audio_only(&self) -> Option<&Variant> {
        self.variants.iter().find(|v| v.audio_only)
    }

    /// Picks a variant; `AtMost` falls back to the smallest video variant.
    pub fn select(&self, preference: QualityPreference) -> Option<&Variant> {
        match preference {
            QualityPreference::Source => self.source(),
            QualityPreference::AudioOnly => self.audio_only(),
            QualityPreference::AtMost(max) => self
                .video()
                .filter(|v| v.height() <= max)
                .max_by_key(|v| (v.height(), v.bandwidth))
                .or_else(|| self.video().min_by_key(|v| (v.height(), v.bandwidth))),
        }
    }

    fn video(&self) -> impl Iterator<Item = &Variant> {
        self.variants.iter().filter(|v| !v.audio_only)
    }
}

fn variant(
    attrs: &HashMap<String, String>,
    names: &HashMap<String, String>,
    url: &str,
) -> Result<Variant, PlaybackError> {
    let bandwidth = attrs
        .get("BANDWIDTH")
        .and_then(|b| b.parse().ok())
        .ok_or_else(|| PlaybackError::Decode("variant without BANDWIDTH".into()))?;
    let resolution = attrs.get("RESOLUTION").and_then(|r| {
        let (w, h) = r.split_once('x')?;
        Some((w.parse().ok()?, h.parse().ok()?))
    });
    let frame_rate = attrs.get("FRAME-RATE").and_then(|f| f.parse().ok());
    let group_id = attrs.get("VIDEO").cloned().unwrap_or_default();
    let name = names.get(&group_id).cloned().unwrap_or_else(|| match resolution {
        Some((_, h)) => format!("{h}p"),
        None => group_id.clone(),
    });
    let is_source = group_id == "chunked" || name.contains("(source)");
    let quality = name.replace("(source)", "").trim().to_owned();
    Ok(Variant {
        audio_only: group_id == "audio_only",
        group_id,
        quality,
        is_source,
        bandwidth,
        resolution,
        frame_rate,
        codecs: attrs
            .get("CODECS")
            .map(|c| c.split(',').map(|s| s.trim().to_owned()).collect())
            .unwrap_or_default(),
        url: url.to_owned(),
    })
}

/// Parses an HLS attribute list (`KEY=value,KEY="quoted, value"`).
fn attributes(list: &str) -> HashMap<String, String> {
    let mut out = HashMap::new();
    let mut rest = list;
    while !rest.is_empty() {
        let Some((key, after)) = rest.split_once('=') else {
            break;
        };
        let (value, next) = if let Some(quoted) = after.strip_prefix('"') {
            match quoted.split_once('"') {
                Some((v, n)) => (v, n.strip_prefix(',').unwrap_or(n)),
                None => (quoted, ""),
            }
        } else {
            match after.split_once(',') {
                Some((v, n)) => (v, n),
                None => (after, ""),
            }
        };
        out.insert(key.trim().to_owned(), value.to_owned());
        rest = next;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attribute_list_with_quoted_commas() {
        let a = attributes(r#"BANDWIDTH=10,CODECS="avc1.4D401F,mp4a.40.2",VIDEO="720p30""#);
        assert_eq!(a["BANDWIDTH"], "10");
        assert_eq!(a["CODECS"], "avc1.4D401F,mp4a.40.2");
        assert_eq!(a["VIDEO"], "720p30");
    }

    #[test]
    fn rejects_non_playlists() {
        assert!(MasterPlaylist::parse("<html>").is_err());
        assert!(MasterPlaylist::parse("#EXTM3U\n").is_err());
    }
}
