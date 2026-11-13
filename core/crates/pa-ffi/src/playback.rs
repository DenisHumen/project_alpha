// SPDX-License-Identifier: MPL-2.0
//! The `PlayerEngine` contract of `pa-playback` over FFI (PLAY-APL-001,
//! ADR-0017).
//!
//! Platform engines are written in Swift (MPVKit, AVPlayer, the own Apple
//! engine) and Kotlin (Media3). They implement the foreign trait
//! [`PlayerEngine`]; the core wraps them with [`ForeignEngine`] into a
//! `pa_playback::PlayerEngine`, so track policy, sessions and the contract
//! suite run on them unchanged.
//!
//! - Types mirror `pa-playback` one to one (same fields, milliseconds as
//!   `u64`); `From` conversions go both ways.
//! - [`EngineStateFold`]: the single reducer (`EngineState::apply`) for
//!   engines in other languages. An engine applies every event it sends, so
//!   its `state()` is the fold of its events by construction.
//! - [`run_player_engine_contract_case`]: runs one case of
//!   `pa_playback::contract::CASES` against a [`PlayerEngineHarness`]
//!   implemented in Swift or Kotlin (XCTest, instrumented tests).
//! - [`mock_engine_harness`]: the Rust mock behind the same traits, for UI
//!   tests and for checking the bindings.

use std::sync::{Arc, Mutex, MutexGuard};

use pa_playback as pb;
use pa_playback::contract::{self, Fixture, Harness};

/// Playback engines.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum EngineKind {
    /// AndroidX Media3 / ExoPlayer.
    Media3,
    /// libmpv; MPVKit on Apple.
    Mpv,
    /// AVPlayer.
    AvFoundation,
    /// Own Apple engine: FFmpeg → VideoToolbox → AVSampleBufferDisplayLayer.
    VideoToolbox,
    /// The mock engine of `pa-playback`.
    Mock,
}

impl From<pb::EngineKind> for EngineKind {
    fn from(k: pb::EngineKind) -> Self {
        match k {
            pb::EngineKind::Media3 => Self::Media3,
            pb::EngineKind::Mpv => Self::Mpv,
            pb::EngineKind::AvFoundation => Self::AvFoundation,
            pb::EngineKind::VideoToolbox => Self::VideoToolbox,
            pb::EngineKind::Mock => Self::Mock,
        }
    }
}

impl From<EngineKind> for pb::EngineKind {
    fn from(k: EngineKind) -> Self {
        match k {
            EngineKind::Media3 => Self::Media3,
            EngineKind::Mpv => Self::Mpv,
            EngineKind::AvFoundation => Self::AvFoundation,
            EngineKind::VideoToolbox => Self::VideoToolbox,
            EngineKind::Mock => Self::Mock,
        }
    }
}

/// One playable item.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct MediaSource {
    /// `file://` URL or plain path, `http(s)://` (including the loopback
    /// `pa-proxy`), or a remux/HLS URL of a media server.
    pub uri: String,
    /// Title for system media controls.
    pub title: Option<String>,
}

impl From<pb::MediaSource> for MediaSource {
    fn from(s: pb::MediaSource) -> Self {
        Self {
            uri: s.uri,
            title: s.title,
        }
    }
}

impl From<MediaSource> for pb::MediaSource {
    fn from(s: MediaSource) -> Self {
        Self {
            uri: s.uri,
            title: s.title,
        }
    }
}

/// What `PlayerEngine.open` plays.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct OpenRequest {
    /// Playlist; never empty.
    pub items: Vec<MediaSource>,
    /// Item to start with.
    pub index: u32,
    /// Resume position in `items[index]`.
    pub start_position_ms: u64,
    /// Start playing once loaded; `false` opens paused.
    pub play: bool,
}

impl From<pb::OpenRequest> for OpenRequest {
    fn from(r: pb::OpenRequest) -> Self {
        Self {
            items: r.items.into_iter().map(Into::into).collect(),
            index: r.index,
            start_position_ms: r.start_position_ms,
            play: r.play,
        }
    }
}

impl From<OpenRequest> for pb::OpenRequest {
    fn from(r: OpenRequest) -> Self {
        Self {
            items: r.items.into_iter().map(Into::into).collect(),
            index: r.index,
            start_position_ms: r.start_position_ms,
            play: r.play,
        }
    }
}

