// SPDX-License-Identifier: MPL-2.0
//! Music entities: artists, albums and tracks (MusicBrainz-oriented).

use serde::{Deserialize, Serialize};

use crate::key::MediaKey;
use crate::video::RowId;

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artist {
    pub id: RowId,
    pub name: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sort_name: Option<String>,
    /// MusicBrainz artist id, lowercase UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Album {
    pub id: RowId,
    pub title: String,
    #[serde(default)]
    pub artist_ids: Vec<RowId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub year: Option<u16>,
    /// MusicBrainz release id, lowercase UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub mbid: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Track {
    pub id: RowId,
    pub title: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub album_id: Option<RowId>,
    #[serde(default)]
    pub artist_ids: Vec<RowId>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub disc_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub track_number: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub duration_ms: Option<u64>,
    /// MusicBrainz recording id, lowercase UUID.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub recording_mbid: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub file_id: Option<RowId>,
}

impl Track {
    /// Sync key from the MusicBrainz recording id, if it is valid.
    pub fn media_key(&self) -> Option<MediaKey> {
        let key = MediaKey::MbRecording {
            mbid: self.recording_mbid.clone()?,
        };
        key.is_valid().then_some(key)
    }
}
