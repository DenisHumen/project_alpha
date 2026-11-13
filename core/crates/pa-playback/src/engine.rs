// SPDX-License-Identifier: MPL-2.0
//! The [`PlayerEngine`] contract: commands, events and the state they build.
//!
//! Rules every engine follows (checked by [`crate::contract`]):
//!
//! - **Commands do not block.** A command validates its arguments and the
//!   engine state, starts the work and returns. The outcome arrives as
//!   [`EngineEvent`]s on the listener. A synchronous `Err` means the command
//!   was rejected and nothing changed.
//! - **State is the fold of events.** [`PlayerEngine::state`] always equals
//!   [`EngineState::default`] with every event sent so far applied by
//!   [`EngineState::apply`]. An engine never changes its state silently, so a
//!   UI that only listens to events sees exactly what `state()` returns.
//! - **Listener calls come without engine locks held** and may call back into
//!   the engine. They arrive on an engine thread: return quickly.
//! - Times are milliseconds (`u64`), as in `pa-model`.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::caps::EngineKind;

/// Volume that leaves the signal unchanged, in percent.
pub const UNITY_VOLUME: f64 = 100.0;
/// Highest volume an engine may accept (Volume Boost, PLAY-004), in percent.
/// Engines without boost clamp to [`UNITY_VOLUME`] and report the result.
pub const MAX_VOLUME: f64 = 300.0;
/// Slowest rate every engine must support.
pub const MIN_REQUIRED_RATE: f64 = 0.5;
/// Fastest rate every engine must support.
pub const MAX_REQUIRED_RATE: f64 = 2.0;

/// One playable item.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct MediaSource {
    /// `file://` or a plain path, `http(s)://` (including the loopback
    /// `pa-proxy` that serves a `ByteSource`, NET-008), or a remux/HLS URL of
    /// a media server.
    pub uri: String,
    /// Title for system media controls; optional.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
}

impl MediaSource {
    pub fn new(uri: impl Into<String>) -> Self {
        Self {
            uri: uri.into(),
            title: None,
        }
    }
}

/// What [`PlayerEngine::open`] plays.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OpenRequest {
    /// Playlist; must not be empty. Video usually passes one item.
    pub items: Vec<MediaSource>,
    /// Item to start with.
    pub index: u32,
    /// Resume position inside `items[index]`; `0` starts from the beginning.
    pub start_position_ms: u64,
    /// Start playing once loaded; `false` opens paused.
    pub play: bool,
}

impl OpenRequest {
    /// One item from the beginning, playing.
    pub fn single(source: MediaSource) -> Self {
        Self {
            items: vec![source],
            index: 0,
            start_position_ms: 0,
            play: true,
        }
    }
}

/// Playlist repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RepeatMode {
    #[default]
    Off,
    All,
    One,
}

/// Kind of a selectable track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum TrackKind {
    Audio,
    Subtitle,
}

/// An audio or subtitle track.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct Track {
    /// Engine-specific id, stable while the item is open.
    pub id: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    /// ISO 639 / BCP 47 code as found in the container.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub language: Option<String>,
    /// Codec name in FFmpeg spelling (`ac3`, `truehd`, `subrip`, `ass`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    /// Audio channel count.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub channels: Option<u32>,
    /// Loaded from a separate file.
    pub external: bool,
    /// Marked default in the container.
    pub default: bool,
    /// Forced subtitles.
    pub forced: bool,
}

/// Why a command or the playback failed.
///
/// Fields are never named `message` (clashes with `Throwable` in Kotlin).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum EngineError {
    /// The command needs an open item (or the engine is closed).
    #[error("invalid state: {details}")]
    InvalidState { details: String },
    /// Out-of-range value or unknown track id.
    #[error("invalid argument: {details}")]
    InvalidArgument { details: String },
    /// The engine cannot do this (rate, format, feature).
    #[error("unsupported: {details}")]
    Unsupported { details: String },
    /// The source cannot be opened or read.
    #[error("source: {details}")]
    Source { details: String },
    /// Demuxing or decoding failed.
    #[error("decoder: {details}")]
    Decoder { details: String },
    /// [`PlayerEngine::close`] was called.
    #[error("the engine is closed")]
    Closed,
    #[error("internal: {details}")]
    Internal { details: String },
}

