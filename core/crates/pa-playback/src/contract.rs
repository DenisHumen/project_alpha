// SPDX-License-Identifier: MPL-2.0
//! Contract test suite that every [`PlayerEngine`] must pass.
//!
//! An engine is tested through a [`Harness`]: it hands out the engine, a
//! [`Fixture`] (two playable items with known properties and one broken
//! source) and a way to let time pass. For [`MockEngine`] that is the virtual
//! clock ([`mock_harness`]); a real engine sleeps and waits for its events,
//! and its fixture files come from the test media of QA-002. Kotlin and Swift
//! engines run the same cases once they are exported through `pa-ffi` as a
//! foreign trait (PLAY-AND-002, PLAY-APL-001).
//!
//! In a crate with an engine, generate one `#[test]` per case:
//!
//! ```ignore
//! pa_playback::player_engine_contract_tests!(|| Box::new(MyHarness::new()));
//! ```
//!
//! or call [`run_all`] from a single test.

use std::sync::{Arc, Mutex};

use crate::engine::{
    EngineError, EngineEvent, EngineListener, EngineState, MediaSource, OpenRequest, PlayerEngine,
    RepeatMode, Track,
};
use crate::mock::{MockEngine, MockMedia};

/// Allowed difference between an expected and a reported position.
pub const POSITION_TOLERANCE_MS: u64 = 500;

/// Media the suite plays.
#[derive(Debug, Clone)]
pub struct Fixture {
    /// Two video items, each at least 10 s long, with at least two audio
    /// tracks and at least one subtitle track that is not on by default.
    pub items: [MediaSource; 2],
    /// Durations of `items`, as the engine will report them.
    pub durations_ms: [u64; 2],
    /// A source that fails to open.
    pub broken: MediaSource,
    /// A subtitle file (SRT) for `add_external_subtitle`.
    pub external_subtitle: String,
}

/// Access to one engine under test. One harness per case.
pub trait Harness {
    fn engine(&self) -> &dyn PlayerEngine;
    fn fixture(&self) -> Fixture;
    /// Lets `ms` of playback time pass and waits until the engine has sent
    /// the resulting events.
    fn advance(&self, ms: u64);
}

/// Collects every event the engine sends.
#[derive(Default)]
pub struct Recorder(Mutex<Vec<EngineEvent>>);

impl Recorder {
    /// Installs a new recorder as the listener of `engine`.
    pub fn attach(engine: &dyn PlayerEngine) -> Arc<Self> {
        let recorder = Arc::new(Self::default());
        engine.set_listener(Some(recorder.clone()));
        recorder
    }

    pub fn events(&self) -> Vec<EngineEvent> {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

impl EngineListener for Recorder {
    fn on_event(&self, event: EngineEvent) {
        self.0.lock().unwrap_or_else(|e| e.into_inner()).push(event);
    }
}

/// One contract case.
pub type Case = fn(&dyn Harness);

/// Every case, by name.
pub const CASES: &[(&str, Case)] = &[
    ("open_loads_item", open_loads_item),
    ("open_at_start_position", open_at_start_position),
    ("open_paused", open_paused),
    ("open_broken_source", open_broken_source),
    ("commands_need_open_item", commands_need_open_item),
    ("play_pause", play_pause),
    ("seek_clamps", seek_clamps),
    ("rate", rate),
    ("volume", volume),
    ("select_tracks", select_tracks),
    ("external_subtitle", external_subtitle),
    ("end_of_playlist", end_of_playlist),
    ("playlist_advance_and_jump", playlist_advance_and_jump),
    ("repeat_one", repeat_one),
    ("stats", stats),
    ("close", close),
];

/// Runs every case on a fresh harness from `make`.
pub fn run_all(make: &dyn Fn() -> Box<dyn Harness>) {
    for (name, case) in CASES {
        let h = make();
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| case(&*h)));
        if let Err(panic) = result {
            let msg = panic
                .downcast_ref::<String>()
                .map(String::as_str)
                .or_else(|| panic.downcast_ref::<&str>().copied())
                .unwrap_or("panic");
            panic!(
                "PlayerEngine contract case `{name}` failed for {:?}: {msg}",
                h.engine().kind()
            );
        }
    }
}

