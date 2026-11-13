// SPDX-License-Identifier: MPL-2.0
//! The `PlayerEngine` contract through the FFI layer: the mock engine goes
//! core → FFI trait → core and must still pass every case, and the exported
//! contract runner reports passes and failures.

use std::sync::Arc;

use pa_ffi::{
    ContractError, CoreEngine, EngineEvent, EngineFixture, EngineState, EngineStateFold,
    ForeignEngine, PlayerEngine, PlayerEngineHarness, mock_engine_harness,
    player_engine_contract_cases, run_player_engine_contract_case,
};
use pa_playback::contract::{self, Fixture, Harness};

/// Mock → `CoreEngine` (FFI trait) → `ForeignEngine` (core trait).
struct RoundTrip {
    mock: Arc<pa_playback::MockEngine>,
    engine: ForeignEngine,
    fixture: Fixture,
}

impl Harness for RoundTrip {
    fn engine(&self) -> &dyn pa_playback::PlayerEngine {
        &self.engine
    }
    fn fixture(&self) -> Fixture {
        self.fixture.clone()
    }
    fn advance(&self, ms: u64) {
        self.mock.advance(ms);
    }
}

fn round_trip() -> Box<dyn Harness> {
    let (fixture, registry) = contract::mock_fixture();
    let mock = Arc::new(
        registry
            .into_iter()
            .fold(pa_playback::MockEngine::new(), |e, (uri, media)| {
                e.with_media(uri, media)
            }),
    );
    let ffi: Arc<dyn PlayerEngine> = Arc::new(CoreEngine(mock.clone()));
    Box::new(RoundTrip {
        mock,
        engine: ForeignEngine(ffi),
        fixture,
    })
}

pa_playback::player_engine_contract_tests!(round_trip);

#[test]
fn exported_runner_passes_the_mock() {
    let cases = player_engine_contract_cases();
    assert_eq!(cases.len(), contract::CASES.len());
    for name in cases {
        run_player_engine_contract_case(name.clone(), mock_engine_harness())
            .unwrap_or_else(|e| panic!("{name}: {e}"));
    }
}

#[test]
fn exported_runner_reports_unknown_cases_and_bad_fixtures() {
    let unknown = run_player_engine_contract_case("nope".into(), mock_engine_harness());
    assert!(matches!(unknown, Err(ContractError::UnknownCase { .. })));

    struct OneItem(Arc<dyn PlayerEngineHarness>);
    impl PlayerEngineHarness for OneItem {
        fn engine(&self) -> Arc<dyn PlayerEngine> {
            self.0.engine()
        }
        fn fixture(&self) -> EngineFixture {
            let mut f = self.0.fixture();
            f.items.truncate(1);
            f
        }
        fn advance(&self, ms: u64) {
            self.0.advance(ms);
        }
    }
    let bad = run_player_engine_contract_case(
        "open_loads_item".into(),
        Arc::new(OneItem(mock_engine_harness())),
    );
    assert!(matches!(bad, Err(ContractError::InvalidFixture { .. })));
}

#[test]
fn exported_runner_reports_a_broken_engine() {
    /// Never lets time pass: the position stays at the start.
    struct Frozen(Arc<dyn PlayerEngineHarness>);
    impl PlayerEngineHarness for Frozen {
        fn engine(&self) -> Arc<dyn PlayerEngine> {
            self.0.engine()
        }
        fn fixture(&self) -> EngineFixture {
            self.0.fixture()
        }
        fn advance(&self, _ms: u64) {}
    }
    let result = run_player_engine_contract_case(
        "play_pause".into(),
        Arc::new(Frozen(mock_engine_harness())),
    );
    match result {
        Err(ContractError::Failed { case, details }) => {
            assert_eq!(case, "play_pause");
            assert!(details.contains("position"), "{details}");
        }
        other => panic!("expected a failure, got {other:?}"),
    }
}

#[test]
fn fold_matches_core_reducer() {
    let fold = EngineStateFold::new();
    assert_eq!(
        fold.state(),
        EngineState::from(pa_playback::EngineState::default())
    );
    fold.apply(EngineEvent::RateChanged { rate: 1.5 });
    fold.apply(EngineEvent::Loading { index: 1 });
    let s = fold.apply(EngineEvent::Loaded {
        duration_ms: 10_000,
        video_width: Some(640),
        video_height: Some(360),
    });
    assert!(s.loaded && !s.buffering);
    assert_eq!((s.rate, s.playlist_index, s.duration_ms), (1.5, 1, 10_000));
    assert_eq!(fold.state(), s);
}
