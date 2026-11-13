// SPDX-License-Identifier: MPL-2.0
//! Capability matrix: what each engine plays on each platform.
//!
//! [`default_capabilities`] is the target matrix from ADR-0003 and
//! docs/research/02 §2.2–2.6. It assumes the planned engine work is done
//! (FFmpeg audio extension and libass overlay for Media3, the own Apple
//! engine through PLAY-APL-010). Platform layers refine it at runtime
//! (MediaCodecList, `AVPlayer.eligibleForHDRPlayback`, display HDR modes)
//! before calling [`crate::select_engine`].

use serde::{Deserialize, Serialize};

/// Playback engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EngineKind {
    /// AndroidX Media3 / ExoPlayer (PLAY-AND-002).
    Media3,
    /// libmpv: desktop (PLAY-DSK), Android fallback (PLAY-AND-007), MPVKit on
    /// Apple (PLAY-APL-001), media_kit in release 0.1.
    Mpv,
    /// AVPlayer (PLAY-APL-002).
    AvFoundation,
    /// Own Apple engine: FFmpeg demuxer → VideoToolbox →
    /// AVSampleBufferDisplayLayer (PLAY-APL-005…010).
    VideoToolbox,
    /// [`crate::MockEngine`] for tests.
    Mock,
}

/// Target platforms of the player.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Platform {
    Android,
    AndroidTv,
    /// iOS and iPadOS.
    Ios,
    TvOs,
    MacOs,
    Windows,
    Linux,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Container {
    Mkv,
    WebM,
    Mp4,
    Mov,
    MpegTs,
    /// Blu-ray transport stream.
    M2ts,
    Avi,
    /// HLS playlist (server remux or transcode).
    Hls,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    H264,
    Hevc,
    Av1,
    Vp9,
    Vp8,
    Mpeg2,
    /// MPEG-4 Part 2 (DivX, Xvid).
    Mpeg4,
    Vc1,
    ProRes,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AudioCodec {
    Aac,
    Mp3,
    Opus,
    Vorbis,
    Flac,
    Alac,
    Ac3,
    Eac3,
    TrueHd,
    /// DTS core.
    Dts,
    /// DTS-HD MA / DTS:X.
    DtsHd,
    Pcm,
    Other,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SubtitleFormat {
    Srt,
    WebVtt,
    Ass,
    Pgs,
    VobSub,
    DvbSub,
    Cea608,
    /// MP4 timed text (`tx3g`).
    MovText,
    Other,
}

/// Dynamic range of the video stream. Dolby Vision is split by profile
/// because engines differ per profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DynamicRange {
    Sdr,
    Hdr10,
    Hdr10Plus,
    Hlg,
    /// Profile 5 (IPTPQc2, no fallback layer).
    DolbyVision5,
    /// Profile 7 (dual layer, Blu-ray): HDR10 base layer.
    DolbyVision7,
    /// Profile 8.x: HDR10 (8.1), SDR (8.2) or HLG (8.4) base layer.
    DolbyVision8,
}

impl DynamicRange {
    pub fn is_dolby_vision(self) -> bool {
        matches!(
            self,
            Self::DolbyVision5 | Self::DolbyVision7 | Self::DolbyVision8
        )
    }
}

/// How an engine shows a dynamic range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RangeSupport {
    /// The signal reaches the display as is (HDMI HDR/DV mode, EDR).
    Native,
    /// Correct colors, tone-mapped by the engine (libplacebo).
    ToneMapped,
}

/// Properties of a file that decide the engine. Filled from `pa-probe`
/// (library scan) or from the server's media info.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaProfile {
    pub container: Container,
    /// `None` for audio-only files.
    pub video_codec: Option<VideoCodec>,
    pub dynamic_range: DynamicRange,
    /// Base layer of a Dolby Vision stream, if it has one: `Hdr10` for P7 and
    /// P8.1, `Sdr` for P8.2, `Hlg` for P8.4.
    pub dv_base_layer: Option<DynamicRange>,
    /// Codecs of all audio tracks.
    pub audio_codecs: Vec<AudioCodec>,
    /// Formats of all subtitle tracks, embedded and sidecar.
    pub subtitle_formats: Vec<SubtitleFormat>,
}

