// SPDX-License-Identifier: MPL-2.0
//! What a probe reports. Plain data, serialisable to JSON.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// Everything [`probe`](crate::probe) learned about one file.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct MediaInfo {
    /// libavformat demuxer name, e.g. `matroska,webm`, `mov,mp4,m4a,3gp,3g2,mj2`, `mpegts`.
    pub container: String,
    /// Human-readable container name, e.g. `Matroska / WebM`.
    pub container_long_name: String,
    /// File size in bytes.
    pub size_bytes: u64,
    /// Duration in milliseconds, if known.
    pub duration_ms: Option<u64>,
    /// How reliable [`duration_ms`](Self::duration_ms) is.
    pub duration_source: Option<DurationSource>,
    /// Overall bit rate, bits per second.
    pub bit_rate: Option<u64>,
    /// Video streams in file order; cover art is included with
    /// [`VideoStream::cover_art`] set.
    pub video: Vec<VideoStream>,
    /// Audio streams in file order.
    pub audio: Vec<AudioStream>,
    /// Subtitle streams in file order.
    pub subtitles: Vec<SubtitleStream>,
    /// Attached files (fonts in MKV and so on).
    pub attachments: Vec<Attachment>,
    /// Chapters in time order.
    pub chapters: Vec<Chapter>,
    /// Container-level tags; keys in lowercase (`title`, `encoder`,
    /// `creation_time`…), first value wins on duplicates.
    pub tags: BTreeMap<String, String>,
    /// Bytes read from the source.
    pub bytes_read: u64,
    /// `true` if libavformat wanted to read outside the head/tail window,
    /// so fields such as a bit-rate based duration may be estimates.
    pub window_truncated: bool,
}

impl MediaInfo {
    /// The main video stream: the first one that is not cover art.
    #[must_use]
    pub fn main_video(&self) -> Option<&VideoStream> {
        self.video.iter().find(|v| !v.cover_art)
    }

    /// Languages of the audio streams in file order, without duplicates
    /// (ISO 639-2 codes as stored in the file).
    #[must_use]
    pub fn audio_languages(&self) -> Vec<String> {
        dedup(self.audio.iter().filter_map(|a| a.common.language.clone()))
    }

    /// Languages of the subtitle streams in file order, without duplicates.
    #[must_use]
    pub fn subtitle_languages(&self) -> Vec<String> {
        dedup(
            self.subtitles
                .iter()
                .filter_map(|s| s.common.language.clone()),
        )
    }
}

fn dedup(items: impl Iterator<Item = String>) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// Where the duration comes from (libavformat `AVDurationEstimationMethod`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DurationSource {
    /// From timestamps (accurate).
    Pts,
    /// From the stream or container header (accurate for sane files).
    Stream,
    /// Estimated from the file size and bit rate: may be off by a lot.
    Bitrate,
}

/// Fields shared by every stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct StreamCommon {
    /// Stream index in the container.
    pub index: u32,
    /// FFmpeg codec name: `h264`, `hevc`, `av1`, `aac`, `eac3`, `subrip`…
    pub codec: String,
    /// Codec profile name, e.g. `Main 10`, `High`, `LC`, `Dolby Digital Plus + Dolby Atmos`.
    pub profile: Option<String>,
    /// Language (ISO 639-2 as stored, e.g. `eng`, `rus`); `und` is dropped.
    pub language: Option<String>,
    /// Track title.
    pub title: Option<String>,
    /// Default track.
    pub default: bool,
    /// Forced track (forced subtitles, forced narrative).
    pub forced: bool,
    /// Commentary track.
    pub comment: bool,
    /// For the hearing impaired (SDH).
    pub hearing_impaired: bool,
    /// For the visually impaired (audio description).
    pub visual_impaired: bool,
    /// Original language track.
    pub original: bool,
    /// Dubbed track.
    pub dub: bool,
}

