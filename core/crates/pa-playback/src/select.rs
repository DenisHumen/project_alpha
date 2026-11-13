// SPDX-License-Identifier: MPL-2.0
//! Engine selection: Auto by the capability matrix, or a manual override.
//!
//! Auto (ADR-0003, docs/research/02 §2.8) walks the platform chain and takes
//! the first engine that plays the file with no limitation, HDR and Dolby
//! Vision included. If every engine has a limitation, it takes the engine
//! whose worst limitation is the mildest ([`Limitation::severity`]); ties go
//! to the earlier engine in the chain.

use serde::{Deserialize, Serialize};

use crate::caps::{
    AudioCodec, Container, DynamicRange, EngineCapabilities, EngineKind, MediaProfile,
    RangeSupport, SubtitleFormat, VideoCodec,
};

/// Why an engine plays a file worse than possible, or not at all.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum Limitation {
    /// Blocking.
    ContainerUnsupported { container: Container },
    /// Blocking.
    VideoCodecUnsupported { codec: VideoCodec },
    /// Blocking: the stream would show wrong colors.
    DynamicRangeUnsupported { range: DynamicRange },
    /// Blocking: no audio track is playable.
    NoPlayableAudio,
    /// Some audio tracks cannot be played.
    AudioCodecUnsupported { codec: AudioCodec },
    /// Shown as its base layer: Dolby Vision as HDR10/HLG/SDR, HDR10+ as
    /// HDR10.
    BaseLayerOnly {
        range: DynamicRange,
        shown_as: DynamicRange,
    },
    /// Tone-mapped instead of sent to the display as is.
    ToneMapped { range: DynamicRange },
    /// A subtitle format cannot be shown.
    SubtitleFormatUnsupported { format: SubtitleFormat },
}

impl Limitation {
    /// `None` = blocking. Otherwise a higher number is a worse experience.
    pub fn severity(&self) -> Option<u8> {
        match self {
            Self::ContainerUnsupported { .. }
            | Self::VideoCodecUnsupported { .. }
            | Self::DynamicRangeUnsupported { .. }
            | Self::NoPlayableAudio => None,
            Self::SubtitleFormatUnsupported { .. } => Some(1),
            Self::AudioCodecUnsupported { .. } => Some(2),
            Self::BaseLayerOnly { range, .. } => Some(if range.is_dolby_vision() { 3 } else { 1 }),
            Self::ToneMapped { .. } => Some(4),
        }
    }

    pub fn is_blocking(&self) -> bool {
        self.severity().is_none()
    }
}

/// Result of checking one engine against one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Evaluation {
    pub engine: EngineKind,
    /// Empty: the engine plays the file at full quality.
    pub limitations: Vec<Limitation>,
}

impl Evaluation {
    pub fn is_full(&self) -> bool {
        self.limitations.is_empty()
    }

    pub fn is_playable(&self) -> bool {
        !self.limitations.iter().any(Limitation::is_blocking)
    }

    /// Worst non-blocking severity, `0` when full. Meaningless if blocked.
    fn worst(&self) -> u8 {
        self.limitations
            .iter()
            .filter_map(Limitation::severity)
            .max()
            .unwrap_or(0)
    }
}

/// Checks `profile` against `caps`.
pub fn evaluate(caps: &EngineCapabilities, profile: &MediaProfile) -> Evaluation {
    let mut limitations = Vec::new();
    if !caps.containers.contains(&profile.container) {
        limitations.push(Limitation::ContainerUnsupported {
            container: profile.container,
        });
    }
    if let Some(codec) = profile.video_codec {
        if !caps.video_codecs.contains(&codec) {
            limitations.push(Limitation::VideoCodecUnsupported { codec });
        }
        if let Some(l) = range_limitation(caps, profile) {
            limitations.push(l);
        }
    }
    let unsupported_audio: Vec<AudioCodec> = dedup(&profile.audio_codecs)
        .into_iter()
        .filter(|c| !caps.audio_codecs.contains(c))
        .collect();
    if !profile.audio_codecs.is_empty()
        && profile
            .audio_codecs
            .iter()
            .all(|c| unsupported_audio.contains(c))
    {
        limitations.push(Limitation::NoPlayableAudio);
    } else {
        limitations.extend(
            unsupported_audio
                .into_iter()
                .map(|codec| Limitation::AudioCodecUnsupported { codec }),
        );
    }
    limitations.extend(
        dedup(&profile.subtitle_formats)
            .into_iter()
            .filter(|f| !caps.subtitle_formats.contains(f))
            .map(|format| Limitation::SubtitleFormatUnsupported { format }),
    );
    Evaluation {
        engine: caps.engine,
        limitations,
    }
}

