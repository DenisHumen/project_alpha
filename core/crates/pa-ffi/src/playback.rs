// SPDX-License-Identifier: MPL-2.0
//! The `PlayerEngine` contract over FFI (`pa-playback`, ADR-0017).
//!
//! - The contract types of `pa-playback` are exported as they are (UniFFI
//!   remote types), so Kotlin and Swift see the same records and enums as
//!   the core.
//! - [`PlayerEngine`] and [`EngineListener`] are foreign traits: the Media3
//!   engine (Kotlin, PLAY-AND-002) and the Apple engines (Swift,
//!   PLAY-APL-001) implement them; [`ForeignEngine`] turns such an engine
//!   into a `pa_playback::PlayerEngine` for the core.
//! - [`engine_state_apply`] is the one reducer of all engines: a platform
//!   engine folds its events with it instead of keeping its own copy.
//! - [`player_engine_contract_run`] runs a case of the core contract suite
//!   (`pa_playback::contract`) against a platform engine through a
//!   [`PlayerEngineHarness`], so instrumented tests on a device run exactly
//!   the cases the mock passes.

use std::sync::Arc;

use pa_playback::contract::{self, Fixture, Harness};
use pa_playback::{
    EngineError, EngineEvent, EngineKind, EngineState, MediaSource, OpenRequest, PlaybackStats,
    RepeatMode, Track, TrackKind,
};

#[uniffi::remote(Enum)]
pub enum EngineKind {
    Media3,
    Mpv,
    AvFoundation,
    VideoToolbox,
    Mock,
}

#[uniffi::remote(Record)]
pub struct MediaSource {
    pub uri: String,
    pub title: Option<String>,
}

#[uniffi::remote(Record)]
pub struct OpenRequest {
    pub items: Vec<MediaSource>,
    pub index: u32,
    pub start_position_ms: u64,
    pub play: bool,
}

#[uniffi::remote(Enum)]
pub enum RepeatMode {
    Off,
    All,
    One,
}

#[uniffi::remote(Enum)]
pub enum TrackKind {
    Audio,
    Subtitle,
}

#[uniffi::remote(Record)]
pub struct Track {
    pub id: String,
    pub title: Option<String>,
    pub language: Option<String>,
    pub codec: Option<String>,
    pub channels: Option<u32>,
    pub external: bool,
    pub default: bool,
    pub forced: bool,
}

#[uniffi::remote(Error)]
pub enum EngineError {
    InvalidState { details: String },
    InvalidArgument { details: String },
    Unsupported { details: String },
    Source { details: String },
    Decoder { details: String },
    Closed,
    Internal { details: String },
}

#[uniffi::remote(Enum)]
pub enum EngineEvent {
    Loading {
        index: u32,
    },
    Loaded {
        duration_ms: u64,
        video_width: Option<u32>,
        video_height: Option<u32>,
    },
    TracksChanged {
        audio: Vec<Track>,
        subtitles: Vec<Track>,
    },
    TrackSelected {
        kind: TrackKind,
        id: Option<String>,
    },
    Position {
        position_ms: u64,
        buffered_ms: u64,
    },
    PlayingChanged {
        playing: bool,
    },
    BufferingChanged {
        buffering: bool,
    },
    RateChanged {
        rate: f64,
    },
    VolumeChanged {
        volume: f64,
    },
    RepeatChanged {
        repeat: RepeatMode,
    },
    Ended,
    Error {
        error: EngineError,
    },
    Closed,
}

#[uniffi::remote(Record)]
pub struct EngineState {
    pub position_ms: u64,
    pub duration_ms: u64,
    pub buffered_ms: u64,
    pub playing: bool,
    pub buffering: bool,
    pub completed: bool,
    pub rate: f64,
    pub volume: f64,
    pub repeat: RepeatMode,
    pub playlist_index: u32,
    pub loaded: bool,
    pub audio_tracks: Vec<Track>,
    pub subtitle_tracks: Vec<Track>,
    pub audio_track: Option<String>,
    pub subtitle_track: Option<String>,
    pub video_width: Option<u32>,
    pub video_height: Option<u32>,
    pub error: Option<EngineError>,
    pub closed: bool,
}

#[uniffi::remote(Record)]
pub struct PlaybackStats {
    pub video_codec: Option<String>,
    pub audio_codec: Option<String>,
    pub hw_decoding: Option<bool>,
    pub audio_passthrough: Option<bool>,
    pub dropped_frames: u64,
    pub bitrate_bps: Option<u64>,
    pub buffered_ms: u64,
}

/// State of an engine before its first event.
#[uniffi::export]
pub fn engine_state_default() -> EngineState {
    EngineState::default()
}

/// Applies one event to a state: the reducer every engine uses
/// (`EngineState::apply`).
#[uniffi::export]
pub fn engine_state_apply(state: EngineState, event: EngineEvent) -> EngineState {
    let mut state = state;
    state.apply(&event);
    state
}

/// The event in the wire format of the contract (tagged JSON, as in
/// `pa-playback`), e.g. for a Flutter OSD behind a platform channel.
#[uniffi::export]
pub fn engine_event_to_json(event: EngineEvent) -> String {
    serde_json::to_string(&event).unwrap_or_default()
}