/// Playlist repeat mode.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

impl From<pb::RepeatMode> for RepeatMode {
    fn from(m: pb::RepeatMode) -> Self {
        match m {
            pb::RepeatMode::Off => Self::Off,
            pb::RepeatMode::All => Self::All,
            pb::RepeatMode::One => Self::One,
        }
    }
}

impl From<RepeatMode> for pb::RepeatMode {
    fn from(m: RepeatMode) -> Self {
        match m {
            RepeatMode::Off => Self::Off,
            RepeatMode::All => Self::All,
            RepeatMode::One => Self::One,
        }
    }
}

/// Kind of a selectable track.
#[derive(Debug, Clone, Copy, PartialEq, Eq, uniffi::Enum)]
pub enum TrackKind {
    Audio,
    Subtitle,
}

impl From<pb::TrackKind> for TrackKind {
    fn from(k: pb::TrackKind) -> Self {
        match k {
            pb::TrackKind::Audio => Self::Audio,
            pb::TrackKind::Subtitle => Self::Subtitle,
        }
    }
}

impl From<TrackKind> for pb::TrackKind {
    fn from(k: TrackKind) -> Self {
        match k {
            TrackKind::Audio => Self::Audio,
            TrackKind::Subtitle => Self::Subtitle,
        }
    }
}

/// An audio or subtitle track.
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct Track {
    /// Engine-specific id, stable while the item is open.
    pub id: String,
    pub title: Option<String>,
    /// ISO 639 / BCP 47 code from the container.
    pub language: Option<String>,
    /// Codec in FFmpeg spelling (`ac3`, `truehd`, `subrip`, `ass`).
    pub codec: Option<String>,
    /// Audio channel count.
    pub channels: Option<u32>,
    /// Loaded from a separate file.
    pub external: bool,
    /// Marked default in the container.
    pub default: bool,
    /// Forced subtitles.
    pub forced: bool,
}

impl From<pb::Track> for Track {
    fn from(t: pb::Track) -> Self {
        Self {
            id: t.id,
            title: t.title,
            language: t.language,
            codec: t.codec,
            channels: t.channels,
            external: t.external,
            default: t.default,
            forced: t.forced,
        }
    }
}

impl From<Track> for pb::Track {
    fn from(t: Track) -> Self {
        Self {
            id: t.id,
            title: t.title,
            language: t.language,
            codec: t.codec,
            channels: t.channels,
            external: t.external,
            default: t.default,
            forced: t.forced,
        }
    }
}

fn tracks_to_ffi(tracks: Vec<pb::Track>) -> Vec<Track> {
    tracks.into_iter().map(Into::into).collect()
}

fn tracks_to_core(tracks: Vec<Track>) -> Vec<pb::Track> {
    tracks.into_iter().map(Into::into).collect()
}

/// Why a command or the playback failed.
///
/// Fields are never named `message` (clashes with `Throwable` in Kotlin).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum EngineError {
    /// The command needs an open item.
    #[error("invalid state: {details}")]
    InvalidState { details: String },
    /// Out-of-range value or unknown track id.
    #[error("invalid argument: {details}")]
    InvalidArgument { details: String },
    /// The engine cannot do this.
    #[error("unsupported: {details}")]
    Unsupported { details: String },
    /// The source cannot be opened or read.
    #[error("source: {details}")]
    Source { details: String },
    /// Demuxing or decoding failed.
    #[error("decoder: {details}")]
    Decoder { details: String },
    /// `close` was called.
    #[error("the engine is closed")]
    Closed,
    #[error("internal: {details}")]
    Internal { details: String },
}

impl From<pb::EngineError> for EngineError {
    fn from(e: pb::EngineError) -> Self {
        match e {
            pb::EngineError::InvalidState { details } => Self::InvalidState { details },
            pb::EngineError::InvalidArgument { details } => Self::InvalidArgument { details },
            pb::EngineError::Unsupported { details } => Self::Unsupported { details },
            pb::EngineError::Source { details } => Self::Source { details },
            pb::EngineError::Decoder { details } => Self::Decoder { details },
            pb::EngineError::Closed => Self::Closed,
            pb::EngineError::Internal { details } => Self::Internal { details },
        }
    }
}

