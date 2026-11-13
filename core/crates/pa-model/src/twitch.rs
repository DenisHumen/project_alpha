// SPDX-License-Identifier: MPL-2.0
//! Twitch entities (docs/research/06 §5.1). Twitch ids are kept as strings.

use serde::{Deserialize, Serialize};

use crate::key::MediaKey;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum BroadcasterType {
    Partner,
    Affiliate,
    #[default]
    Normal,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Streamer {
    pub user_id: String,
    pub login: String,
    pub display_name: String,
    #[serde(default)]
    pub broadcaster_type: BroadcasterType,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub description: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub avatar_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offline_banner_url: Option<String>,
    /// Unix milliseconds.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_seen_live_at_ms: Option<i64>,
    /// Unix milliseconds.
    pub updated_at_ms: i64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoType {
    Archive,
    Highlight,
    Upload,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum VideoAvailability {
    Online,
    Deleted,
    #[default]
    Unknown,
}

/// Twitch VOD (archive, highlight or upload).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Video {
    pub video_id: String,
    pub user_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub stream_id: Option<String>,
    pub video_type: VideoType,
    pub title: String,
    /// Unix milliseconds.
    pub published_at_ms: i64,
    pub duration_ms: u64,
    #[serde(default)]
    pub view_count: u64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub thumbnail_url: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub is_sub_only: Option<bool>,
    #[serde(default)]
    pub availability: VideoAvailability,
}

impl Video {
    /// `twitch:vod:<video_id>`: the same key online and for the downloaded copy.
    pub fn media_key(&self) -> MediaKey {
        MediaKey::TwitchVod {
            video_id: self.video_id.clone(),
        }
    }
}