/// The state as JSON (field names as in `pa-playback`).
#[uniffi::export]
pub fn engine_state_to_json(state: EngineState) -> String {
    serde_json::to_string(&state).unwrap_or_default()
}

/// Receives engine events (foreign trait; Rust listeners work too).
///
/// See `pa_playback::EngineListener`: called on an engine thread without
/// engine locks held; return quickly.
#[uniffi::export(with_foreign)]
pub trait EngineListener: Send + Sync {
    fn on_event(&self, event: EngineEvent);
}

/// A playback engine implemented on the platform (Media3, AVFoundation…).
///
/// Same rules as `pa_playback::PlayerEngine`: commands do not block, results
/// arrive as events, `state()` is the fold of the events
/// ([`engine_state_apply`]). The video surface is platform UI and not part
/// of this trait.
#[uniffi::export(with_foreign)]
pub trait PlayerEngine: Send + Sync {
    fn kind(&self) -> EngineKind;
    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>);
    fn state(&self) -> EngineState;
    /// `PlayerEngine::open`. Renamed: UniFFI escapes the Swift keyword
    /// `open` with backticks inside the generated C header of foreign
    /// traits, and that header does not compile (PLAY-APL-001).
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
    /// `PlayerEngine::close`. Renamed: generated Kotlin objects are
    /// `AutoCloseable`, whose `close()` frees the handle.
    fn close_engine(&self) -> Result<(), EngineError>;
}

/// Passes events of a platform engine to a core listener.
struct CoreListener(Arc<dyn pa_playback::EngineListener>);

impl EngineListener for CoreListener {
    fn on_event(&self, event: EngineEvent) {
        self.0.on_event(event);
    }
}

/// A platform engine seen by the core as a `pa_playback::PlayerEngine`.
pub struct ForeignEngine(pub Arc<dyn PlayerEngine>);

impl pa_playback::PlayerEngine for ForeignEngine {
    fn kind(&self) -> EngineKind {
        self.0.kind()
    }
    fn set_listener(&self, listener: Option<Arc<dyn pa_playback::EngineListener>>) {
        self.0
            .set_listener(listener.map(|l| Arc::new(CoreListener(l)) as Arc<dyn EngineListener>));
    }
    fn state(&self) -> EngineState {
        self.0.state()
    }
    fn open(&self, request: OpenRequest) -> Result<(), EngineError> {
        self.0.open_media(request)
    }
    fn play(&self) -> Result<(), EngineError> {
        self.0.play()
    }
    fn pause(&self) -> Result<(), EngineError> {
        self.0.pause()
    }
    fn seek(&self, position_ms: u64) -> Result<(), EngineError> {
        self.0.seek(position_ms)
    }
    fn set_rate(&self, rate: f64) -> Result<(), EngineError> {
        self.0.set_rate(rate)
    }
    fn set_volume(&self, volume: f64) -> Result<(), EngineError> {
        self.0.set_volume(volume)
    }
    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError> {
        self.0.set_repeat(repeat)
    }
    fn jump(&self, index: u32) -> Result<(), EngineError> {
        self.0.jump(index)
    }
    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.0.set_audio_track(id)
    }
    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.0.set_subtitle_track(id)
    }
    fn add_external_subtitle(&self, uri: String, title: Option<String>) -> Result<(), EngineError> {
        self.0.add_external_subtitle(uri, title)
    }
    fn stats(&self) -> Result<PlaybackStats, EngineError> {
        self.0.stats()
    }
    fn close(&self) -> Result<(), EngineError> {
        self.0.close_engine()
    }
}

/// Media of the contract suite on the device (see `pa_playback::contract::Fixture`).
#[derive(Debug, Clone, PartialEq, Eq, uniffi::Record)]
pub struct ContractFixture {
    /// Exactly two items, each at least 10 s long, with at least two audio
    /// tracks and a subtitle track that is not on by default.
    pub items: Vec<MediaSource>,
    /// Durations of `items` as the engine reports them.
    pub durations_ms: Vec<u64>,
    /// A source that fails to open.
    pub broken: MediaSource,
    /// An SRT file for `add_external_subtitle`.
    pub external_subtitle: String,
}

/// A platform engine under test (foreign trait). One harness per case.
#[uniffi::export(with_foreign)]
pub trait PlayerEngineHarness: Send + Sync {
    /// The engine under test; the same object on every call.
    fn engine(&self) -> Arc<dyn PlayerEngine>;
    fn fixture(&self) -> ContractFixture;
    /// Lets `ms` of playback time pass, then waits until the engine has
    /// finished the commands sent so far and reported their events.
    fn advance(&self, ms: u64);
}

struct HarnessBridge {
    harness: Arc<dyn PlayerEngineHarness>,
    engine: ForeignEngine,
    fixture: Fixture,
}

impl Harness for HarnessBridge {
    fn engine(&self) -> &dyn pa_playback::PlayerEngine {
        &self.engine
    }
    fn fixture(&self) -> Fixture {
        self.fixture.clone()
    }
    fn advance(&self, ms: u64) {
        self.harness.advance(ms);
    }
}

