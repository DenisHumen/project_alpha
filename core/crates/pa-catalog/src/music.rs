// SPDX-License-Identifier: MPL-2.0
//! Music views for the music UI (MUS-004, docs/research/01 §4.11): tracks
//! with what the album, artist, genre, composer and folder views and the
//! Hi-Res / Lossless / downloaded chips need, play counts for the music
//! Home, and audio playlists.
//!
//! The app groups the tracks itself (albums, artists, genres, composers,
//! folders): one query gives every view, and the same grouping works on
//! the v0 library, which has only plain tracks.

use std::collections::HashMap;

use pa_library::lists::{self, Entry, PlaylistKind};
use pa_library::music as m;
use pa_library::rusqlite::{Connection, Row};

use crate::items::{self, Item, ItemRef};
use crate::lists::{PlaylistInfo, items_of};
use crate::progress::{PROFILE, key_of};
use crate::{Catalog, Error, now_ms};

/// A music track with its album, credits, stream format and plays.
#[derive(Debug, Clone, PartialEq)]
pub struct MusicTrack {
    pub item: Item,
    /// `music_albums.id`.
    pub album_id: Option<i64>,
    /// Album artists, main first (compilations: "Various Artists").
    pub album_artists: Vec<String>,
    /// Year of the album (the track's year is [`Item::year`]).
    pub album_year: Option<i64>,
    pub compilation: bool,
    /// Track artists, main first.
    pub artists: Vec<String>,
    pub composers: Vec<String>,
    pub genres: Vec<String>,
    pub disc_number: Option<i64>,
    pub disc_total: Option<i64>,
    /// `flac`, `alac`, `mp3`, `aac`, `opus`, `pcm`, `dsd`…
    pub codec: Option<String>,
    pub lossless: Option<bool>,
    pub sample_rate_hz: Option<i64>,
    pub bit_depth: Option<i64>,
    pub channels: Option<i64>,
    pub bitrate_kbps: Option<i64>,
    /// The file is on this device (a `file://` source): playable offline.
    pub local: bool,
    pub play_count: i64,
    pub last_played_at_ms: Option<i64>,
}

/// Names joined by the unit separator in SQL.
fn names(v: Option<String>) -> Vec<String> {
    v.map(|s| s.split('\u{1f}').map(str::to_owned).collect())
        .unwrap_or_default()
}

/// Ordered names of a link table, joined by the unit separator.
macro_rules! joined {
    ($select:literal) => {
        concat!("(SELECT group_concat(n, char(31)) FROM (", $select, "))")
    };
}

const EXTRA_SQL: &str = concat!(
    "
SELECT t.id, t.album_id, a.year, ifnull(a.compilation, 0), t.disc_number, t.disc_total,
       t.codec, t.lossless, t.sample_rate_hz, t.bit_depth, t.channels, t.bitrate_kbps,
       s.uri LIKE 'file:%', ifnull(p.play_count, 0), p.last_played_at_ms, ",
    joined!(
        "SELECT ar.name AS n FROM music_album_artists x JOIN music_artists ar ON ar.id = x.artist_id
         WHERE x.album_id = t.album_id ORDER BY x.position"
    ),
    ", ",
    joined!(
        "SELECT ar.name AS n FROM music_track_artists x JOIN music_artists ar ON ar.id = x.artist_id
         WHERE x.track_id = t.id AND x.role = 'artist' ORDER BY x.position"
    ),
    ", ",
    joined!(
        "SELECT ar.name AS n FROM music_track_artists x JOIN music_artists ar ON ar.id = x.artist_id
         WHERE x.track_id = t.id AND x.role = 'composer' ORDER BY x.position"
    ),
    ", ",
    joined!(
        "SELECT g.name AS n FROM music_track_genres x JOIN music_genres g ON g.id = x.genre_id
         WHERE x.track_id = t.id ORDER BY x.position"
    ),
    "
FROM music_tracks t
JOIN files f ON f.id = t.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
LEFT JOIN music_albums a ON a.id = t.album_id
LEFT JOIN music_plays p ON p.profile_id = ?1 AND p.media_key = 'file:' || f.fingerprint"
);

/// The music fields of a row of [`EXTRA_SQL`]; `item` is filled in later.
fn extra_row(r: &Row<'_>, item: Item) -> pa_library::rusqlite::Result<MusicTrack> {
    Ok(MusicTrack {
        item,
        album_id: r.get(1)?,
        album_year: r.get(2)?,
        compilation: r.get::<_, i64>(3)? != 0,
        disc_number: r.get(4)?,
        disc_total: r.get(5)?,
        codec: r.get(6)?,
        lossless: r.get::<_, Option<i64>>(7)?.map(|v| v != 0),
        sample_rate_hz: r.get(8)?,
        bit_depth: r.get(9)?,
        channels: r.get(10)?,
        bitrate_kbps: r.get(11)?,
        local: r.get(12)?,
        play_count: r.get(13)?,
        last_played_at_ms: r.get(14)?,
        album_artists: names(r.get(15)?),
        artists: names(r.get(16)?),
        composers: names(r.get(17)?),
        genres: names(r.get(18)?),
    })
}