impl From<EngineError> for pb::EngineError {
    fn from(e: EngineError) -> Self {
        match e {
            EngineError::InvalidState { details } => Self::InvalidState { details },
            EngineError::InvalidArgument { details } => Self::InvalidArgument { details },
            EngineError::Unsupported { details } => Self::Unsupported { details },
            EngineError::Source { details } => Self::Source { details },
            EngineError::Decoder { details } => Self::Decoder { details },
            EngineError::Closed => Self::Closed,
            EngineError::Internal { details } => Self::Internal { details },
        }
    }
}

/// A Swift or Kotlin engine threw something that is not an `EngineError`.
impl From<uniffi::UnexpectedUniFFICallbackError> for EngineError {
    fn from(e: uniffi::UnexpectedUniFFICallbackError) -> Self {
        Self::Internal { details: e.reason }
    }
}

/// Everything an engine reports; see `pa_playback::EngineEvent`.
#[derive(Debug, Clone, PartialEq, uniffi::Enum)]
pub enum EngineEvent {
    /// Playlist item `index` starts opening; resets all but rate, volume
    /// and repeat.
    Loading { index: u32 },
    /// The item is open.
    Loaded {
        duration_ms: u64,
        video_width: Option<u32>,
        video_height: Option<u32>,
    },
    TracksChanged {
        audio: Vec<Track>,
        subtitles: Vec<Track>,
    },
    /// `id: None` means off.
    TrackSelected { kind: TrackKind, id: Option<String> },
    /// At least every 500 ms while playing, and after a seek.
    Position { position_ms: u64, buffered_ms: u64 },
    PlayingChanged { playing: bool },
    BufferingChanged { buffering: bool },
    RateChanged { rate: f64 },
    VolumeChanged { volume: f64 },
    RepeatChanged { repeat: RepeatMode },
    /// The last item ended (repeat off).
    Ended,
    /// Playback failed; the engine stays usable for a new `open`.
    Error { error: EngineError },
    /// `close` finished; no events follow.
    Closed,
}

impl From<pb::EngineEvent> for EngineEvent {
    fn from(e: pb::EngineEvent) -> Self {
        use pb::EngineEvent as E;
        match e {
            E::Loading { index } => Self::Loading { index },
            E::Loaded {
                duration_ms,
                video_width,
                video_height,
            } => Self::Loaded {
                duration_ms,
                video_width,
                video_height,
            },
            E::TracksChanged { audio, subtitles } => Self::TracksChanged {
                audio: tracks_to_ffi(audio),
                subtitles: tracks_to_ffi(subtitles),
            },
            E::TrackSelected { kind, id } => Self::TrackSelected {
                kind: kind.into(),
                id,
            },
            E::Position {
                position_ms,
                buffered_ms,
            } => Self::Position {
                position_ms,
                buffered_ms,
            },
            E::PlayingChanged { playing } => Self::PlayingChanged { playing },
            E::BufferingChanged { buffering } => Self::BufferingChanged { buffering },
            E::RateChanged { rate } => Self::RateChanged { rate },
            E::VolumeChanged { volume } => Self::VolumeChanged { volume },
            E::RepeatChanged { repeat } => Self::RepeatChanged {
                repeat: repeat.into(),
            },
            E::Ended => Self::Ended,
            E::Error { error } => Self::Error {
                error: error.into(),
            },
            E::Closed => Self::Closed,
        }
    }
}

impl From<EngineEvent> for pb::EngineEvent {
    fn from(e: EngineEvent) -> Self {
        use EngineEvent as E;
        match e {
            E::Loading { index } => Self::Loading { index },
            E::Loaded {
                duration_ms,
                video_width,
                video_height,
            } => Self::Loaded {
                duration_ms,
                video_width,
                video_height,
            },
            E::TracksChanged { audio, subtitles } => Self::TracksChanged {
                audio: tracks_to_core(audio),
                subtitles: tracks_to_core(subtitles),
            },
            E::TrackSelected { kind, id } => Self::TrackSelected {
                kind: kind.into(),
                id,
            },
            E::Position {
                position_ms,
                buffered_ms,
            } => Self::Position {
                position_ms,
                buffered_ms,
            },
            E::PlayingChanged { playing } => Self::PlayingChanged { playing },
            E::BufferingChanged { buffering } => Self::BufferingChanged { buffering },
            E::RateChanged { rate } => Self::RateChanged { rate },
            E::VolumeChanged { volume } => Self::VolumeChanged { volume },
            E::RepeatChanged { repeat } => Self::RepeatChanged {
                repeat: repeat.into(),
            },
            E::Ended => Self::Ended,
            E::Error { error } => Self::Error {
                error: error.into(),
            },
            E::Closed => Self::Closed,
        }
    }
}

