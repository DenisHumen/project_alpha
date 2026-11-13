// SPDX-License-Identifier: MPL-2.0
//! Cross-domain items: [`MediaItem`] (a card on a shelf) and [`PlaybackItem`]
//! (an entry of the shared "Continue watching" row, docs/research/01 §2.7).

use serde::{Deserialize, Serialize};

use crate::key::MediaKey;
use crate::video::RowId;

/// What kind of thing an item is; drives badges and filters in the UI.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum MediaKind {
    Movie,
    Episode,
    Track,
    TwitchVod,
    TwitchRecording,
    TwitchClip,
    /// Unrecognised video file.
    Video,
}

/// A playable item of any domain, as shown on shelves and in search.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct MediaItem {
    pub key: MediaKey,
    pub kind: MediaKind,
    pub title: String,
    /// Second line: show and episode, streamer and category, artist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub subtitle: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub work_id: Option<RowId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub episode_id: Option<RowId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_id: Option<RowId>,
    /// Local files with this content (copies on different sources).
    #[serde(default)]
    pub file_ids: Vec<RowId>,
}

/// Lower bound of the "in progress" window, fraction of the duration.
pub const IN_PROGRESS_MIN: f64 = 0.02;
/// Upper bound of the "in progress" window; beyond it an item counts as watched.
pub const IN_PROGRESS_MAX: f64 = 0.95;

/// Playback state of one item on one profile: the unit of "Continue watching"
/// and of position sync. Merging (HLC, LWW) belongs to pa-sync.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct PlaybackItem {
    pub media_key: MediaKey,
    pub source_type: MediaKind,
    pub position_ms: u64,
    /// Unknown until the player or metadata reports it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// Unix milliseconds of the last change on any device.
    pub updated_at_ms: i64,
    /// Device that made the last change.
    pub device_id: String,
    #[serde(default)]
    pub watched: bool,
}

impl PlaybackItem {
    /// Position as a fraction of the duration, clamped to `0.0..=1.0`.
    pub fn progress(&self) -> Option<f64> {
        let duration = self.duration_ms.filter(|d| *d > 0)?;
        Some((self.position_ms as f64 / duration as f64).min(1.0))
    }

    /// Whether the item belongs in the "Continue watching" row.
    pub fn is_in_progress(&self) -> bool {
        !self.watched
            && self
                .progress()
                .is_some_and(|p| (IN_PROGRESS_MIN..=IN_PROGRESS_MAX).contains(&p))
    }
}
