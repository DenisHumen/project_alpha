// SPDX-License-Identifier: MPL-2.0
//! Playback session (PLAY-005) on the mock engine with a fake clock.
//!
//! "Done when": killing the process during playback loses at most 10 s of
//! position. The host below writes every save into a store (the database);
//! a kill can happen at any instant, so after every step the stored position
//! must be within 10 s of the engine position.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use pa_playback::session::{MAX_SAVE_INTERVAL_MS, MIN_SAVE_INTERVAL_MS};
use pa_playback::{
    EngineEvent, EngineListener, MediaSource, MockEngine, MockMedia, OpenRequest, PlaybackSession,
    PlayerEngine, SaveReason, SessionAction, SessionConfig, StartMode, UpNext, start_position,
};

const HOUR: u64 = 3_600_000;
const KILL_BOUND_MS: u64 = 10_000;

#[derive(Default)]
struct Queue(Mutex<Vec<EngineEvent>>);

impl EngineListener for Queue {
    fn on_event(&self, event: EngineEvent) {
        self.0.lock().unwrap().push(event);
    }
}

/// A host: mock engine, fake monotonic clock, progress store, session.
struct Host {
    engine: MockEngine,
    events: Arc<Queue>,
    now_ms: u64,
    session: PlaybackSession,
    /// item id → (position, duration): what survives a kill.
    store: HashMap<String, (u64, u64)>,
    saves: Vec<(String, u64, SaveReason)>,
    cards: Vec<UpNext>,
    play_next: Vec<String>,
}

fn media(duration_ms: u64) -> MockMedia {
    MockMedia {
        duration_ms,
        video_size: Some((1920, 1080)),
        ..MockMedia::default()
    }
}

impl Host {
    fn new(config: SessionConfig, items: &[(&str, u64)]) -> Self {
        let mut engine = MockEngine::new();
        for (id, d) in items {
            engine = engine.with_media(*id, media(*d));
        }
        let events = Arc::new(Queue::default());
        engine.set_listener(Some(events.clone()));
        let session = PlaybackSession::new(config, items[0].0, 0).unwrap();
        Self {
            engine,
            events,
            now_ms: 0,
            session,
            store: HashMap::new(),
            saves: Vec::new(),
            cards: Vec::new(),
            play_next: Vec::new(),
        }
    }

    /// Opens the current item the way the app does: from the stored
    /// position (resume) or from 0 (start over).
    fn open(&mut self, mode: StartMode) {
        let id = self.session.item_id().to_owned();
        let (pos, dur) = self.store.get(&id).copied().unwrap_or((0, 0));
        let start = start_position(mode, pos, Some(dur).filter(|d| *d > 0));
        self.engine
            .open(OpenRequest {
                start_position_ms: start,
                ..OpenRequest::single(MediaSource::new(id))
            })
            .unwrap();
        self.pump();
    }

    fn handle(&mut self, actions: Vec<SessionAction>) {
        for a in actions {
            match a {
                SessionAction::Save(s) => {
                    self.store
                        .insert(s.item_id.clone(), (s.position_ms, s.duration_ms));
                    self.saves.push((s.item_id, s.position_ms, s.reason));
                }
                SessionAction::UpNext(card) => self.cards.push(card),
                SessionAction::PlayNext { item_id } => self.play_next.push(item_id),
            }
        }
    }

    /// Delivers queued engine events to the session.
    fn pump(&mut self) {
        let events = std::mem::take(&mut *self.events.0.lock().unwrap());
        for e in events {
            let actions = self.session.on_event(&e, self.now_ms);
            self.handle(actions);
        }
    }

    /// `ms` of wall time in 1 s timer ticks.
    fn run(&mut self, ms: u64) {
        let mut left = ms;
        while left > 0 {
            let step = left.min(1_000);
            left -= step;
            self.now_ms += step;
            self.engine.advance(step);
            self.pump();
            let actions = self.session.tick(self.now_ms);
            self.handle(actions);
        }
    }

    fn stored(&self, id: &str) -> Option<u64> {
        self.store.get(id).map(|s| s.0)
    }

    fn position(&self) -> u64 {
        self.engine.state().position_ms
    }

    /// The kill invariant: what is stored is at most 10 s off.
    fn assert_kill_bound(&self) {
        let id = self.session.item_id();
        let pos = self.position();
        let stored = self.stored(id).unwrap_or(0);
        assert!(
            pos.abs_diff(stored) <= KILL_BOUND_MS,
            "at {} ms: engine {pos}, stored {stored}",
            self.now_ms
        );
    }
}

/// Tiny deterministic generator for the random walk.
struct Lcg(u64);