impl Default for MediaProfile {
    fn default() -> Self {
        Self {
            container: Container::Other,
            video_codec: None,
            dynamic_range: DynamicRange::Sdr,
            dv_base_layer: None,
            audio_codecs: Vec::new(),
            subtitle_formats: Vec::new(),
        }
    }
}

/// One supported dynamic range.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct RangeCapability {
    pub range: DynamicRange,
    pub support: RangeSupport,
}

/// What one engine plays on one platform.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EngineCapabilities {
    pub engine: EngineKind,
    pub platform: Platform,
    pub containers: Vec<Container>,
    pub video_codecs: Vec<VideoCodec>,
    pub audio_codecs: Vec<AudioCodec>,
    pub subtitle_formats: Vec<SubtitleFormat>,
    /// Ranges other than SDR; SDR is always supported.
    pub dynamic_ranges: Vec<RangeCapability>,
}

impl EngineCapabilities {
    /// Support for `range`, `None` if the engine cannot show it correctly.
    pub fn range_support(&self, range: DynamicRange) -> Option<RangeSupport> {
        if range == DynamicRange::Sdr {
            return Some(RangeSupport::Native);
        }
        self.dynamic_ranges
            .iter()
            .find(|c| c.range == range)
            .map(|c| c.support)
    }
}

/// Engines tried by Auto on `platform`, best first (ADR-0003).
pub fn auto_chain(platform: Platform) -> &'static [EngineKind] {
    use EngineKind::*;
    match platform {
        Platform::Android | Platform::AndroidTv => &[Media3, Mpv],
        Platform::Ios | Platform::TvOs | Platform::MacOs => &[AvFoundation, VideoToolbox, Mpv],
        Platform::Windows | Platform::Linux => &[Mpv],
    }
}

/// Built-in capabilities of the engines in [`auto_chain`]`(platform)`, in
/// chain order.
pub fn default_candidates(platform: Platform) -> Vec<EngineCapabilities> {
    auto_chain(platform)
        .iter()
        .filter_map(|&engine| default_capabilities(engine, platform))
        .collect()
}

