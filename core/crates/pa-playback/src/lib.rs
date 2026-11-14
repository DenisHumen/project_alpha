// SPDX-License-Identifier: MPL-2.0
//! Playback contract of the project_alpha core (PLAY-003, ADR-0003, ADR-0017).
//!
//! - [`PlayerEngine`]: the one contract every engine implements — Media3 on
//!   Android, libmpv on the desktop and as a fallback, AVFoundation and the
//!   own FFmpeg → VideoToolbox engine on Apple, and [`MockEngine`] in tests.
//!   Commands are non-blocking; results come as [`EngineEvent`]s, and
//!   [`EngineState`] is their fold.
//! - [`caps`]: the capability matrix (containers, codecs, HDR and Dolby
//!   Vision per engine and platform).
//! - [`select_engine`]: Auto selection along the platform chain
//!   (Android: Media3 → libmpv; Apple: AVFoundation → own engine → MPVKit)
//!   with a manual override.
//! - [`contract`]: the test suite every engine must pass; the mock passes it.
//! - [`session`]: the playback session over an engine (PLAY-005): resume or
//!   start over, progress saves, Up Next with a countdown.
//!
//! Types follow the `pa-model` rules (plain records and enums, milliseconds
//! as `u64`, serde), so `pa-ffi` and `pa-frb` can export them as they are.
//! The mapping to the Dart `PlayerEngine` of `pa_player` is in README.md.

pub mod caps;
pub mod contract;
mod engine;
mod mock;
mod select;
pub mod session;

pub use caps::{
    AudioCodec, Container, DynamicRange, EngineCapabilities, EngineKind, MediaProfile, Platform,
    RangeCapability, RangeSupport, SubtitleFormat, VideoCodec, auto_chain, default_candidates,
    default_capabilities,
};
pub use engine::{
    EngineError, EngineEvent, EngineListener, EngineState, MAX_REQUIRED_RATE, MAX_VOLUME,
    MIN_REQUIRED_RATE, MediaSource, OpenRequest, PlaybackStats, PlayerEngine, RepeatMode, Track,
    TrackKind, UNITY_VOLUME,
};
pub use mock::{MockEngine, MockMedia};
pub use select::{
    EnginePreference, Evaluation, Limitation, SelectError, Selection, evaluate, select_engine,
};
pub use session::{
    PlaybackSession, ProgressSave, SaveReason, SessionAction, SessionConfig, SessionError,
    StartMode, UpNext, start_position,
};