/// Names of the contract cases, in order.
#[uniffi::export]
pub fn player_engine_contract_cases() -> Vec<String> {
    contract::CASES
        .iter()
        .map(|(n, _)| (*n).to_owned())
        .collect()
}

/// Runs one contract case against the engine of `harness`.
///
/// Returns `None` when the case passes, otherwise why it failed. The case
/// argument is called `name`: a parameter named `case` (a Swift keyword)
/// breaks the generated C header for Swift.
#[uniffi::export]
pub fn player_engine_contract_run(
    name: String,
    harness: Arc<dyn PlayerEngineHarness>,
) -> Option<String> {
    let Some((_, run)) = contract::CASES.iter().find(|(n, _)| *n == name) else {
        return Some(format!("unknown contract case `{name}`"));
    };
    let fx = harness.fixture();
    let (Ok(items), Ok(durations_ms)) = (
        <[MediaSource; 2]>::try_from(fx.items),
        <[u64; 2]>::try_from(fx.durations_ms),
    ) else {
        return Some("the fixture must have exactly two items and two durations".into());
    };
    let bridge = HarnessBridge {
        engine: ForeignEngine(harness.engine()),
        harness,
        fixture: Fixture {
            items,
            durations_ms,
            broken: fx.broken,
            external_subtitle: fx.external_subtitle,
        },
    };
    let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| run(&bridge)));
    result.err().map(|panic| {
        panic
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| panic.downcast_ref::<&str>().map(|s| (*s).to_owned()))
            .unwrap_or_else(|| "panic".into())
    })
}

/// A core engine (the mock, later Rust engines) behind the FFI trait.
pub struct CoreEngine(pub Arc<dyn pa_playback::PlayerEngine>);

/// Passes events of a core engine to a platform listener.
struct PlatformListener(Arc<dyn EngineListener>);

impl pa_playback::EngineListener for PlatformListener {
    fn on_event(&self, event: EngineEvent) {
        self.0.on_event(event);
    }
}

impl PlayerEngine for CoreEngine {
    fn kind(&self) -> EngineKind {
        self.0.kind()
    }
    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>) {
        self.0.set_listener(
            listener.map(|l| Arc::new(PlatformListener(l)) as Arc<dyn pa_playback::EngineListener>),
        );
    }
    fn state(&self) -> EngineState {
        self.0.state()
    }
    fn open_media(&self, request: OpenRequest) -> Result<(), EngineError> {
        self.0.open(request)
    }
    fn play(&self) -> Result<(), EngineError> {
        self.0.play()
    }
    fn pause(&self) -> Result<(), EngineError> {
        self.0.pause()
    }
    fn seek(&self, position_ms: u64) -> Result<(), EngineError> {
        self.0.seek(position_ms)
    }
    fn set_rate(&self, rate: f64) -> Result<(), EngineError> {
        self.0.set_rate(rate)
    }
    fn set_volume(&self, volume: f64) -> Result<(), EngineError> {
        self.0.set_volume(volume)
    }
    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError> {
        self.0.set_repeat(repeat)
    }
    fn jump(&self, index: u32) -> Result<(), EngineError> {
        self.0.jump(index)
    }
    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.0.set_audio_track(id)
    }
    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.0.set_subtitle_track(id)
    }
    fn add_external_subtitle(&self, uri: String, title: Option<String>) -> Result<(), EngineError> {
        self.0.add_external_subtitle(uri, title)
    }
    fn stats(&self) -> Result<PlaybackStats, EngineError> {
        self.0.stats()
    }
    fn close_engine(&self) -> Result<(), EngineError> {
        self.0.close()
    }
}

/// The Rust mock with its fixture and virtual clock.
struct MockHarness {
    mock: Arc<pa_playback::MockEngine>,
    engine: Arc<dyn PlayerEngine>,
    fixture: ContractFixture,
}

impl PlayerEngineHarness for MockHarness {
    fn engine(&self) -> Arc<dyn PlayerEngine> {
        self.engine.clone()
    }
    fn fixture(&self) -> ContractFixture {
        self.fixture.clone()
    }
    fn advance(&self, ms: u64) {
        self.mock.advance(ms);
    }
}

/// A harness over a fresh `pa_playback::MockEngine` (`mock://` URIs, virtual
/// clock): a stand-in engine for UI tests and a check on a platform that the
/// bindings and the contract runner work (PLAY-APL-001).
#[uniffi::export]
pub fn mock_engine_harness() -> Arc<dyn PlayerEngineHarness> {
    let (fixture, registry) = contract::mock_fixture();
    let mock = Arc::new(
        registry
            .into_iter()
            .fold(pa_playback::MockEngine::new(), |e, (uri, media)| {
                e.with_media(uri, media)
            }),
    );
    Arc::new(MockHarness {
        engine: Arc::new(CoreEngine(mock.clone())),
        mock,
        fixture: ContractFixture {
            items: fixture.items.to_vec(),
            durations_ms: fixture.durations_ms.to_vec(),
            broken: fixture.broken,
            external_subtitle: fixture.external_subtitle,
        },
    })
}