fn open(h: &dyn Harness, index: u32, start_position_ms: u64, play: bool) -> Arc<Recorder> {
    let rec = Recorder::attach(h.engine());
    let request = OpenRequest {
        items: h.fixture().items.to_vec(),
        index,
        start_position_ms,
        play,
    };
    h.engine().open(request).expect("open is accepted");
    h.advance(0);
    rec
}

/// The core invariant: state() is the fold of the events.
fn assert_state_is_fold(h: &dyn Harness, rec: &Recorder) -> EngineState {
    let state = h.engine().state();
    // The recorder was attached before the first command, so the fold starts
    // from the default state.
    assert_eq!(
        EngineState::from_events(&rec.events()),
        state,
        "state() must equal the fold of all events"
    );
    state
}

fn assert_near(actual: u64, expected: u64, what: &str) {
    assert!(
        actual.abs_diff(expected) <= POSITION_TOLERANCE_MS,
        "{what}: expected ~{expected} ms, got {actual} ms"
    );
}

fn non_selected(tracks: &[Track], selected: &Option<String>) -> Track {
    tracks
        .iter()
        .find(|t| Some(&t.id) != selected.as_ref())
        .cloned()
        .expect("fixture has another track")
}

pub fn open_loads_item(h: &dyn Harness) {
    let rec = open(h, 0, 0, true);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.loaded && !s.buffering && s.error.is_none(), "{s:?}");
    assert_near(s.duration_ms, h.fixture().durations_ms[0], "duration");
    assert!(s.playing, "open with play=true plays");
    assert!(s.audio_tracks.len() >= 2, "fixture audio tracks reported");
    assert!(!s.subtitle_tracks.is_empty(), "fixture subtitles reported");
    assert!(s.audio_track.is_some(), "an audio track is selected");
    assert!(s.video_width.is_some() && s.video_height.is_some());
    let events = rec.events();
    let first = events.first().expect("events were sent");
    assert_eq!(
        first,
        &EngineEvent::Loading { index: 0 },
        "Loading comes first"
    );
    let loaded = events
        .iter()
        .position(|e| matches!(e, EngineEvent::Loaded { .. }));
    let tracks = events
        .iter()
        .position(|e| matches!(e, EngineEvent::TracksChanged { .. }));
    assert!(
        loaded.is_some() && tracks.is_some(),
        "Loaded and TracksChanged sent"
    );
}

pub fn open_at_start_position(h: &dyn Harness) {
    let rec = open(h, 0, 4_000, true);
    let s = assert_state_is_fold(h, &rec);
    assert_near(s.position_ms, 4_000, "start position");
    let beyond = h.fixture().durations_ms[0] + 60_000;
    let rec = open(h, 0, beyond, false);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.position_ms <= s.duration_ms, "start position is clamped");
}

pub fn open_paused(h: &dyn Harness) {
    let rec = open(h, 0, 2_000, false);
    h.advance(1_000);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.loaded && !s.playing, "{s:?}");
    assert_near(s.position_ms, 2_000, "paused position does not move");
}

pub fn open_broken_source(h: &dyn Harness) {
    let rec = Recorder::attach(h.engine());
    let request = OpenRequest::single(h.fixture().broken);
    // Either rejected at once or reported as an Error event.
    if h.engine().open(request).is_ok() {
        h.advance(0);
        let s = assert_state_is_fold(h, &rec);
        assert!(
            matches!(s.error, Some(EngineError::Source { .. })),
            "broken source reports a Source error: {s:?}"
        );
        assert!(!s.playing && !s.loaded);
    }
    // The engine stays usable.
    let rec = open(h, 0, 0, true);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.loaded && s.error.is_none(), "{s:?}");
}

