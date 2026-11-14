// SPDX-License-Identifier: MPL-2.0
//! Playback session (PLAY-005, docs/research/01 §2.7): where to start, when
//! to save progress, and Up Next with a countdown.
//!
//! [`PlaybackSession`] is a plain state machine without threads, timers or
//! storage. The host feeds it the [`EngineEvent`]s of its engine, a timer
//! tick (about once a second) and app lifecycle changes, each with the
//! current time of a monotonic clock, and carries out the returned
//! [`SessionAction`]s: write progress (`pa-library` `report_position`), show
//! or update the Up Next card, open the next item. So the same logic runs
//! under every engine and is tested with the mock engine and a fake clock.
//!
//! Progress is saved:
//!
//! - while playing, every [`SessionConfig::save_interval_ms`] of wall time
//!   **or** of position, whichever comes first (so fast playback does not
//!   lose more position); the interval is 5–10 s, so a killed process loses
//!   at most the interval plus one tick;
//! - on pause, on a seek, on an error, at the end of the item;
//! - when the app goes to the background ([`PlaybackSession::on_background`])
//!   and on exit ([`PlaybackSession::on_exit`]).
//!
//! A save that repeats the previous one (same position and duration) is
//! dropped.
//!
//! Up Next: when the item ends and the host has given a next item
//! ([`PlaybackSession::set_next`], e.g. `pa-library` `next_episode`), the
//! session shows it. With continuous playback on it counts down
//! [`SessionConfig::up_next_countdown_ms`] and then asks the host to play
//! it; "Play now" skips the countdown, "Cancel" hides the card. Going to the
//! background stops the countdown (the card stays, without a timer), and
//! seeking back into the item hides the card.

use pa_model::{IN_PROGRESS_MAX, IN_PROGRESS_MIN};
use serde::{Deserialize, Serialize};

use crate::engine::{EngineEvent, EngineState};

/// Shortest allowed progress save interval.
pub const MIN_SAVE_INTERVAL_MS: u64 = 5_000;
/// Longest allowed progress save interval: the "done when" bound of PLAY-005
/// (a killed process loses at most 10 s).
pub const MAX_SAVE_INTERVAL_MS: u64 = 10_000;
/// Default progress save interval.
pub const DEFAULT_SAVE_INTERVAL_MS: u64 = 5_000;
/// Default Up Next countdown.
pub const DEFAULT_UP_NEXT_COUNTDOWN_MS: u64 = 10_000;
/// Longest allowed Up Next countdown.
pub const MAX_UP_NEXT_COUNTDOWN_MS: u64 = 60_000;
/// A position change that differs from the expected playback progress by
/// more than this is treated as a seek.
pub const SEEK_TOLERANCE_MS: u64 = 2_000;

/// How to start an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum StartMode {
    /// From the saved position if the item is in progress.
    #[default]
    Resume,
    /// "Start over": from the beginning, whatever was saved.
    StartOver,
}

/// Where to open an item, given its saved position.
///
/// [`StartMode::Resume`] resumes only an item in progress: a position below
/// 2 % or beyond 95 % of a known duration (the "Continue watching" window of
/// `pa-model`) starts from the beginning. With an unknown duration a saved
/// position is trusted.
pub fn start_position(mode: StartMode, saved_position_ms: u64, duration_ms: Option<u64>) -> u64 {
    if mode == StartMode::StartOver || saved_position_ms == 0 {
        return 0;
    }
    match duration_ms.filter(|d| *d > 0) {
        Some(d) => {
            let f = saved_position_ms as f64 / d as f64;
            if (IN_PROGRESS_MIN..=IN_PROGRESS_MAX).contains(&f) {
                saved_position_ms
            } else {
                0
            }
        }
        None => saved_position_ms,
    }
}

/// Session settings.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Periodic save interval, [`MIN_SAVE_INTERVAL_MS`]..=[`MAX_SAVE_INTERVAL_MS`].
    pub save_interval_ms: u64,
    /// Continuous playback: count down and play the next item by itself.
    /// Off: the Up Next card waits for "Play now".
    pub continuous: bool,
    /// Up Next countdown, `1..=`[`MAX_UP_NEXT_COUNTDOWN_MS`].
    pub up_next_countdown_ms: u64,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            save_interval_ms: DEFAULT_SAVE_INTERVAL_MS,
            continuous: true,
            up_next_countdown_ms: DEFAULT_UP_NEXT_COUNTDOWN_MS,
        }
    }
}

