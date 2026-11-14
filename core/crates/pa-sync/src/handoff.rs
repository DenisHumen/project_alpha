// SPDX-License-Identifier: MPL-2.0
//! Instant resume between devices (SYNC-003, `docs/api/sync-protocol.md`
//! §7): the rules for the ephemeral playback session, without I/O.
//!
//! A playing device sends `pb.update` every [`HEARTBEAT_INTERVAL_MS`] and at
//! once on pause, seek, stop and going to the background; the update lives
//! [`PLAYING_TTL_MS`] while playing (three missed heartbeats) and
//! [`PAUSED_TTL_MS`] otherwise. It never enters the outbox or a journal:
//! durable progress is still the `progress.upsert` op.
//!
//! A device that starts a title takes the newest [`ResumePoint`] by HLC of
//! its stored progress (which already holds the `progress.upsert` ops of
//! the others) and the last `pb.update` of another device, after a `pull`
//! of at most [`PULL_TIMEOUT_MS`]. A `playing` update moves on with time,
//! so a heartbeat up to 5 s old still gives the position within a second.

use crate::hlc::Hlc;
use crate::proto::{PlayState, PlaybackUpdate};
use pa_model::PlaybackItem;

/// A playing device sends `pb.update` this often.
pub const HEARTBEAT_INTERVAL_MS: u64 = 5_000;

/// A `playing` update is current this long: three heartbeats.
pub const PLAYING_TTL_MS: u64 = 3 * HEARTBEAT_INTERVAL_MS;

/// A paused or stopped update is kept this long (the replay time of relays).
pub const PAUSED_TTL_MS: u64 = 600_000;

/// TTL of an update without `ttl_ms` (older senders).
pub const DEFAULT_TTL_MS: u64 = PLAYING_TTL_MS;

/// How long a device waits for its `pull` before it starts playing
/// (500–800 ms by the protocol).
pub const PULL_TIMEOUT_MS: u64 = 600;

/// A newer position this far from the one playing is offered to the user
/// ("Jump to 41:11?") instead of being ignored.
pub const OFFER_MIN_GAP_MS: u64 = 5_000;

/// The `ttl_ms` a device puts into its update in `state`.
pub fn ttl_for(state: PlayState) -> u64 {
    match state {
        PlayState::Playing => PLAYING_TTL_MS,
        _ => PAUSED_TTL_MS,
    }
}

/// Whether `update` is still current at `now_ms` (Unix ms on this device).
/// Clocks of LAN devices are assumed to be within a few seconds.
pub fn is_current(update: &PlaybackUpdate, now_ms: u64) -> bool {
    let ttl = update.ttl_ms.unwrap_or(DEFAULT_TTL_MS);
    now_ms < update.hlc.wall_ms.saturating_add(ttl)
}

/// Whether another device plays `update`'s title right now (for the
/// "Continue on this device" banner).
pub fn is_playing_now(update: &PlaybackUpdate, now_ms: u64) -> bool {
    update.state == PlayState::Playing && is_current(update, now_ms)
}

/// Where a title was left, by whom and when.
#[derive(Debug, Clone, PartialEq)]
pub struct ResumePoint {
    pub position_ms: u64,
    pub duration_ms: Option<u64>,
    /// Order of the points: last writer wins.
    pub stamp: Hlc,
    /// State of the device that sent it; `None` for stored progress.
    pub state: Option<PlayState>,
    pub rate: f64,
    /// How long a `playing` point keeps moving on.
    pub ttl_ms: u64,
}

impl ResumePoint {
    /// Stored progress (the last change on any device, `progress.upsert`
    /// included). Its stamp is the change time and device.
    pub fn from_progress(p: &PlaybackItem) -> Self {
        Self {
            position_ms: p.position_ms,
            duration_ms: p.duration_ms,
            stamp: Hlc {
                wall_ms: u64::try_from(p.updated_at_ms).unwrap_or(0),
                counter: 0,
                device: p.device_id.clone(),
            },
            state: None,
            rate: 1.0,
            ttl_ms: 0,
        }
    }