fn range_limitation(caps: &EngineCapabilities, profile: &MediaProfile) -> Option<Limitation> {
    let range = profile.dynamic_range;
    match caps.range_support(range) {
        Some(RangeSupport::Native) => return None,
        Some(RangeSupport::ToneMapped) => return Some(Limitation::ToneMapped { range }),
        None => {}
    }
    // Fall back to what the stream also carries.
    let base = match range {
        DynamicRange::Hdr10Plus => Some(DynamicRange::Hdr10),
        r if r.is_dolby_vision() => profile.dv_base_layer,
        _ => None,
    };
    match base.map(|b| (b, caps.range_support(b))) {
        Some((shown_as, Some(RangeSupport::Native))) => {
            Some(Limitation::BaseLayerOnly { range, shown_as })
        }
        Some((shown_as, Some(RangeSupport::ToneMapped))) => {
            Some(Limitation::ToneMapped { range: shown_as })
        }
        _ => Some(Limitation::DynamicRangeUnsupported { range }),
    }
}

fn dedup<T: Copy + PartialEq>(items: &[T]) -> Vec<T> {
    let mut out: Vec<T> = Vec::with_capacity(items.len());
    for &item in items {
        if !out.contains(&item) {
            out.push(item);
        }
    }
    out
}

/// User setting "Player engine".
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "mode", content = "engine")]
pub enum EnginePreference {
    #[default]
    Auto,
    /// Always this engine if the platform has it, even when the matrix says
    /// it cannot play the file (the matrix may be conservative).
    Manual(EngineKind),
}

/// The chosen engine and why.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Selection {
    pub engine: EngineKind,
    /// Chosen by [`EnginePreference::Manual`].
    pub manual: bool,
    /// Every candidate, in chain order: for the stats overlay and logs.
    pub evaluations: Vec<Evaluation>,
}

impl Selection {
    /// Evaluation of the chosen engine.
    pub fn chosen(&self) -> &Evaluation {
        self.evaluations
            .iter()
            .find(|e| e.engine == self.engine)
            .expect("the chosen engine is a candidate")
    }
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SelectError {
    #[error("no player engine is available")]
    NoCandidates,
    #[error("engine {engine:?} is not available on this platform")]
    EngineNotAvailable { engine: EngineKind },
    #[error("no engine can play this file")]
    NoPlayableEngine { evaluations: Vec<Evaluation> },
}

/// Chooses an engine for `profile` among `candidates` (chain order, usually
/// [`crate::default_candidates`] refined by the platform).
pub fn select_engine(
    candidates: &[EngineCapabilities],
    profile: &MediaProfile,
    preference: EnginePreference,
) -> Result<Selection, SelectError> {
    if candidates.is_empty() {
        return Err(SelectError::NoCandidates);
    }
    let evaluations: Vec<Evaluation> = candidates.iter().map(|c| evaluate(c, profile)).collect();
    let (engine, manual) = match preference {
        EnginePreference::Manual(engine) => {
            if !candidates.iter().any(|c| c.engine == engine) {
                return Err(SelectError::EngineNotAvailable { engine });
            }
            (engine, true)
        }
        EnginePreference::Auto => {
            // min_by_key keeps the first of equal keys: chain order wins ties.
            let best = evaluations
                .iter()
                .filter(|e| e.is_playable())
                .min_by_key(|e| e.worst());
            match best {
                Some(e) => (e.engine, false),
                None => return Err(SelectError::NoPlayableEngine { evaluations }),
            }
        }
    };
    Ok(Selection {
        engine,
        manual,
        evaluations,
    })
}