impl EngineError {
    pub(crate) fn state(details: impl Into<String>) -> Self {
        Self::InvalidState {
            details: details.into(),
        }
    }

    pub(crate) fn argument(details: impl Into<String>) -> Self {
        Self::InvalidArgument {
            details: details.into(),
        }
    }
}

/// Everything an engine reports. The UI and the core build
/// [`EngineState`] from these with [`EngineState::apply`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "event")]
pub enum EngineEvent {
    /// Playlist item `index` starts opening. Resets everything except rate,
    /// volume and repeat; `playing` becomes `false` until `PlayingChanged`.
    Loading { index: u32 },
    /// The item is open: duration and video size are known.
    Loaded {
        duration_ms: u64,
        video_width: Option<u32>,
        video_height: Option<u32>,
    },
    /// Track lists changed (after load or an external subtitle).
    TracksChanged {
        audio: Vec<Track>,
        subtitles: Vec<Track>,
    },
    /// Selected track; `None` means off.
    TrackSelected { kind: TrackKind, id: Option<String> },
    /// Position tick (at least every 500 ms while playing) or a finished seek.
    Position { position_ms: u64, buffered_ms: u64 },
    PlayingChanged { playing: bool },
    BufferingChanged { buffering: bool },
    RateChanged { rate: f64 },
    VolumeChanged { volume: f64 },
    RepeatChanged { repeat: RepeatMode },
    /// The last item ended (repeat off). Playback is paused at the end.
    Ended,
    /// Playback failed; the engine stays usable for a new `open`.
    Error { error: EngineError },
    /// [`PlayerEngine::close`] finished; no events follow.
    Closed,
}

/// Snapshot of an engine; mirrors `EngineState` of the Dart package
/// `pa_player`.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct EngineState {
    pub position_ms: u64,
    pub duration_ms: u64,
    /// Absolute position up to which the item is buffered.
    pub buffered_ms: u64,
    pub playing: bool,
    pub buffering: bool,
    /// The last item ended.
    pub completed: bool,
    pub rate: f64,
    /// Percent, [`UNITY_VOLUME`] = unchanged.
    pub volume: f64,
    pub repeat: RepeatMode,
    pub playlist_index: u32,
    /// `true` after `Loaded` of the current item.
    pub loaded: bool,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
    pub audio_track: Option<String>,
    /// `None` = subtitles off.
    pub subtitle_track: Option<String>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    pub error: Option<EngineError>,
    pub closed: bool,
}

impl Default for EngineState {
    fn default() -> Self {
        Self {
            position_ms: 0,
            duration_ms: 0,
            buffered_ms: 0,
            playing: false,
            buffering: false,
            completed: false,
            rate: 1.0,
            volume: UNITY_VOLUME,
            repeat: RepeatMode::Off,
            playlist_index: 0,
            loaded: false,
            audio_tracks: Vec::new(),
            subtitle_tracks: Vec::new(),
            audio_track: None,
            subtitle_track: None,
            video_width: None,
            video_height: None,
            error: None,
            closed: false,
        }
    }
}

impl EngineState {
    /// Applies one event: the only way an engine state changes.
    pub fn apply(&mut self, event: &EngineEvent) {
        match event {
            EngineEvent::Loading { index } => {
                *self = Self {
                    playlist_index: *index,
                    buffering: true,
                    rate: self.rate,
                    volume: self.volume,
                    repeat: self.repeat,
                    ..Self::default()
                };
            }
            EngineEvent::Loaded {
                duration_ms,
                video_width,
                video_height,
            } => {
                self.loaded = true;
                self.buffering = false;
                self.duration_ms = *duration_ms;
                self.video_width = *video_width;
                self.video_height = *video_height;
            }
            EngineEvent::TracksChanged { audio, subtitles } => {
                self.audio_tracks.clone_from(audio);
                self.subtitle_tracks.clone_from(subtitles);
            }
            EngineEvent::TrackSelected { kind, id } => match kind {
                TrackKind::Audio => self.audio_track.clone_from(id),
                TrackKind::Subtitle => self.subtitle_track.clone_from(id),
            },
            EngineEvent::Position {
                position_ms,
                buffered_ms,
            } => {
                self.position_ms = *position_ms;
                self.buffered_ms = *buffered_ms;
                if *position_ms < self.duration_ms {
                    self.completed = false;
                }
            }
            EngineEvent::PlayingChanged { playing } => self.playing = *playing,
            EngineEvent::BufferingChanged { buffering } => self.buffering = *buffering,
            EngineEvent::RateChanged { rate } => self.rate = *rate,
            EngineEvent::VolumeChanged { volume } => self.volume = *volume,
            EngineEvent::RepeatChanged { repeat } => self.repeat = *repeat,
            EngineEvent::Ended => {
                self.completed = true;
                self.playing = false;
            }
            EngineEvent::Error { error } => {
                self.error = Some(error.clone());
                self.playing = false;
                self.buffering = false;
            }
            EngineEvent::Closed => {
                self.closed = true;
                self.playing = false;
                self.buffering = false;
            }
        }
    }

