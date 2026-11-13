// SPDX-License-Identifier: MPL-2.0
//! The PlayerEngine contract suite against the mock (PLAY-003 "done when").

use std::sync::Arc;

use pa_playback::contract::{self, Harness};
use pa_playback::{
    EngineError, EngineEvent, EngineKind, EngineListener, EngineState, MockEngine, OpenRequest,
    PlaybackStats, PlayerEngine, RepeatMode,
};

pa_playback::player_engine_contract_tests!(contract::mock_harness);

#[test]
fn run_all_passes() {
    contract::run_all(&contract::mock_harness);
}

/// The suite is not vacuous: an engine that ignores `pause` fails it.
#[test]
#[should_panic(expected = "play_pause")]
fn broken_engine_fails() {
    /// The mock, except that `pause` is accepted and does nothing.
    struct IgnoresPause(MockEngine, contract::Fixture);

    impl Harness for IgnoresPause {
        fn engine(&self) -> &dyn PlayerEngine {
            self
        }
        fn fixture(&self) -> contract::Fixture {
            self.1.clone()
        }
        fn advance(&self, ms: u64) {
            self.0.advance(ms)
        }
    }

    impl PlayerEngine for IgnoresPause {
        fn kind(&self) -> EngineKind {
            self.0.kind()
        }
        fn set_listener(&self, l: Option<Arc<dyn EngineListener>>) {
            self.0.set_listener(l)
        }
        fn state(&self) -> EngineState {
            self.0.state()
        }
        fn open(&self, r: OpenRequest) -> Result<(), EngineError> {
            self.0.open(r)
        }
        fn play(&self) -> Result<(), EngineError> {
            self.0.play()
        }
        fn pause(&self) -> Result<(), EngineError> {
            if self.0.state().loaded {
                Ok(())
            } else {
                self.0.pause()
            }
        }
        fn seek(&self, p: u64) -> Result<(), EngineError> {
            self.0.seek(p)
        }
        fn set_rate(&self, r: f64) -> Result<(), EngineError> {
            self.0.set_rate(r)
        }
        fn set_volume(&self, v: f64) -> Result<(), EngineError> {
            self.0.set_volume(v)
        }
        fn set_repeat(&self, r: RepeatMode) -> Result<(), EngineError> {
            self.0.set_repeat(r)
        }
        fn jump(&self, i: u32) -> Result<(), EngineError> {
            self.0.jump(i)
        }
        fn set_audio_track(&self, id: Option<String>) -> Result<(), EngineError> {
            self.0.set_audio_track(id)
        }
        fn set_subtitle_track(&self, id: Option<String>) -> Result<(), EngineError> {
            self.0.set_subtitle_track(id)
        }
        fn add_external_subtitle(&self, u: String, t: Option<String>) -> Result<(), EngineError> {
            self.0.add_external_subtitle(u, t)
        }
        fn stats(&self) -> Result<PlaybackStats, EngineError> {
            self.0.stats()
        }
        fn close(&self) -> Result<(), EngineError> {
            self.0.close()
        }
    }

    contract::run_all(&|| {
        let (fixture, media) = contract::mock_fixture();
        let engine = media
            .into_iter()
            .fold(MockEngine::new(), |e, (uri, m)| e.with_media(uri, m));
        Box::new(IgnoresPause(engine, fixture))
    });
}

#[test]
fn events_serialize_as_tagged_json() {
    let event = EngineEvent::Position {
        position_ms: 1_500,
        buffered_ms: 6_500,
    };
    let json = serde_json::to_string(&event).expect("serialize");
    assert_eq!(
        json,
        r#"{"event":"position","position_ms":1500,"buffered_ms":6500}"#
    );
    let error = EngineEvent::Error {
        error: EngineError::Source {
            details: "x".into(),
        },
    };
    let back: EngineEvent =
        serde_json::from_str(&serde_json::to_string(&error).expect("serialize")).expect("parse");
    assert_eq!(back, error);
}

#[test]
fn listener_may_call_back_into_the_engine() {
    struct Reentrant(std::sync::Mutex<Option<Arc<MockEngine>>>);
    impl EngineListener for Reentrant {
        fn on_event(&self, _: EngineEvent) {
            if let Some(engine) = self.0.lock().expect("lock").as_ref() {
                let _ = engine.state();
            }
        }
    }
    let (fixture, media) = contract::mock_fixture();
    let engine = Arc::new(
        media
            .into_iter()
            .fold(MockEngine::new(), |e, (u, m)| e.with_media(u, m)),
    );
    let listener = Arc::new(Reentrant(std::sync::Mutex::new(Some(engine.clone()))));
    engine.set_listener(Some(listener.clone()));
    engine
        .open(OpenRequest::single(fixture.items[0].clone()))
        .expect("open does not deadlock");
    // Break the Arc cycle engine → listener → engine.
    *listener.0.lock().expect("lock") = None;
}
