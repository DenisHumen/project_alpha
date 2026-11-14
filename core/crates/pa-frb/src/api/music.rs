// SPDX-License-Identifier: MPL-2.0
//! Types of the music views (MUS-004) for Dart. The calls are methods of
//! [`CoreLibrary`](crate::api::library::CoreLibrary): `music_tracks`,
//! `record_track_play` and the audio playlists.

use pa_catalog as c;

use crate::api::library::CoreItem;

/// A music track with its album, credits, stream format and plays.
#[derive(Debug, Clone, PartialEq)]
pub struct CoreMusicTrack {
    pub item: CoreItem,
    /// `music_albums.id`.
    pub album_id: Option<i64>,
    /// Album artists, main first (compilations: "Various Artists").
    pub album_artists: Vec<String>,
    pub album_year: Option<i32>,
    pub compilation: bool,
    /// Track artists, main first.
    pub artists: Vec<String>,
    pub composers: Vec<String>,
    pub genres: Vec<String>,
    pub disc_number: Option<i32>,
    pub disc_total: Option<i32>,
    /// `flac`, `alac`, `mp3`, `aac`, `opus`, `pcm`, `dsd`…
    pub codec: Option<String>,
    pub lossless: Option<bool>,
    pub sample_rate_hz: Option<i32>,
    pub bit_depth: Option<i32>,
    pub channels: Option<i32>,
    pub bitrate_kbps: Option<i32>,
    /// The file is on this device (a local folder): playable offline.
    pub local: bool,
    pub play_count: i64,
    pub last_played_at_ms: Option<i64>,
}

fn small(v: Option<i64>) -> Option<i32> {
    v.and_then(|v| i32::try_from(v).ok())
}

impl From<c::MusicTrack> for CoreMusicTrack {
    fn from(t: c::MusicTrack) -> Self {
        Self {
            item: t.item.into(),
            album_id: t.album_id,
            album_artists: t.album_artists,
            album_year: small(t.album_year),
            compilation: t.compilation,
            artists: t.artists,
            composers: t.composers,
            genres: t.genres,
            disc_number: small(t.disc_number),
            disc_total: small(t.disc_total),
            codec: t.codec,
            lossless: t.lossless,
            sample_rate_hz: small(t.sample_rate_hz),
            bit_depth: small(t.bit_depth),
            channels: small(t.channels),
            bitrate_kbps: small(t.bitrate_kbps),
            local: t.local,
            play_count: t.play_count,
            last_played_at_ms: t.last_played_at_ms,
        }
    }
}