impl Lcg {
    fn next(&mut self, n: u64) -> u64 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1_442_695_040_888_963_407);
        (self.0 >> 33) % n
    }
}

#[test]
fn kill_at_any_moment_loses_at_most_10_s() {
    for interval in [MIN_SAVE_INTERVAL_MS, 7_500, MAX_SAVE_INTERVAL_MS] {
        for seed in 1..=8 {
            let config = SessionConfig {
                save_interval_ms: interval,
                ..SessionConfig::default()
            };
            let mut h = Host::new(config, &[("movie", 2 * HOUR)]);
            h.open(StartMode::Resume);
            let mut rng = Lcg(seed);
            for _ in 0..400 {
                match rng.next(20) {
                    0 => {
                        let to = rng.next(2 * HOUR - 60_000);
                        h.engine.seek(to).unwrap();
                    }
                    1 => h.engine.pause().unwrap(),
                    2 => h.engine.play().unwrap(),
                    3 => {
                        let rate = [0.5, 1.0, 1.5, 2.0][rng.next(4) as usize];
                        h.engine.set_rate(rate).unwrap();
                    }
                    _ => {}
                }
                h.pump();
                h.assert_kill_bound();
                for _ in 0..rng.next(15) + 1 {
                    h.run(1_000);
                    h.assert_kill_bound();
                }
            }
            assert!(h.saves.len() > 10, "interval {interval}, seed {seed}");
        }
    }
}

#[test]
fn saves_every_interval_and_on_pause_seek_background_exit() {
    let mut h = Host::new(SessionConfig::default(), &[("movie", HOUR)]);
    h.store.insert("movie".into(), (600_000, HOUR));
    h.open(StartMode::Resume);
    assert_eq!(h.position(), 600_000, "resumed");
    assert!(h.saves.is_empty(), "opening does not save");

    h.run(4_000);
    assert!(h.saves.is_empty());
    h.run(1_000);
    assert_eq!(h.saves, [("movie".into(), 605_000, SaveReason::Interval)]);
    h.run(5_000);
    assert_eq!(h.saves.last().unwrap().1, 610_000);

    h.run(2_000);
    h.engine.pause().unwrap();
    h.pump();
    assert_eq!(
        h.saves.last().unwrap(),
        &("movie".into(), 612_000, SaveReason::Pause)
    );
    // Paused: no periodic saves.
    let n = h.saves.len();
    h.run(30_000);
    assert_eq!(h.saves.len(), n);

    h.engine.seek(1_200_000).unwrap();
    h.pump();
    assert_eq!(
        h.saves.last().unwrap(),
        &("movie".into(), 1_200_000, SaveReason::Seek)
    );

    h.engine.play().unwrap();
    h.pump();
    h.run(3_000);
    let actions = h.session.on_background(h.now_ms);
    h.handle(actions);
    assert_eq!(
        h.saves.last().unwrap(),
        &("movie".into(), 1_203_000, SaveReason::Background)
    );
    // Nothing new to save: no duplicate.
    let n = h.saves.len();
    let actions = h.session.on_background(h.now_ms);
    h.handle(actions);
    assert_eq!(h.saves.len(), n);

    h.run(1_500);
    let actions = h.session.on_exit();
    h.handle(actions);
    assert_eq!(
        h.saves.last().unwrap(),
        &("movie".into(), 1_204_500, SaveReason::Exit)
    );
    assert!(h.session.on_exit().is_empty(), "exit is idempotent");
    h.run(20_000);
    assert_eq!(h.saves.last().unwrap().2, SaveReason::Exit);
}

#[test]
fn fast_playback_saves_by_position_too() {
    let config = SessionConfig {
        save_interval_ms: MAX_SAVE_INTERVAL_MS,
        ..SessionConfig::default()
    };
    let mut h = Host::new(config, &[("movie", HOUR)]);
    h.open(StartMode::StartOver);
    h.engine.set_rate(2.0).unwrap();
    h.pump();
    h.run(5_000);
    let positions: Vec<u64> = h.saves.iter().map(|s| s.1).collect();
    assert_eq!(positions, [10_000], "10 s of position in 5 s of wall time");
    h.run(5_000);
    assert_eq!(h.saves.last().unwrap().1, 20_000);
}

#[test]
fn start_over_ignores_the_saved_position() {
    let mut h = Host::new(SessionConfig::default(), &[("movie", HOUR)]);
    h.store.insert("movie".into(), (600_000, HOUR));
    h.open(StartMode::StartOver);
    assert_eq!(h.position(), 0);
}