pub fn commands_need_open_item(h: &dyn Harness) {
    let e = h.engine();
    let state_error = |r: Result<(), EngineError>, what: &str| {
        assert!(
            matches!(r, Err(EngineError::InvalidState { .. })),
            "{what} before open: {r:?}"
        );
    };
    state_error(e.play(), "play");
    state_error(e.pause(), "pause");
    state_error(e.seek(1_000), "seek");
    state_error(e.set_audio_track(None), "set_audio_track");
    state_error(e.set_subtitle_track(None), "set_subtitle_track");
    state_error(
        e.add_external_subtitle("x.srt".into(), None),
        "add_external_subtitle",
    );
    // Settings are accepted before open and survive it.
    let rec = Recorder::attach(e);
    e.set_rate(1.25).expect("rate before open");
    e.set_volume(40.0).expect("volume before open");
    e.set_repeat(RepeatMode::All).expect("repeat before open");
    e.open(OpenRequest {
        items: h.fixture().items.to_vec(),
        index: 0,
        start_position_ms: 0,
        play: false,
    })
    .expect("open");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!((s.rate, s.volume, s.repeat), (1.25, 40.0, RepeatMode::All));
}

pub fn play_pause(h: &dyn Harness) {
    let rec = open(h, 0, 0, true);
    h.advance(2_000);
    let s = assert_state_is_fold(h, &rec);
    assert_near(s.position_ms, 2_000, "position after 2 s of playback");
    h.engine().pause().expect("pause");
    h.advance(2_000);
    let s = assert_state_is_fold(h, &rec);
    assert!(!s.playing);
    assert_near(s.position_ms, 2_000, "position while paused");
    h.engine().play().expect("play");
    h.advance(1_000);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.playing);
    assert_near(s.position_ms, 3_000, "position after resume");
}

pub fn seek_clamps(h: &dyn Harness) {
    let rec = open(h, 0, 0, false);
    h.engine().seek(5_000).expect("seek");
    h.advance(0);
    assert_near(assert_state_is_fold(h, &rec).position_ms, 5_000, "seek");
    h.engine().seek(u64::MAX).expect("seek beyond the end");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.position_ms <= s.duration_ms, "seek is clamped: {s:?}");
    h.engine().seek(0).expect("seek to start");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.position_ms <= POSITION_TOLERANCE_MS && !s.completed);
}

pub fn rate(h: &dyn Harness) {
    let rec = open(h, 0, 0, true);
    for bad in [0.0, -1.0, f64::NAN, f64::INFINITY] {
        assert!(
            matches!(
                h.engine().set_rate(bad),
                Err(EngineError::InvalidArgument { .. })
            ),
            "rate {bad} is rejected"
        );
    }
    for good in [crate::MIN_REQUIRED_RATE, crate::MAX_REQUIRED_RATE] {
        h.engine().set_rate(good).expect("required rate");
    }
    h.engine().set_rate(1.5).expect("rate 1.5");
    let before = h.engine().state().position_ms;
    h.advance(2_000);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!(s.rate, 1.5);
    assert_near(s.position_ms, before + 3_000, "position at 1.5×");
}

pub fn volume(h: &dyn Harness) {
    let rec = open(h, 0, 0, false);
    h.engine().set_volume(50.0).expect("volume 50");
    h.advance(0);
    assert_eq!(assert_state_is_fold(h, &rec).volume, 50.0);
    assert!(matches!(
        h.engine().set_volume(-1.0),
        Err(EngineError::InvalidArgument { .. })
    ));
    h.engine().set_volume(250.0).expect("boost is accepted");
    h.advance(0);
    let v = assert_state_is_fold(h, &rec).volume;
    assert!(
        (crate::UNITY_VOLUME..=250.0).contains(&v),
        "boost applied or clamped to unity: {v}"
    );
}

pub fn select_tracks(h: &dyn Harness) {
    let rec = open(h, 0, 0, false);
    let s = h.engine().state();
    let other = non_selected(&s.audio_tracks, &s.audio_track);
    h.engine()
        .set_audio_track(Some(other.id.clone()))
        .expect("audio");
    let sub = s.subtitle_tracks[0].id.clone();
    h.engine()
        .set_subtitle_track(Some(sub.clone()))
        .expect("subtitle on");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!(s.audio_track, Some(other.id));
    assert_eq!(s.subtitle_track, Some(sub));
    h.engine().set_subtitle_track(None).expect("subtitle off");
    h.advance(0);
    assert_eq!(assert_state_is_fold(h, &rec).subtitle_track, None);
    let unknown = h.engine().set_audio_track(Some("no-such-track".into()));
    assert!(
        matches!(unknown, Err(EngineError::InvalidArgument { .. })),
        "{unknown:?}"
    );
}