/// Snapshot of an engine: the fold of its events
/// (`pa_playback::EngineState`).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
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
    /// Percent, 100 = unchanged.
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

impl From<pb::EngineState> for EngineState {
    fn from(s: pb::EngineState) -> Self {
        Self {
            position_ms: s.position_ms,
            duration_ms: s.duration_ms,
            buffered_ms: s.buffered_ms,
            playing: s.playing,
            buffering: s.buffering,
            completed: s.completed,
            rate: s.rate,
            volume: s.volume,
            repeat: s.repeat.into(),
            playlist_index: s.playlist_index,
            loaded: s.loaded,
            audio_tracks: tracks_to_ffi(s.audio_tracks),
            subtitle_tracks: tracks_to_ffi(s.subtitle_tracks),
            audio_track: s.audio_track,
            subtitle_track: s.subtitle_track,
            video_width: s.video_width,
            video_height: s.video_height,
            error: s.error.map(Into::into),
            closed: s.closed,
        }
    }
}

impl From<EngineState> for pb::EngineState {
    fn from(s: EngineState) -> Self {
        Self {
            position_ms: s.position_ms,
            duration_ms: s.duration_ms,
            buffered_ms: s.buffered_ms,
            playing: s.playing,
            buffering: s.buffering,
            completed: s.completed,
            rate: s.rate,
            volume: s.volume,
            repeat: s.repeat.into(),
            playlist_index: s.playlist_index,
            loaded: s.loaded,
            audio_tracks: tracks_to_core(s.audio_tracks),
            subtitle_tracks: tracks_to_core(s.subtitle_tracks),
            audio_track: s.audio_track,
            subtitle_track: s.subtitle_track,
            video_width: s.video_width,
            video_height: s.video_height,
            error: s.error.map(Into::into),
            closed: s.closed,
        }
    }
}

/// Technical numbers for the stats overlay (PLAY-010 extends the model).
#[derive(Debug, Clone, PartialEq, uniffi::Record)]
pub struct PlaybackStats {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    /// Hardware decoding is active.
    pub hw_decoding: Option<bool>,
    /// Audio is sent as a bitstream.
    pub audio_passthrough: Option<bool>,
    pub dropped_frames: u64,
    pub bitrate_bps: Option<u64>,
    pub buffered_ms: u64,
}

impl From<pb::PlaybackStats> for PlaybackStats {
    fn from(s: pb::PlaybackStats) -> Self {
        Self {
            video_codec: s.video_codec,
            audio_codec: s.audio_codec,
            hw_decoding: s.hw_decoding,
            audio_passthrough: s.audio_passthrough,
            dropped_frames: s.dropped_frames,
            bitrate_bps: s.bitrate_bps,
            buffered_ms: s.buffered_ms,
        }
    }
}

impl From<PlaybackStats> for pb::PlaybackStats {
    fn from(s: PlaybackStats) -> Self {
        Self {
            video_codec: s.video_codec,
            audio_codec: s.audio_codec,
            hw_decoding: s.hw_decoding,
            audio_passthrough: s.audio_passthrough,
            dropped_frames: s.dropped_frames,
            bitrate_bps: s.bitrate_bps,
            buffered_ms: s.buffered_ms,
        }
    }
}

/// Receives engine events. Implemented by the core (and by UI code that
/// listens to a Swift or Kotlin engine directly).
#[uniffi::export(with_foreign)]
pub trait EngineListener: Send + Sync {
    fn on_event(&self, event: EngineEvent);
}