impl SessionConfig {
    /// Checks the ranges.
    pub fn validate(&self) -> Result<(), SessionError> {
        if !(MIN_SAVE_INTERVAL_MS..=MAX_SAVE_INTERVAL_MS).contains(&self.save_interval_ms) {
            return Err(SessionError::InvalidConfig {
                details: format!("save interval {} ms", self.save_interval_ms),
            });
        }
        if !(1..=MAX_UP_NEXT_COUNTDOWN_MS).contains(&self.up_next_countdown_ms) {
            return Err(SessionError::InvalidConfig {
                details: format!("up next countdown {} ms", self.up_next_countdown_ms),
            });
        }
        Ok(())
    }
}

/// Why a session cannot be created.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub enum SessionError {
    #[error("invalid session config: {details}")]
    InvalidConfig { details: String },
}

/// Why progress is saved.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SaveReason {
    Interval,
    Pause,
    Seek,
    /// The item played to its end; the position is the duration.
    Ended,
    Error,
    Background,
    Exit,
}

/// A progress write for the host (`pa-library` `report_position`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ProgressSave {
    /// The host's id of the item (a media key, a library item id).
    pub item_id: String,
    pub position_ms: u64,
    pub duration_ms: u64,
    pub reason: SaveReason,
}

/// The Up Next card.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "state")]
pub enum UpNext {
    #[default]
    Hidden,
    /// Plays `item_id` when `remaining_ms` reaches zero.
    Countdown { item_id: String, remaining_ms: u64 },
    /// Shown without a countdown: waits for "Play now".
    Offered { item_id: String },
}

/// What the host must do.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case", tag = "action")]
pub enum SessionAction {
    /// Write progress.
    Save(ProgressSave),
    /// Show, update or hide the Up Next card.
    UpNext(UpNext),
    /// Open `item_id` (from its beginning, or [`start_position`]) and call
    /// [`PlaybackSession::begin_item`].
    PlayNext { item_id: String },
}

/// One playback session: the current item, its engine state, saves and Up
/// Next. Times (`now_ms`) come from a monotonic clock of the host.
#[derive(Debug, Clone)]
pub struct PlaybackSession {
    config: SessionConfig,
    item_id: String,
    state: EngineState,
    next: Option<String>,
    up_next: UpNext,
    countdown_deadline_ms: u64,
    /// The user cancelled Up Next for this ending.
    dismissed: bool,
    last_save_ms: u64,
    /// Last written (position, duration) of the current item.
    last_saved: Option<(u64, u64)>,
    /// Position of the last save, or the first position of the item:
    /// periodic saves also fire after this much playback.
    mark_pos: Option<u64>,
    /// Position and time of the last position or play/pause change, for
    /// seek detection. `None` until the first position of the item.
    anchor: Option<(u64, u64)>,
    exited: bool,
}

impl PlaybackSession {
    /// A session for `item_id`, started at `now_ms`. The host opens the
    /// engine itself (at [`start_position`]).
    pub fn new(
        config: SessionConfig,
        item_id: impl Into<String>,
        now_ms: u64,
    ) -> Result<Self, SessionError> {
        config.validate()?;
        Ok(Self {
            config,
            item_id: item_id.into(),
            state: EngineState::default(),
            next: None,
            up_next: UpNext::Hidden,
            countdown_deadline_ms: 0,
            dismissed: false,
            last_save_ms: now_ms,
            last_saved: None,
            mark_pos: None,
            anchor: None,
            exited: false,
        })
    }

    pub fn config(&self) -> &SessionConfig {
        &self.config
    }

    /// Id of the current item.
    pub fn item_id(&self) -> &str {
        &self.item_id
    }

    /// Engine state as seen through the events.
    pub fn state(&self) -> &EngineState {
        &self.state
    }

    pub fn up_next(&self) -> &UpNext {
        &self.up_next
    }