    /// A `pb.update` of another device.
    pub fn from_update(u: &PlaybackUpdate) -> Self {
        Self {
            position_ms: u.position_ms,
            duration_ms: u.duration_ms,
            stamp: u.hlc.clone(),
            state: Some(u.state),
            rate: if u.rate.is_finite() && u.rate > 0.0 {
                u.rate
            } else {
                1.0
            },
            ttl_ms: u.ttl_ms.unwrap_or(DEFAULT_TTL_MS),
        }
    }

    /// The device that set this point.
    pub fn device(&self) -> &str {
        &self.stamp.device
    }

    /// The position at `now_ms`: a point that was playing moved on with its
    /// rate since it was sampled (at most for its TTL, at most to the end).
    pub fn position_at(&self, now_ms: u64) -> u64 {
        if self.state != Some(PlayState::Playing) {
            return self.position_ms;
        }
        let elapsed = now_ms.saturating_sub(self.stamp.wall_ms).min(self.ttl_ms);
        let moved = (elapsed as f64 * self.rate).round() as u64;
        let pos = self.position_ms.saturating_add(moved);
        match self.duration_ms {
            Some(d) if d > 0 => pos.min(d),
            _ => pos,
        }
    }
}

/// The newest point by HLC (last writer wins), if any.
pub fn newest(points: impl IntoIterator<Item = ResumePoint>) -> Option<ResumePoint> {
    points.into_iter().max_by(|a, b| a.stamp.cmp(&b.stamp))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn update(state: PlayState, pos: u64, wall_ms: u64, device: &str) -> PlaybackUpdate {
        PlaybackUpdate {
            profile: "default".into(),
            key: "tmdb:movie:603".parse().unwrap(),
            position_ms: pos,
            duration_ms: Some(8_160_000),
            rate: 1.0,
            state,
            hlc: Hlc {
                wall_ms,
                counter: 0,
                device: device.into(),
            },
            ttl_ms: Some(ttl_for(state)),
        }
    }

    #[test]
    fn playing_point_moves_on_until_its_ttl() {
        let p = ResumePoint::from_update(&update(PlayState::Playing, 60_000, 1_000, "tv"));
        assert_eq!(p.position_at(1_000), 60_000);
        assert_eq!(p.position_at(5_000), 64_000);
        assert_eq!(p.position_at(1_000 + 60_000), 60_000 + PLAYING_TTL_MS);
        let paused = ResumePoint::from_update(&update(PlayState::Paused, 60_000, 1_000, "tv"));
        assert_eq!(paused.position_at(50_000), 60_000);
    }

    #[test]
    fn newest_wins_by_hlc() {
        let stored = ResumePoint::from_progress(&PlaybackItem {
            media_key: "tmdb:movie:603".parse().unwrap(),
            source_type: pa_model::MediaKind::Movie,
            position_ms: 10_000,
            duration_ms: None,
            updated_at_ms: 2_000,
            device_id: "phone".into(),
            watched: false,
        });
        let older = ResumePoint::from_update(&update(PlayState::Paused, 99_000, 1_999, "tv"));
        let newer = ResumePoint::from_update(&update(PlayState::Paused, 5_000, 2_001, "tv"));
        assert_eq!(newest([stored.clone(), older]).unwrap(), stored);
        assert_eq!(newest([stored, newer.clone()]).unwrap(), newer);
        assert!(newest([]).is_none());
    }

    #[test]
    fn playing_now_needs_a_fresh_playing_update() {
        let u = update(PlayState::Playing, 0, 10_000, "tv");
        assert!(is_playing_now(&u, 10_000 + PLAYING_TTL_MS - 1));
        assert!(!is_playing_now(&u, 10_000 + PLAYING_TTL_MS));
        let p = update(PlayState::Paused, 0, 10_000, "tv");
        assert!(is_current(&p, 10_000 + PLAYING_TTL_MS));
        assert!(!is_playing_now(&p, 10_000));
        let old = PlaybackUpdate { ttl_ms: None, ..u };
        assert!(!is_current(&old, 10_000 + DEFAULT_TTL_MS));
    }
}