/// A playback engine; see `pa_playback::PlayerEngine` for the rules:
/// commands do not block, results come as events, `state()` is the fold of
/// the events (use [`EngineStateFold`]), the listener is called without
/// engine locks held and may call back into the engine.
#[uniffi::export(with_foreign)]
pub trait PlayerEngine: Send + Sync {
    fn kind(&self) -> EngineKind;
    /// Replaces the listener; `None` removes it.
    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>);
    fn state(&self) -> EngineState;
    /// `open` of the core contract. Named `open_media` because UniFFI
    /// escapes the Swift keyword `open` into the C header of foreign traits.
    fn open_media(&self, request: OpenRequest) -> Result<(), EngineError>;
    fn play(&self) -> Result<(), EngineError>;
    fn pause(&self) -> Result<(), EngineError>;
    fn seek(&self, position_ms: u64) -> Result<(), EngineError>;
    fn set_rate(&self, rate: f64) -> Result<(), EngineError>;
    fn set_volume(&self, volume: f64) -> Result<(), EngineError>;
    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError>;
    fn jump(&self, index: u32) -> Result<(), EngineError>;
    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError>;
    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError>;
    fn add_external_subtitle(&self, uri: String, title: Option<String>) -> Result<(), EngineError>;
    fn stats(&self) -> Result<PlaybackStats, EngineError>;
    fn close(&self) -> Result<(), EngineError>;
}

/// The state reducer of the core for engines in Swift and Kotlin.
///
/// The engine calls [`EngineStateFold::apply`] with every event right
/// before sending it and answers `state()` from [`EngineStateFold::state`].
#[derive(Debug, Default, uniffi::Object)]
pub struct EngineStateFold {
    state: Mutex<pb::EngineState>,
}

impl EngineStateFold {
    fn lock(&self) -> MutexGuard<'_, pb::EngineState> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[uniffi::export]
impl EngineStateFold {
    /// Starts from the default state.
    #[uniffi::constructor]
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    /// Applies `event` and returns the new state.
    pub fn apply(&self, event: EngineEvent) -> EngineState {
        let mut state = self.lock();
        state.apply(&event.into());
        state.clone().into()
    }

    /// Current state.
    pub fn state(&self) -> EngineState {
        self.lock().clone().into()
    }
}

/// Core-side listener handed to a foreign engine.
struct ListenerBridge(Arc<dyn pb::EngineListener>);

impl EngineListener for ListenerBridge {
    fn on_event(&self, event: EngineEvent) {
        self.0.on_event(event.into());
    }
}

/// Foreign listener handed to a core engine.
struct CoreListenerBridge(Arc<dyn EngineListener>);

impl pb::EngineListener for CoreListenerBridge {
    fn on_event(&self, event: pb::EngineEvent) {
        self.0.on_event(event.into());
    }
}

/// A Swift or Kotlin engine as a `pa_playback::PlayerEngine`.
pub struct ForeignEngine(pub Arc<dyn PlayerEngine>);

impl pb::PlayerEngine for ForeignEngine {
    fn kind(&self) -> pb::EngineKind {
        self.0.kind().into()
    }

    fn set_listener(&self, listener: Option<Arc<dyn pb::EngineListener>>) {
        self.0.set_listener(
            listener.map(|l| Arc::new(ListenerBridge(l)) as Arc<dyn EngineListener>),
        );
    }

    fn state(&self) -> pb::EngineState {
        self.0.state().into()
    }

    fn open(&self, request: pb::OpenRequest) -> Result<(), pb::EngineError> {
        Ok(self.0.open_media(request.into())?)
    }

    fn play(&self) -> Result<(), pb::EngineError> {
        Ok(self.0.play()?)
    }

    fn pause(&self) -> Result<(), pb::EngineError> {
        Ok(self.0.pause()?)
    }

    fn seek(&self, position_ms: u64) -> Result<(), pb::EngineError> {
        Ok(self.0.seek(position_ms)?)
    }

    fn set_rate(&self, rate: f64) -> Result<(), pb::EngineError> {
        Ok(self.0.set_rate(rate)?)
    }

    fn set_volume(&self, volume: f64) -> Result<(), pb::EngineError> {
        Ok(self.0.set_volume(volume)?)
    }

    fn set_repeat(&self, repeat: pb::RepeatMode) -> Result<(), pb::EngineError> {
        Ok(self.0.set_repeat(repeat.into())?)
    }