/// A video stream.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct VideoStream {
    /// Codec, language and flags.
    #[serde(flatten)]
    pub common: StreamCommon,
    /// Codec tag as text if printable (`hvc1`, `dvh1`, `avc1`).
    pub codec_tag: Option<String>,
    /// Coded width in pixels.
    pub width: u32,
    /// Coded height in pixels.
    pub height: u32,
    /// Sample aspect ratio, if not square.
    pub sample_aspect_ratio: Option<(u32, u32)>,
    /// Average frame rate as a fraction, e.g. `(24000, 1001)`.
    pub frame_rate: Option<(u32, u32)>,
    /// Pixel format, e.g. `yuv420p10le`.
    pub pix_fmt: Option<String>,
    /// Bits per component.
    pub bit_depth: Option<u8>,
    /// Interlaced (any field order other than progressive), if known.
    pub interlaced: Option<bool>,
    /// Colour primaries, e.g. `bt709`, `bt2020`.
    pub color_primaries: Option<String>,
    /// Transfer characteristics, e.g. `bt709`, `smpte2084`, `arib-std-b67`.
    pub color_transfer: Option<String>,
    /// Matrix coefficients, e.g. `bt2020nc`.
    pub color_space: Option<String>,
    /// `tv` (limited) or `pc` (full).
    pub color_range: Option<String>,
    /// HDR formats, strongest first (e.g. `[DolbyVision, Hdr10]` for DV 8.1);
    /// empty for SDR.
    pub hdr: Vec<HdrFormat>,
    /// Dolby Vision configuration, if any.
    pub dolby_vision: Option<DolbyVision>,
    /// Static HDR metadata: mastering display (SMPTE ST 2086).
    pub mastering_display: bool,
    /// Static HDR metadata: content light level (MaxCLL/MaxFALL).
    pub content_light_level: bool,
    /// Cover art or thumbnail (`attached_pic`), not a real video track.
    pub cover_art: bool,
}

impl VideoStream {
    /// The strongest HDR format, `None` for SDR.
    #[must_use]
    pub fn hdr_format(&self) -> Option<HdrFormat> {
        self.hdr.first().copied()
    }
}

/// An HDR format of a video stream.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrFormat {
    /// Dolby Vision (profile in [`DolbyVision`]).
    DolbyVision,
    /// HDR10+ (SMPTE ST 2094-40 dynamic metadata).
    Hdr10Plus,
    /// HDR10: PQ transfer (SMPTE ST 2084), usually with static metadata.
    Hdr10,
    /// Hybrid Log-Gamma (ARIB STD-B67).
    Hlg,
}

/// Dolby Vision configuration (`dvcC`/`dvvC` record or RPUs in the bitstream).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct DolbyVision {
    /// Profile (4, 5, 7, 8, 9, 10…); `None` if only RPUs were seen.
    pub profile: Option<u8>,
    /// Level.
    pub level: Option<u8>,
    /// Base-layer signal compatibility id: 0 none (profile 5), 1 HDR10,
    /// 2 SDR, 4 HLG, 6 BT.2100 PQ (Blu-ray).
    pub compatibility_id: Option<u8>,
    /// RPU present.
    pub rpu: bool,
    /// Enhancement layer present (profile 7 FEL/MEL).
    pub enhancement_layer: bool,
    /// Base layer present.
    pub base_layer: bool,
}

impl DolbyVision {
    /// Conventional name: `8.1`, `5`, `7.6`… `None` without a profile.
    #[must_use]
    pub fn profile_name(&self) -> Option<String> {
        let profile = self.profile?;
        Some(match self.compatibility_id {
            Some(id) if id != 0 && profile >= 7 => format!("{profile}.{id}"),
            _ => profile.to_string(),
        })
    }
}