    /// The item to offer when this one ends (`None`: nothing follows). May
    /// come late, e.g. after an async library query; if the item has
    /// already ended, the card is shown now.
    pub fn set_next(&mut self, item_id: Option<String>, now_ms: u64) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if self.exited || self.next == item_id {
            return out;
        }
        self.next = item_id;
        if self.next.is_none() {
            self.set_up_next(UpNext::Hidden, &mut out);
        } else if self.state.completed && !self.dismissed {
            self.show_up_next(now_ms, &mut out);
        }
        out
    }

    /// The host opened a new item (after [`SessionAction::PlayNext`] or a
    /// manual switch): saves of the old item stop, Up Next resets.
    pub fn begin_item(&mut self, item_id: impl Into<String>, now_ms: u64) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        self.save(SaveReason::Exit, &mut out);
        self.item_id = item_id.into();
        self.state = EngineState {
            rate: self.state.rate,
            volume: self.state.volume,
            repeat: self.state.repeat,
            ..EngineState::default()
        };
        self.next = None;
        self.dismissed = false;
        self.set_up_next(UpNext::Hidden, &mut out);
        self.last_save_ms = now_ms;
        self.last_saved = None;
        self.mark_pos = None;
        self.anchor = None;
        out
    }

    /// Feeds one engine event.
    pub fn on_event(&mut self, event: &EngineEvent, now_ms: u64) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        let before = self.state.clone();
        self.state.apply(event);
        match event {
            EngineEvent::Loading { .. } => {
                self.anchor = None;
                self.last_saved = None;
                self.mark_pos = None;
            }
            EngineEvent::Position { position_ms, .. } => {
                if self.is_seek(&before, *position_ms, now_ms) {
                    self.save(SaveReason::Seek, &mut out);
                    self.last_save_ms = now_ms;
                }
                self.anchor = Some((*position_ms, now_ms));
                self.mark_pos.get_or_insert(*position_ms);
                if !self.state.completed {
                    self.dismissed = false;
                    self.set_up_next(UpNext::Hidden, &mut out);
                }
                self.periodic(now_ms, &mut out);
            }
            EngineEvent::PlayingChanged { playing } => {
                if self.anchor.is_some() {
                    self.anchor = Some((self.state.position_ms, now_ms));
                }
                if before.playing && !playing && !self.state.completed {
                    self.save(SaveReason::Pause, &mut out);
                    self.last_save_ms = now_ms;
                }
            }
            EngineEvent::Ended => {
                self.state.position_ms = self.state.duration_ms;
                self.save(SaveReason::Ended, &mut out);
                self.last_save_ms = now_ms;
                self.show_up_next(now_ms, &mut out);
            }
            EngineEvent::Error { .. } => {
                self.save(SaveReason::Error, &mut out);
                self.last_save_ms = now_ms;
            }
            EngineEvent::Closed => self.exit(&mut out),
            _ => {}
        }
        out
    }

    /// Timer tick (about once a second, also while paused): periodic saves
    /// and the Up Next countdown.
    pub fn tick(&mut self, now_ms: u64) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        self.periodic(now_ms, &mut out);
        if let UpNext::Countdown { item_id, .. } = &self.up_next {
            let item_id = item_id.clone();
            let remaining_ms = self.countdown_deadline_ms.saturating_sub(now_ms);
            if remaining_ms == 0 {
                self.play_next(&mut out);
            } else {
                self.set_up_next(
                    UpNext::Countdown {
                        item_id,
                        remaining_ms,
                    },
                    &mut out,
                );
            }
        }
        out
    }

    /// The app goes to the background (or loses focus on a TV): saves now
    /// and stops the Up Next countdown.
    pub fn on_background(&mut self, now_ms: u64) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if self.exited {
            return out;
        }
        self.save(SaveReason::Background, &mut out);
        self.last_save_ms = now_ms;
        if let UpNext::Countdown { item_id, .. } = &self.up_next {
            let item_id = item_id.clone();
            self.set_up_next(UpNext::Offered { item_id }, &mut out);
        }
        out
    }

    /// The player closes: the final save. Later calls do nothing.
    pub fn on_exit(&mut self) -> Vec<SessionAction> {
        let mut out = Vec::new();
        self.exit(&mut out);
        out
    }

    /// "Play now" on the Up Next card.
    pub fn play_next_now(&mut self) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if !self.exited && self.up_next != UpNext::Hidden {
            self.play_next(&mut out);
        }
        out
    }

    /// "Cancel" on the Up Next card: hides it until the item ends again.
    pub fn cancel_up_next(&mut self) -> Vec<SessionAction> {
        let mut out = Vec::new();
        if !self.exited && self.up_next != UpNext::Hidden {
            self.dismissed = true;
            self.set_up_next(UpNext::Hidden, &mut out);
        }
        out
    }

    // ---- internals --------------------------------------------------------

    fn exit(&mut self, out: &mut Vec<SessionAction>) {
        if self.exited {
            return;
        }
        self.save(SaveReason::Exit, out);
        self.set_up_next(UpNext::Hidden, out);
        self.exited = true;
    }

    /// A position that does not follow from playback since the anchor.
    fn is_seek(&self, before: &EngineState, position_ms: u64, now_ms: u64) -> bool {
        let Some((anchor_pos, anchor_ms)) = self.anchor else {
            return false;
        };
        let expected = if before.playing {
            let elapsed = now_ms.saturating_sub(anchor_ms) as f64 * before.rate.max(0.0);
            anchor_pos.saturating_add(elapsed.round() as u64)
        } else {
            anchor_pos
        };
        position_ms.abs_diff(expected) > SEEK_TOLERANCE_MS
    }

    fn periodic(&mut self, now_ms: u64, out: &mut Vec<SessionAction>) {
        if !self.state.playing {
            return;
        }
        let interval = self.config.save_interval_ms;
        let moved = self
            .mark_pos
            .is_some_and(|pos| pos.abs_diff(self.state.position_ms) >= interval);
        let wall = now_ms.saturating_sub(self.last_save_ms);
        if wall >= interval || moved {
            self.save(SaveReason::Interval, out);
            self.last_save_ms = now_ms;
        }
    }

    fn save(&mut self, reason: SaveReason, out: &mut Vec<SessionAction>) {
        let s = &self.state;
        if !s.loaded || s.duration_ms == 0 {
            return;
        }
        let position_ms = s.position_ms.min(s.duration_ms);
        let saved = (position_ms, s.duration_ms);
        self.mark_pos = Some(position_ms);
        if self.last_saved == Some(saved) {
            return;
        }
        self.last_saved = Some(saved);
        out.push(SessionAction::Save(ProgressSave {
            item_id: self.item_id.clone(),
            position_ms,
            duration_ms: s.duration_ms,
            reason,
        }));
    }

    fn show_up_next(&mut self, now_ms: u64, out: &mut Vec<SessionAction>) {
        let Some(item_id) = self.next.clone() else {
            return;
        };
        let card = if self.config.continuous {
            self.countdown_deadline_ms = now_ms + self.config.up_next_countdown_ms;
            UpNext::Countdown {
                item_id,
                remaining_ms: self.config.up_next_countdown_ms,
            }
        } else {
            UpNext::Offered { item_id }
        };
        self.set_up_next(card, out);
    }

    fn play_next(&mut self, out: &mut Vec<SessionAction>) {
        let item_id = match &self.up_next {
            UpNext::Countdown { item_id, .. } | UpNext::Offered { item_id } => item_id.clone(),
            UpNext::Hidden => return,
        };
        self.up_next = UpNext::Hidden;
        out.push(SessionAction::PlayNext { item_id });
    }

    fn set_up_next(&mut self, card: UpNext, out: &mut Vec<SessionAction>) {
        if self.up_next != card {
            self.up_next = card.clone();
            out.push(SessionAction::UpNext(card));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn start_position_rules() {
        let d = Some(100_000);
        assert_eq!(start_position(StartMode::Resume, 50_000, d), 50_000);
        assert_eq!(start_position(StartMode::StartOver, 50_000, d), 0);
        // Below 2 % and beyond 95 %: from the beginning.
        assert_eq!(start_position(StartMode::Resume, 1_000, d), 0);
        assert_eq!(start_position(StartMode::Resume, 96_000, d), 0);
        assert_eq!(start_position(StartMode::Resume, 2_000, d), 2_000);
        assert_eq!(start_position(StartMode::Resume, 95_000, d), 95_000);
        // Unknown duration: trust the position.
        assert_eq!(start_position(StartMode::Resume, 7_000, None), 7_000);
        assert_eq!(start_position(StartMode::Resume, 7_000, Some(0)), 7_000);
        assert_eq!(start_position(StartMode::Resume, 0, None), 0);
    }

    #[test]
    fn config_ranges() {
        assert!(SessionConfig::default().validate().is_ok());
        for bad in [
            SessionConfig {
                save_interval_ms: 4_999,
                ..Default::default()
            },
            SessionConfig {
                save_interval_ms: 10_001,
                ..Default::default()
            },
            SessionConfig {
                up_next_countdown_ms: 0,
                ..Default::default()
            },
            SessionConfig {
                up_next_countdown_ms: 60_001,
                ..Default::default()
            },
        ] {
            assert!(PlaybackSession::new(bad, "a", 0).is_err(), "{bad:?}");
        }
    }

    #[test]
    fn no_save_before_load() {
        let mut s = PlaybackSession::new(SessionConfig::default(), "a", 0).unwrap();
        assert!(s.on_background(1_000).is_empty());
        assert!(s.tick(20_000).is_empty());
        assert!(s.on_exit().is_empty());
    }

    #[test]
    fn serde_shapes() {
        let a = SessionAction::UpNext(UpNext::Countdown {
            item_id: "e2".into(),
            remaining_ms: 3_000,
        });
        let json = serde_json::to_string(&a).unwrap();
        assert_eq!(
            json,
            r#"{"action":"up_next","state":"countdown","item_id":"e2","remaining_ms":3000}"#
        );
    }
}