pub fn external_subtitle(h: &dyn Harness) {
    let rec = open(h, 0, 0, false);
    let before = h.engine().state().subtitle_tracks.len();
    let uri = h.fixture().external_subtitle;
    h.engine()
        .add_external_subtitle(uri, Some("External".into()))
        .expect("external subtitle");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!(s.subtitle_tracks.len(), before + 1);
    let track = s
        .subtitle_tracks
        .iter()
        .find(|t| t.external)
        .expect("external track");
    assert_eq!(s.subtitle_track.as_ref(), Some(&track.id), "it is selected");
}

pub fn end_of_playlist(h: &dyn Harness) {
    let duration = h.fixture().durations_ms[1];
    let rec = open(h, 1, duration.saturating_sub(1_000), true);
    h.advance(2_000);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.completed && !s.playing, "{s:?}");
    assert_eq!(s.playlist_index, 1);
    assert_near(s.position_ms, s.duration_ms, "position at the end");
    assert_eq!(rec.events().last(), Some(&EngineEvent::Ended));
    // Play after the end starts over.
    h.engine().play().expect("play after end");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert!(s.playing && !s.completed);
    assert!(s.position_ms <= POSITION_TOLERANCE_MS);
}

pub fn playlist_advance_and_jump(h: &dyn Harness) {
    let fx = h.fixture();
    let rec = open(h, 0, fx.durations_ms[0].saturating_sub(1_000), true);
    h.advance(2_000);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!(s.playlist_index, 1, "next item after the first ends");
    assert!(s.playing && !s.completed);
    assert_near(s.duration_ms, fx.durations_ms[1], "duration of item 1");
    assert!(
        !rec.events().contains(&EngineEvent::Ended),
        "no Ended mid-playlist"
    );
    h.engine().jump(0).expect("jump");
    h.advance(0);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!(s.playlist_index, 0);
    assert!(s.position_ms <= POSITION_TOLERANCE_MS);
    assert!(matches!(
        h.engine().jump(2),
        Err(EngineError::InvalidArgument { .. })
    ));
}

pub fn repeat_one(h: &dyn Harness) {
    let duration = h.fixture().durations_ms[0];
    let rec = Recorder::attach(h.engine());
    h.engine().set_repeat(RepeatMode::One).expect("repeat one");
    h.engine()
        .open(OpenRequest {
            items: h.fixture().items.to_vec(),
            index: 0,
            start_position_ms: duration.saturating_sub(1_000),
            play: true,
        })
        .expect("open");
    h.advance(2_000);
    let s = assert_state_is_fold(h, &rec);
    assert_eq!(s.playlist_index, 0, "repeat one stays on the item");
    assert!(s.playing && !s.completed);
    assert!(s.position_ms < duration / 2, "restarted: {s:?}");
}

pub fn stats(h: &dyn Harness) {
    assert!(h.engine().stats().is_err(), "stats need an open item");
    let _rec = open(h, 0, 0, true);
    let stats = h.engine().stats().expect("stats");
    assert!(
        stats.video_codec.is_some(),
        "video codec reported: {stats:?}"
    );
    assert!(
        stats.audio_codec.is_some(),
        "audio codec reported: {stats:?}"
    );
}

pub fn close(h: &dyn Harness) {
    let rec = open(h, 0, 0, true);
    h.engine().close().expect("close");
    h.engine().close().expect("close is idempotent");
    let s = assert_state_is_fold(h, &rec);
    assert!(s.closed && !s.playing);
    assert_eq!(rec.events().last(), Some(&EngineEvent::Closed));
    let count = rec.events().len();
    assert_eq!(h.engine().play(), Err(EngineError::Closed));
    assert_eq!(h.engine().set_rate(1.0), Err(EngineError::Closed));
    assert!(matches!(
        h.engine()
            .open(OpenRequest::single(h.fixture().items[0].clone())),
        Err(EngineError::Closed)
    ));
    assert_eq!(rec.events().len(), count, "no events after Closed");
}