fn music_tracks(conn: &Connection) -> Result<Vec<MusicTrack>, Error> {
    // The order of `items::tracks`: artist, album, disc, number.
    let tracks = items::tracks(conn)?;
    let order: Vec<i64> = tracks
        .iter()
        .filter_map(|i| match i.id {
            ItemRef::Track(t) => Some(t),
            _ => None,
        })
        .collect();
    let mut items: HashMap<i64, Item> = order.iter().copied().zip(tracks).collect();
    let mut extra: HashMap<i64, MusicTrack> = HashMap::with_capacity(items.len());
    let mut stmt = conn.prepare_cached(EXTRA_SQL)?;
    let mut rows = stmt.query([PROFILE])?;
    while let Some(r) = rows.next()? {
        let id: i64 = r.get(0)?;
        if let Some(item) = items.remove(&id) {
            extra.insert(id, extra_row(r, item)?);
        }
    }
    Ok(order.into_iter().filter_map(|t| extra.remove(&t)).collect())
}

fn require_track(item: ItemRef) -> Result<(), Error> {
    match item {
        ItemRef::Track(_) => Ok(()),
        other => Err(Error::Invalid(format!("not a track: {other}"))),
    }
}

fn require_audio_playlist(conn: &Connection, id: i64) -> Result<(), Error> {
    match lists::playlist(conn, id)? {
        Some(p) if p.profile_id == PROFILE && p.kind == PlaylistKind::Audio => Ok(()),
        _ => Err(Error::NotFound(format!("playlist {id}"))),
    }
}

fn track_key(conn: &Connection, item: ItemRef) -> Result<String, Error> {
    require_track(item)?;
    key_of(conn, item)?
        .map(|(k, _)| k.to_string())
        .ok_or_else(|| Error::Invalid(format!("{item} has no stable key yet (not hashed)")))
}

impl Catalog {
    /// Present music tracks with album, credits, format and plays, by
    /// artist, album, disc and track number.
    pub fn music_tracks(&self) -> Result<Vec<MusicTrack>, Error> {
        music_tracks(&self.lock())
    }

    /// Counts a play of a track (it started in the player). Returns the new
    /// play count; `0` if the track has no stable key yet (not hashed).
    pub fn record_track_play(&self, item: ItemRef) -> Result<i64, Error> {
        require_track(item)?;
        let conn = self.lock();
        match key_of(&conn, item)? {
            Some((key, _)) => Ok(m::record_play(&conn, PROFILE, &key.to_string(), now_ms())?),
            None => Ok(0),
        }
    }

    /// Audio playlists, most recently changed first.
    pub fn music_playlists(&self) -> Result<Vec<PlaylistInfo>, Error> {
        Ok(
            lists::playlists(&self.lock(), PROFILE, Some(PlaylistKind::Audio))?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    /// Creates an empty audio playlist.
    pub fn create_music_playlist(&self, name: &str) -> Result<PlaylistInfo, Error> {
        Ok(
            lists::create_playlist(&self.lock(), PROFILE, name, PlaylistKind::Audio, now_ms())?
                .into(),
        )
    }

    pub fn rename_music_playlist(&self, id: i64, name: &str) -> Result<(), Error> {
        let conn = self.lock();
        require_audio_playlist(&conn, id)?;
        lists::rename_playlist(&conn, id, name, now_ms())?;
        Ok(())
    }

    pub fn delete_music_playlist(&self, id: i64) -> Result<(), Error> {
        let conn = self.lock();
        require_audio_playlist(&conn, id)?;
        lists::delete_playlist(&conn, id)?;
        Ok(())
    }

    /// Present tracks of an audio playlist in order.
    pub fn music_playlist_items(&self, id: i64) -> Result<Vec<Item>, Error> {
        let conn = self.lock();
        require_audio_playlist(&conn, id)?;
        let entries: Vec<Entry> = lists::playlist_items(&conn, id)?
            .into_iter()
            .map(|i| Entry::Media(i.media_key))
            .collect();
        items_of(&conn, &entries)
    }

    /// Appends a track; `false` if it already was in the playlist.
    pub fn add_to_music_playlist(&self, id: i64, item: ItemRef) -> Result<bool, Error> {
        let conn = self.lock();
        require_audio_playlist(&conn, id)?;
        let key = track_key(&conn, item)?;
        Ok(lists::add_to_playlist(&conn, id, &key, now_ms())?)
    }

    /// Removes a track; `false` if it was not in the playlist.
    pub fn remove_from_music_playlist(&self, id: i64, item: ItemRef) -> Result<bool, Error> {
        let conn = self.lock();
        require_audio_playlist(&conn, id)?;
        let key = track_key(&conn, item)?;
        Ok(lists::remove_from_playlist(&conn, id, &key, now_ms())?)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn names_split_on_the_unit_separator() {
        assert_eq!(names(None), Vec::<String>::new());
        assert_eq!(names(Some("A".into())), ["A"]);
        assert_eq!(names(Some("A\u{1f}B, C".into())), ["A", "B, C"]);
    }
}