/// An audio stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct AudioStream {
    /// Codec, language and flags.
    #[serde(flatten)]
    pub common: StreamCommon,
    /// Number of channels.
    pub channels: u32,
    /// Channel layout, e.g. `stereo`, `5.1(side)`, `7.1`.
    pub channel_layout: Option<String>,
    /// Sample rate, Hz.
    pub sample_rate: Option<u32>,
    /// Bits per sample of lossless/PCM audio.
    pub bit_depth: Option<u8>,
    /// Bit rate, bits per second, if declared.
    pub bit_rate: Option<u64>,
    /// Object audio: Dolby Atmos (E-AC-3 JOC, TrueHD) or DTS:X, as
    /// reported by FFmpeg's codec profile.
    pub object_audio: Option<ObjectAudio>,
}

/// Object-based audio formats.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ObjectAudio {
    /// Dolby Atmos in E-AC-3 (JOC) or TrueHD.
    Atmos,
    /// DTS:X in DTS-HD MA.
    DtsX,
}

/// A subtitle stream.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SubtitleStream {
    /// Codec, language and flags.
    #[serde(flatten)]
    pub common: StreamCommon,
    /// Text (`subrip`, `ass`, `mov_text`, `webvtt`) or bitmap (`hdmv_pgs_subtitle`,
    /// `dvd_subtitle`, `dvb_subtitle`).
    pub kind: SubtitleKind,
}

/// Text or bitmap subtitles.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleKind {
    /// Text that can be restyled.
    Text,
    /// Pictures.
    Bitmap,
    /// Unknown to this FFmpeg build.
    #[default]
    Unknown,
}

/// An attached file.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Attachment {
    /// Stream index in the container.
    pub index: u32,
    /// File name, e.g. `DejaVuSans.ttf`.
    pub filename: Option<String>,
    /// MIME type, e.g. `font/ttf`, `application/x-truetype-font`.
    pub mime_type: Option<String>,
    /// FFmpeg codec name (`ttf`, `otf`, `bin_data`…).
    pub codec: String,
    /// Size in bytes, if the data was read.
    pub size_bytes: Option<u64>,
}

/// A chapter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Chapter {
    /// Start, milliseconds.
    pub start_ms: u64,
    /// End, milliseconds.
    pub end_ms: u64,
    /// Title, if any.
    pub title: Option<String>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dolby_vision_profile_names() {
        let dv = |profile, compat| DolbyVision {
            profile: Some(profile),
            compatibility_id: Some(compat),
            ..DolbyVision::default()
        };
        assert_eq!(dv(8, 1).profile_name().as_deref(), Some("8.1"));
        assert_eq!(dv(8, 4).profile_name().as_deref(), Some("8.4"));
        assert_eq!(dv(5, 0).profile_name().as_deref(), Some("5"));
        assert_eq!(dv(7, 6).profile_name().as_deref(), Some("7.6"));
        assert_eq!(DolbyVision::default().profile_name(), None);
    }

    #[test]
    fn languages_and_main_video() {
        let audio = |lang: &str| AudioStream {
            common: StreamCommon {
                language: Some(lang.into()),
                ..StreamCommon::default()
            },
            ..AudioStream::default()
        };
        let info = MediaInfo {
            video: vec![
                VideoStream {
                    cover_art: true,
                    ..VideoStream::default()
                },
                VideoStream {
                    width: 1920,
                    ..VideoStream::default()
                },
            ],
            audio: vec![audio("eng"), audio("rus"), audio("eng")],
            ..MediaInfo::default()
        };
        assert_eq!(info.main_video().unwrap().width, 1920);
        assert_eq!(info.audio_languages(), ["eng", "rus"]);
        assert!(info.subtitle_languages().is_empty());
    }

    #[test]
    fn serialises_to_json() {
        let v = VideoStream {
            common: StreamCommon {
                codec: "hevc".into(),
                ..StreamCommon::default()
            },
            hdr: vec![HdrFormat::DolbyVision, HdrFormat::Hdr10],
            ..VideoStream::default()
        };
        let json = serde_json::to_value(&v).unwrap();
        assert_eq!(json["codec"], "hevc");
        assert_eq!(json["hdr"], serde_json::json!(["dolby_vision", "hdr10"]));
        let back: VideoStream = serde_json::from_value(json).unwrap();
        assert_eq!(back, v);
    }
}