/// The fixture for the mock, also handy in UI tests.
pub fn mock_fixture() -> (Fixture, Vec<(String, MockMedia)>) {
    let audio = |id: &str, lang: &str, codec: &str, default: bool| Track {
        id: id.into(),
        language: Some(lang.into()),
        codec: Some(codec.into()),
        channels: Some(6),
        default,
        ..Track::default()
    };
    let subtitle = |id: &str, lang: &str| Track {
        id: id.into(),
        language: Some(lang.into()),
        codec: Some("subrip".into()),
        ..Track::default()
    };
    let media = |duration_ms| MockMedia {
        duration_ms,
        video_size: Some((1920, 1080)),
        video_codec: Some("hevc".into()),
        audio_tracks: vec![
            audio("1", "eng", "eac3", true),
            audio("2", "rus", "ac3", false),
        ],
        subtitle_tracks: vec![subtitle("1", "eng"), subtitle("2", "rus")],
    };
    let fixture = Fixture {
        items: [
            MediaSource::new("mock://episode-1.mkv"),
            MediaSource::new("mock://episode-2.mkv"),
        ],
        durations_ms: [30_000, 20_000],
        broken: MediaSource::new("mock://missing.mkv"),
        external_subtitle: "mock://episode-1.ru.srt".into(),
    };
    let registry = vec![
        ("mock://episode-1.mkv".into(), media(30_000)),
        ("mock://episode-2.mkv".into(), media(20_000)),
    ];
    (fixture, registry)
}

struct MockHarness {
    engine: MockEngine,
    fixture: Fixture,
}

impl Harness for MockHarness {
    fn engine(&self) -> &dyn PlayerEngine {
        &self.engine
    }
    fn fixture(&self) -> Fixture {
        self.fixture.clone()
    }
    fn advance(&self, ms: u64) {
        self.engine.advance(ms);
    }
}

/// A harness over a fresh [`MockEngine`] with [`mock_fixture`].
pub fn mock_harness() -> Box<dyn Harness> {
    let (fixture, registry) = mock_fixture();
    let engine = registry
        .into_iter()
        .fold(MockEngine::new(), |e, (uri, media)| {
            e.with_media(uri, media)
        });
    Box::new(MockHarness { engine, fixture })
}

/// Generates one `#[test]` per contract case for the harness factory
/// `$make: impl Fn() -> Box<dyn Harness>`.
#[macro_export]
macro_rules! player_engine_contract_tests {
    ($make:expr) => {
        $crate::__player_engine_contract_cases!(
            $make;
            open_loads_item,
            open_at_start_position,
            open_paused,
            open_broken_source,
            commands_need_open_item,
            play_pause,
            seek_clamps,
            rate,
            volume,
            select_tracks,
            external_subtitle,
            end_of_playlist,
            playlist_advance_and_jump,
            repeat_one,
            stats,
            close
        );
    };
}

#[doc(hidden)]
#[macro_export]
macro_rules! __player_engine_contract_cases {
    ($make:expr; $($case:ident),+ $(,)?) => {
        #[allow(unused_imports)]
        mod player_engine_contract {
            use super::*;

            fn harness() -> ::std::boxed::Box<dyn $crate::contract::Harness> {
                let make = $make;
                make()
            }

            $(
                #[test]
                fn $case() {
                    let h = harness();
                    $crate::contract::$case(&*h);
                }
            )+

            /// Fails if a case was added to `CASES` but not to the macro.
            #[test]
            fn macro_covers_all_cases() {
                let names = [$(stringify!($case)),+];
                let all: ::std::vec::Vec<&str> =
                    $crate::contract::CASES.iter().map(|(n, _)| *n).collect();
                assert_eq!(names.as_slice(), all.as_slice());
            }
        }
    };
}