    fn jump(&self, index: u32) -> Result<(), pb::EngineError> {
        Ok(self.0.jump(index)?)
    }

    fn set_audio_track(&self, id: Option<String>) -> Result<(), pb::EngineError> {
        Ok(self.0.set_audio_track(id)?)
    }

    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), pb::EngineError> {
        Ok(self.0.set_subtitle_track(id)?)
    }

    fn add_external_subtitle(
        &self,
        uri: String,
        title: Option<String>,
    ) -> Result<(), pb::EngineError> {
        Ok(self.0.add_external_subtitle(uri, title)?)
    }

    fn stats(&self) -> Result<pb::PlaybackStats, pb::EngineError> {
        Ok(self.0.stats()?.into())
    }

    fn close(&self) -> Result<(), pb::EngineError> {
        Ok(self.0.close()?)
    }
}

/// A core engine (the mock, later Rust engines) behind the FFI trait.
pub struct CoreEngine(pub Arc<dyn pb::PlayerEngine>);

impl PlayerEngine for CoreEngine {
    fn kind(&self) -> EngineKind {
        self.0.kind().into()
    }

    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>) {
        self.0.set_listener(
            listener.map(|l| Arc::new(CoreListenerBridge(l)) as Arc<dyn pb::EngineListener>),
        );
    }

    fn state(&self) -> EngineState {
        self.0.state().into()
    }

    fn open_media(&self, request: OpenRequest) -> Result<(), EngineError> {
        Ok(self.0.open(request.into())?)
    }

    fn play(&self) -> Result<(), EngineError> {
        Ok(self.0.play()?)
    }

    fn pause(&self) -> Result<(), EngineError> {
        Ok(self.0.pause()?)
    }

    fn seek(&self, position_ms: u64) -> Result<(), EngineError> {
        Ok(self.0.seek(position_ms)?)
    }

    fn set_rate(&self, rate: f64) -> Result<(), EngineError> {
        Ok(self.0.set_rate(rate)?)
    }

    fn set_volume(&self, volume: f64) -> Result<(), EngineError> {
        Ok(self.0.set_volume(volume)?)
    }

    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError> {
        Ok(self.0.set_repeat(repeat.into())?)
    }

    fn jump(&self, index: u32) -> Result<(), EngineError> {
        Ok(self.0.jump(index)?)
    }

    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError> {
        Ok(self.0.set_audio_track(id)?)
    }

    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError> {
        Ok(self.0.set_subtitle_track(id)?)
    }

    fn add_external_subtitle(&self, uri: String, title: Option<String>) -> Result<(), EngineError> {
        Ok(self.0.add_external_subtitle(uri, title)?)
    }

    fn stats(&self) -> Result<PlaybackStats, EngineError> {
        Ok(self.0.stats()?.into())
    }

    fn close(&self) -> Result<(), EngineError> {
        Ok(self.0.close()?)
    }
}

// ---------------------------------------------------------------------------
// Contract suite for engines in Swift and Kotlin.
// ---------------------------------------------------------------------------

/// Media for the contract suite (`pa_playback::contract::Fixture`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct EngineFixture {
    /// Exactly two video items, each at least 10 s long, with at least two
    /// audio tracks and one subtitle track that is not on by default.
    pub items: Vec<MediaSource>,
    /// Durations of `items` as the engine reports them.
    pub durations_ms: Vec<u64>,
    /// A source that fails to open.
    pub broken: MediaSource,
    /// A subtitle file (SRT) for `add_external_subtitle`.
    pub external_subtitle: String,
}

/// Access to one engine under test, implemented next to the engine
/// (XCTest, Android instrumented test). One harness per case.
#[uniffi::export(with_foreign)]
pub trait PlayerEngineHarness: Send + Sync {
    /// The engine under test; the same object on every call.
    fn engine(&self) -> Arc<dyn PlayerEngine>;
    fn fixture(&self) -> EngineFixture;
    /// Lets `ms` of playback time pass (`0`: none) and returns once the
    /// engine has sent the resulting events: pending opens and seeks have
    /// finished and, while playing, a fresh `Position` was sent.
    fn advance(&self, ms: u64);
}