    /// Folds a sequence of events into a state.
    pub fn from_events<'a>(events: impl IntoIterator<Item = &'a EngineEvent>) -> Self {
        let mut state = Self::default();
        for event in events {
            state.apply(event);
        }
        state
    }
}

/// Technical numbers for the stats overlay. PLAY-010 extends this model;
/// new fields are optional so every engine keeps compiling.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(default)]
pub struct PlaybackStats {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// Hardware decoding is active.
    pub hw_decoding: Option<bool>,
    /// Audio is sent as a bitstream (passthrough).
    pub audio_passthrough: Option<bool>,
    pub dropped_frames: u64,
    pub bitrate_bps: Option<u64>,
    pub buffered_ms: u64,
}

/// Receives engine events.
pub trait EngineListener: Send + Sync {
    fn on_event(&self, event: EngineEvent);
}

/// A playback engine (Media3, libmpv, AVFoundation, the own Apple engine,
/// the mock). The video surface is platform UI and not part of this trait.
pub trait PlayerEngine: Send + Sync {
    /// Which engine this is.
    fn kind(&self) -> EngineKind;

    /// Replaces the listener; `None` removes it.
    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>);

    /// Current state, equal to the fold of all events sent so far.
    fn state(&self) -> EngineState;

    /// Replaces the playlist and opens `items[index]` at
    /// `start_position_ms` (clamped to the duration). Sends `Loading`,
    /// `Loaded`, `TracksChanged`, `TrackSelected` for both kinds, `Position`,
    /// and `PlayingChanged` if `play`; or `Error` if the source fails.
    fn open(&self, request: OpenRequest) -> Result<(), EngineError>;

    fn play(&self) -> Result<(), EngineError>;
    fn pause(&self) -> Result<(), EngineError>;

    /// Seeks inside the current item; the position is clamped to
    /// `0..=duration`. Sends `Position` when done.
    fn seek(&self, position_ms: u64) -> Result<(), EngineError>;

    /// Rates in [`MIN_REQUIRED_RATE`]`..=`[`MAX_REQUIRED_RATE`] must work;
    /// others may be `Unsupported`; zero, negative or non-finite is
    /// `InvalidArgument`. Allowed before `open`.
    fn set_rate(&self, rate: f64) -> Result<(), EngineError>;

    /// Percent, `0..=`[`MAX_VOLUME`]; the engine may clamp the upper end and
    /// reports the applied value. Allowed before `open`.
    fn set_volume(&self, volume: f64) -> Result<(), EngineError>;

    /// Allowed before `open`.
    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError>;

    /// Opens playlist item `index` from its beginning.
    fn jump(&self, index: u32) -> Result<(), EngineError>;

    /// `None` disables audio.
    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError>;

    /// `None` disables subtitles.
    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError>;

    /// Loads a subtitle file (SRT, ASS, VTT) and selects it.
    fn add_external_subtitle(&self, uri: String, title: Option<String>)
    -> Result<(), EngineError>;

    /// Current technical numbers.
    fn stats(&self) -> Result<PlaybackStats, EngineError>;

    /// Stops playback and frees the decoder. Idempotent; every later command
    /// fails with [`EngineError::Closed`].
    fn close(&self) -> Result<(), EngineError>;
}