/// Built-in capabilities of `engine` on `platform`; `None` if the engine
/// does not exist there. The mock plays everything everywhere.
pub fn default_capabilities(engine: EngineKind, platform: Platform) -> Option<EngineCapabilities> {
    use AudioCodec as A;
    use Container as C;
    use DynamicRange as R;
    use RangeSupport::{Native, ToneMapped};
    use SubtitleFormat as S;
    use VideoCodec as V;

    const ALL_C: &[C] = &[
        C::Mkv,
        C::WebM,
        C::Mp4,
        C::Mov,
        C::MpegTs,
        C::M2ts,
        C::Avi,
        C::Hls,
        C::Other,
    ];
    const ALL_V: &[V] = &[
        V::H264,
        V::Hevc,
        V::Av1,
        V::Vp9,
        V::Vp8,
        V::Mpeg2,
        V::Mpeg4,
        V::Vc1,
        V::ProRes,
        V::Other,
    ];
    const ALL_A: &[A] = &[
        A::Aac,
        A::Mp3,
        A::Opus,
        A::Vorbis,
        A::Flac,
        A::Alac,
        A::Ac3,
        A::Eac3,
        A::TrueHd,
        A::Dts,
        A::DtsHd,
        A::Pcm,
        A::Other,
    ];
    const ALL_S: &[S] = &[
        S::Srt,
        S::WebVtt,
        S::Ass,
        S::Pgs,
        S::VobSub,
        S::DvbSub,
        S::Cea608,
        S::MovText,
        S::Other,
    ];
    let apple = matches!(platform, Platform::Ios | Platform::TvOs | Platform::MacOs);
    let android = matches!(platform, Platform::Android | Platform::AndroidTv);

    type Row = (
        &'static [C],
        &'static [V],
        &'static [A],
        &'static [S],
        Vec<(R, RangeSupport)>,
    );
    let (containers, video, audio, subs, ranges): Row = match engine {
        EngineKind::Mock => (
            ALL_C,
            ALL_V,
            ALL_A,
            ALL_S,
            [
                R::Hdr10,
                R::Hdr10Plus,
                R::Hlg,
                R::DolbyVision5,
                R::DolbyVision7,
                R::DolbyVision8,
            ]
            .map(|r| (r, Native))
            .to_vec(),
        ),
        // MediaCodec + FFmpeg audio extension (PLAY-AND-004); ASS, PGS
        // and VobSub through the core overlay (PLAY-AND-006). P7 only on
        // some SoCs: platforms add it at runtime.
        EngineKind::Media3 if android => (
            &[C::Mkv, C::WebM, C::Mp4, C::Mov, C::MpegTs, C::Hls],
            &[V::H264, V::Hevc, V::Av1, V::Vp9, V::Vp8, V::Mpeg2, V::Mpeg4],
            &[
                A::Aac,
                A::Mp3,
                A::Opus,
                A::Vorbis,
                A::Flac,
                A::Alac,
                A::Ac3,
                A::Eac3,
                A::TrueHd,
                A::Dts,
                A::DtsHd,
                A::Pcm,
            ],
            &[
                S::Srt,
                S::WebVtt,
                S::Ass,
                S::Pgs,
                S::VobSub,
                S::DvbSub,
                S::Cea608,
                S::MovText,
            ],
            [
                R::Hdr10,
                R::Hdr10Plus,
                R::Hlg,
                R::DolbyVision5,
                R::DolbyVision8,
            ]
            .map(|r| (r, Native))
            .to_vec(),
        ),
        EngineKind::Media3 => return None,
        // AVPlayer: no MKV, no ASS/PGS, no TrueHD/DTS; native HDR, DV
        // and Match Content.
        EngineKind::AvFoundation if apple => (
            &[C::Mp4, C::Mov, C::Hls],
            &[V::H264, V::Hevc, V::Av1, V::ProRes],
            &[
                A::Aac,
                A::Mp3,
                A::Flac,
                A::Alac,
                A::Ac3,
                A::Eac3,
                A::Pcm,
                A::Opus,
            ],
            &[S::WebVtt, S::Cea608, S::MovText],
            [
                R::Hdr10,
                R::Hdr10Plus,
                R::Hlg,
                R::DolbyVision5,
                R::DolbyVision8,
            ]
            .map(|r| (r, Native))
            .to_vec(),
        ),
        // Own engine: FFmpeg demux and software fallback, VideoToolbox,
        // core subtitle overlay; P7 converted to P8.1 (PLAY-APL-007).
        EngineKind::VideoToolbox if apple => (
            ALL_C,
            ALL_V,
            ALL_A,
            ALL_S,
            [
                R::Hdr10,
                R::Hlg,
                R::DolbyVision5,
                R::DolbyVision7,
                R::DolbyVision8,
            ]
            .map(|r| (r, Native))
            .to_vec(),
        ),
        EngineKind::AvFoundation | EngineKind::VideoToolbox => return None,
        // libmpv plays everything; HDR goes to the display only through
        // the native desktop surface (ADR-0009), DV is always tone-mapped
        // by libplacebo. On Android and Apple everything is tone-mapped.
        EngineKind::Mpv => {
            let hdr = if apple || android { ToneMapped } else { Native };
            (
                ALL_C,
                ALL_V,
                ALL_A,
                ALL_S,
                vec![
                    (R::Hdr10, hdr),
                    (R::Hdr10Plus, hdr),
                    (R::Hlg, hdr),
                    (R::DolbyVision5, ToneMapped),
                    (R::DolbyVision7, ToneMapped),
                    (R::DolbyVision8, ToneMapped),
                ],
            )
        }
    };

    Some(EngineCapabilities {
        engine,
        platform,
        containers: containers.to_vec(),
        video_codecs: video.to_vec(),
        audio_codecs: audio.to_vec(),
        subtitle_formats: subs.to_vec(),
        dynamic_ranges: ranges
            .into_iter()
            .map(|(range, support)| RangeCapability { range, support })
            .collect(),
    })
}