/// Result of a contract case that did not pass.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, uniffi::Error)]
pub enum ContractError {
    /// No case with this name in [`player_engine_contract_cases`].
    #[error("unknown contract case {name}")]
    UnknownCase { name: String },
    /// The fixture does not have exactly two items and two durations.
    #[error("invalid fixture: {details}")]
    InvalidFixture { details: String },
    /// The engine broke the contract.
    #[error("contract case {case} failed: {details}")]
    Failed { case: String, details: String },
}

struct ForeignHarness {
    harness: Arc<dyn PlayerEngineHarness>,
    engine: ForeignEngine,
    fixture: Fixture,
}

impl Harness for ForeignHarness {
    fn engine(&self) -> &dyn pb::PlayerEngine {
        &self.engine
    }

    fn fixture(&self) -> Fixture {
        self.fixture.clone()
    }

    fn advance(&self, ms: u64) {
        self.harness.advance(ms);
    }
}

fn core_fixture(f: EngineFixture) -> Result<Fixture, ContractError> {
    let invalid = |details: &str| ContractError::InvalidFixture {
        details: details.into(),
    };
    let [a, b]: [MediaSource; 2] = f
        .items
        .try_into()
        .map_err(|_| invalid("exactly two items are needed"))?;
    let durations: [u64; 2] = f
        .durations_ms
        .try_into()
        .map_err(|_| invalid("exactly two durations are needed"))?;
    Ok(Fixture {
        items: [a.into(), b.into()],
        durations_ms: durations,
        broken: f.broken.into(),
        external_subtitle: f.external_subtitle,
    })
}

/// Names of every case of the `PlayerEngine` contract suite, in order.
#[uniffi::export]
pub fn player_engine_contract_cases() -> Vec<String> {
    contract::CASES
        .iter()
        .map(|(name, _)| (*name).to_owned())
        .collect()
}

/// Runs contract case `name` against the engine of `harness` on the
/// calling thread. Blocks while the case plays (seconds for a real engine).
#[uniffi::export]
pub fn run_player_engine_contract_case(
    name: String,
    harness: Arc<dyn PlayerEngineHarness>,
) -> Result<(), ContractError> {
    let case = contract::CASES
        .iter()
        .find(|(n, _)| *n == name)
        .map(|(_, case)| *case)
        .ok_or_else(|| ContractError::UnknownCase { name: name.clone() })?;
    let fixture = core_fixture(harness.fixture())?;
    let h = ForeignHarness {
        engine: ForeignEngine(harness.engine()),
        harness,
        fixture,
    };
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| case(&h))).map_err(|panic| {
        let details = panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "panic".to_owned());
        ContractError::Failed {
            case: name,
            details,
        }
    })
}

/// The Rust mock engine with its fixture and virtual clock.
struct MockHarness {
    mock: Arc<pb::MockEngine>,
    engine: Arc<dyn PlayerEngine>,
    fixture: EngineFixture,
}

impl PlayerEngineHarness for MockHarness {
    fn engine(&self) -> Arc<dyn PlayerEngine> {
        self.engine.clone()
    }

    fn fixture(&self) -> EngineFixture {
        self.fixture.clone()
    }

    fn advance(&self, ms: u64) {
        self.mock.advance(ms);
    }
}

/// A harness over a fresh `pa_playback::MockEngine` (`mock://` URIs, virtual
/// clock): a stand-in engine for UI tests and a check that the bindings and
/// the contract runner work on a platform.
#[uniffi::export]
pub fn mock_engine_harness() -> Arc<dyn PlayerEngineHarness> {
    let (fixture, registry) = contract::mock_fixture();
    let mock = Arc::new(
        registry
            .into_iter()
            .fold(pb::MockEngine::new(), |e, (uri, media)| {
                e.with_media(uri, media)
            }),
    );
    let engine: Arc<dyn PlayerEngine> = Arc::new(CoreEngine(mock.clone()));
    Arc::new(MockHarness {
        mock,
        engine,
        fixture: EngineFixture {
            items: fixture.items.into_iter().map(Into::into).collect(),
            durations_ms: fixture.durations_ms.to_vec(),
            broken: fixture.broken.into(),
            external_subtitle: fixture.external_subtitle,
        },
    })
}