#[test]
fn up_next_counts_down_and_plays_the_next_episode() {
    let mut h = Host::new(SessionConfig::default(), &[("e1", 60_000), ("e2", 60_000)]);
    let actions = h.session.set_next(Some("e2".into()), 0);
    assert!(actions.is_empty(), "nothing shown before the end");
    h.open(StartMode::Resume);
    h.run(60_000);
    assert_eq!(h.stored("e1"), Some(60_000), "end saved as the duration");
    assert_eq!(
        h.cards.first(),
        Some(&UpNext::Countdown {
            item_id: "e2".into(),
            remaining_ms: 10_000
        })
    );
    h.run(9_000);
    assert_eq!(
        h.session.up_next(),
        &UpNext::Countdown {
            item_id: "e2".into(),
            remaining_ms: 1_000
        }
    );
    assert!(h.play_next.is_empty());
    h.run(1_000);
    assert_eq!(h.play_next, ["e2"]);
    assert_eq!(h.session.up_next(), &UpNext::Hidden);

    // The host switches items; saves go to e2 now.
    let actions = h.session.begin_item("e2", h.now_ms);
    h.handle(actions);
    h.open(StartMode::Resume);
    h.run(5_000);
    assert_eq!(
        h.saves.last().unwrap(),
        &("e2".into(), 5_000, SaveReason::Interval)
    );
    assert_eq!(h.stored("e1"), Some(60_000));
}

#[test]
fn up_next_play_now_cancel_and_seek_back() {
    let mut h = Host::new(SessionConfig::default(), &[("e1", 60_000)]);
    h.session.set_next(Some("e2".into()), 0);
    h.open(StartMode::StartOver);
    h.run(60_000);
    let actions = h.session.cancel_up_next();
    assert_eq!(actions, [SessionAction::UpNext(UpNext::Hidden)]);
    h.run(20_000);
    assert!(h.play_next.is_empty(), "cancelled countdown never fires");

    // Ends again: the card comes back; seeking back hides it.
    h.engine.play().unwrap();
    h.pump();
    h.run(60_000);
    assert!(matches!(h.session.up_next(), UpNext::Countdown { .. }));
    h.engine.seek(30_000).unwrap();
    h.pump();
    assert_eq!(h.session.up_next(), &UpNext::Hidden);

    h.engine.play().unwrap();
    h.pump();
    h.run(30_000);
    h.run(2_000);
    let actions = h.session.play_next_now();
    assert_eq!(
        actions,
        [SessionAction::PlayNext {
            item_id: "e2".into()
        }]
    );
}

#[test]
fn up_next_without_continuous_playback_waits() {
    let config = SessionConfig {
        continuous: false,
        ..SessionConfig::default()
    };
    let mut h = Host::new(config, &[("e1", 60_000)]);
    h.session.set_next(Some("e2".into()), 0);
    h.open(StartMode::StartOver);
    h.run(120_000);
    assert_eq!(
        h.session.up_next(),
        &UpNext::Offered {
            item_id: "e2".into()
        }
    );
    assert!(h.play_next.is_empty());
}

#[test]
fn late_next_and_background_stop_the_countdown() {
    let mut h = Host::new(SessionConfig::default(), &[("e1", 60_000)]);
    h.open(StartMode::StartOver);
    h.run(61_000);
    assert_eq!(h.session.up_next(), &UpNext::Hidden, "nothing to offer yet");
    // The library answers after the end.
    let actions = h.session.set_next(Some("e2".into()), h.now_ms);
    h.handle(actions);
    assert!(matches!(h.session.up_next(), UpNext::Countdown { .. }));
    h.run(3_000);
    let actions = h.session.on_background(h.now_ms);
    h.handle(actions);
    assert_eq!(
        h.session.up_next(),
        &UpNext::Offered {
            item_id: "e2".into()
        }
    );
    h.run(30_000);
    assert!(h.play_next.is_empty(), "no autoplay in the background");
}

#[test]
fn error_and_close_save_the_last_position() {
    let mut h = Host::new(SessionConfig::default(), &[("movie", HOUR)]);
    h.open(StartMode::StartOver);
    h.run(3_000);
    h.engine.close().unwrap();
    h.pump();
    assert_eq!(
        h.saves.last().unwrap(),
        &("movie".into(), 3_000, SaveReason::Exit)
    );
    assert!(h.session.tick(h.now_ms + 60_000).is_empty());

    let mut h = Host::new(SessionConfig::default(), &[("movie", HOUR)]);
    h.open(StartMode::StartOver);
    h.run(4_000);
    let actions = h.session.on_event(
        &EngineEvent::Error {
            error: pa_playback::EngineError::Decoder {
                details: "broken frame".into(),
            },
        },
        h.now_ms,
    );
    h.handle(actions);
    assert_eq!(
        h.saves.last().unwrap(),
        &("movie".into(), 4_000, SaveReason::Error)
    );
}
