// SPDX-License-Identifier: MPL-2.0
//! The PlayerEngine contract through the FFI foreign traits: the mock,
//! wrapped as a "platform" engine, passes every case via
//! `player_engine_contract_run`, the path the Kotlin and Swift engines take.

use std::sync::Arc;

use pa_ffi::{
    ContractFixture, EngineListener, PlayerEngine, PlayerEngineHarness, engine_event_to_json,
    engine_state_apply, engine_state_default, player_engine_contract_cases,
    player_engine_contract_run,
};
use pa_playback::contract;
use pa_playback::{
    EngineError, EngineEvent, EngineKind, EngineState, MockEngine, OpenRequest, PlaybackStats,
    PlayerEngine as CoreEngine, RepeatMode,
};

/// Forwards core events to an FFI listener.
struct ToFfi(Arc<dyn EngineListener>);

impl pa_playback::EngineListener for ToFfi {
    fn on_event(&self, event: EngineEvent) {
        self.0.on_event(event);
    }
}

/// The mock behind the FFI trait, as a Kotlin engine would implement it.
struct Platform {
    mock: MockEngine,
    /// `pause` is accepted and ignored: the suite must notice.
    ignore_pause: bool,
}

impl PlayerEngine for Platform {
    fn kind(&self) -> EngineKind {
        self.mock.kind()
    }
    fn set_listener(&self, listener: Option<Arc<dyn EngineListener>>) {
        self.mock.set_listener(
            listener.map(|l| Arc::new(ToFfi(l)) as Arc<dyn pa_playback::EngineListener>),
        );
    }
    fn state(&self) -> EngineState {
        self.mock.state()
    }
    fn open_media(&self, request: OpenRequest) -> Result<(), EngineError> {
        self.mock.open(request)
    }
    fn play(&self) -> Result<(), EngineError> {
        self.mock.play()
    }
    fn pause(&self) -> Result<(), EngineError> {
        if self.ignore_pause && self.mock.state().loaded {
            return Ok(());
        }
        self.mock.pause()
    }
    fn seek(&self, position_ms: u64) -> Result<(), EngineError> {
        self.mock.seek(position_ms)
    }
    fn set_rate(&self, rate: f64) -> Result<(), EngineError> {
        self.mock.set_rate(rate)
    }
    fn set_volume(&self, volume: f64) -> Result<(), EngineError> {
        self.mock.set_volume(volume)
    }
    fn set_repeat(&self, repeat: RepeatMode) -> Result<(), EngineError> {
        self.mock.set_repeat(repeat)
    }
    fn jump(&self, index: u32) -> Result<(), EngineError> {
        self.mock.jump(index)
    }
    fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.mock.set_audio_track(id)
    }
    fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError> {
        self.mock.set_subtitle_track(id)
    }
    fn add_external_subtitle(&self, uri: String, title: Option<String>) -> Result<(), EngineError> {
        self.mock.add_external_subtitle(uri, title)
    }
    fn stats(&self) -> Result<PlaybackStats, EngineError> {
        self.mock.stats()
    }
    fn close_engine(&self) -> Result<(), EngineError> {
        self.mock.close()
    }
}

struct MockHarness {
    engine: Arc<Platform>,
    fixture: contract::Fixture,
}

impl MockHarness {
    fn new(ignore_pause: bool) -> Self {
        let (fixture, media) = contract::mock_fixture();
        let mock = media
            .into_iter()
            .fold(MockEngine::new(), |e, (uri, m)| e.with_media(uri, m));
        Self {
            engine: Arc::new(Platform { mock, ignore_pause }),
            fixture,
        }
    }
}

impl PlayerEngineHarness for MockHarness {
    fn engine(&self) -> Arc<dyn PlayerEngine> {
        self.engine.clone()
    }
    fn fixture(&self) -> ContractFixture {
        ContractFixture {
            items: self.fixture.items.to_vec(),
            durations_ms: self.fixture.durations_ms.to_vec(),
            broken: self.fixture.broken.clone(),
            external_subtitle: self.fixture.external_subtitle.clone(),
        }
    }
    fn advance(&self, ms: u64) {
        self.engine.mock.advance(ms);
    }
}

#[test]
fn mock_passes_every_case_over_ffi() {
    let cases = player_engine_contract_cases();
    assert_eq!(cases.len(), contract::CASES.len());
    for case in cases {
        let failure = player_engine_contract_run(case.clone(), Arc::new(MockHarness::new(false)));
        assert_eq!(failure, None, "case {case}");
    }
}

#[test]
fn a_broken_engine_fails_over_ffi() {
    let failure = player_engine_contract_run("play_pause".into(), Arc::new(MockHarness::new(true)));
    assert!(failure.is_some(), "ignoring pause must fail play_pause");
}

#[test]
fn unknown_case_and_bad_fixture_are_reported() {
    let failure = player_engine_contract_run("nope".into(), Arc::new(MockHarness::new(false)));
    assert!(failure.unwrap().contains("unknown"));

    struct OneItem(MockHarness);
    impl PlayerEngineHarness for OneItem {
        fn engine(&self) -> Arc<dyn PlayerEngine> {
            self.0.engine()
        }
        fn fixture(&self) -> ContractFixture {
            let mut fx = self.0.fixture();
            fx.items.pop();
            fx
        }
        fn advance(&self, ms: u64) {
            self.0.advance(ms)
        }
    }
    let failure = player_engine_contract_run(
        "open_loads_item".into(),
        Arc::new(OneItem(MockHarness::new(false))),
    );
    assert!(failure.unwrap().contains("two items"));
}

#[test]
fn reducer_and_json_match_the_core() {
    let event = EngineEvent::Position {
        position_ms: 1_500,
        buffered_ms: 6_500,
    };
    let state = engine_state_apply(engine_state_default(), event.clone());
    assert_eq!(state, EngineState::from_events([&event]));
    assert_eq!(
        engine_event_to_json(event),
        r#"{"event":"position","position_ms":1500,"buffered_ms":6500}"#
    );
}

/// `mock_engine_harness`: the core mock behind the FFI traits
/// (`CoreEngine`), as the Swift round-trip test uses it.
#[test]
fn exported_mock_harness_passes_every_case() {
    for case in player_engine_contract_cases() {
        let failure = player_engine_contract_run(case.clone(), pa_ffi::mock_engine_harness());
        assert_eq!(failure, None, "{case}");
    }
}
