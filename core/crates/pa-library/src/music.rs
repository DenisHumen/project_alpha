// SPDX-License-Identifier: MPL-2.0
//! Music library: artists, albums, tracks, genres (MUS-001,
//! docs/research/01 §4.11, docs/dev/db.md "Музыка").
//!
//! One audio file of `files` is one track. The scanner (`pa_scanner::index_music`)
//! reads the tags of new and changed files ([`stale_files`]), writes each
//! with [`put_track`], then calls [`refresh_album`] for every album it
//! touched, picks the cover ([`set_album_art`]) and drops rows left without
//! tracks ([`remove_orphans`]).
//!
//! Album grouping (the album key):
//!
//! 1. a MusicBrainz release id groups by release;
//! 2. else an album artist tag (or the compilation flag, as
//!    [`VARIOUS_ARTISTS`]) groups by album artist and title across folders
//!    and sources;
//! 3. else the album folder and title group the tracks; [`refresh_album`]
//!    then makes an album whose tracks have several artists a compilation
//!    by [`VARIOUS_ARTISTS`].
//!
//! `CD1`/`Disc 2` subfolders belong to the album folder above them.
//! Queries list only tracks whose file is present (not soft-deleted).
//!
//! ```
//! use pa_library::music::{self, TrackFile, TrackTags};
//!
//! let conn = pa_library::open_in_memory().unwrap();
//! conn.execute_batch(
//!     "INSERT INTO sources (id, kind, name, uri, created_at_ms) VALUES (1, 'local', 'Music', 'file:///m', 0);
//!      INSERT INTO files (id, source_id, path, size_bytes, added_at_ms) VALUES (7, 1, '/A/B/01.flac', 10, 0);",
//! ).unwrap();
//! let file = &music::stale_files(&conn, 1).unwrap()[0];
//! let tags = TrackTags {
//!     title: Some("Song".into()),
//!     artists: vec!["Artist".into()],
//!     album: Some("Album".into()),
//!     ..TrackTags::default()
//! };
//! let put = music::put_track(&conn, file, &tags, 1).unwrap();
//! music::refresh_album(&conn, put.album_id.unwrap(), 1).unwrap();
//! let albums = music::albums(&conn).unwrap();
//! assert_eq!(albums[0].title, "Album");
//! assert_eq!(music::artists(&conn).unwrap()[0].name, "Artist");
//! ```

use pa_model::{Album, Artist, Track};
use rusqlite::{Connection, OptionalExtension, params};

use crate::Error;

/// Album artist of compilations without one.
pub const VARIOUS_ARTISTS: &str = "Various Artists";

/// Tags and stream properties of one audio file, as the scanner read them.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct TrackTags {
    /// `None`: the file name (without extension) is used.
    pub title: Option<String>,
    /// Track artists, main artist first.
    pub artists: Vec<String>,
    pub album_artists: Vec<String>,
    pub album: Option<String>,
    pub composers: Vec<String>,
    pub genres: Vec<String>,
    pub disc_number: Option<u32>,
    pub disc_total: Option<u32>,
    pub track_number: Option<u32>,
    pub track_total: Option<u32>,
    pub year: Option<u16>,
    /// iTunes `cpil` / ID3 `TCMP` / Vorbis `COMPILATION`.
    pub compilation: bool,
    /// Sort name of the single track artist (`ARTISTSORT`).
    pub artist_sort_name: Option<String>,
    /// MusicBrainz id of the single track artist.
    pub artist_mbid: Option<String>,
    pub recording_mbid: Option<String>,
    pub release_mbid: Option<String>,
    pub release_group_mbid: Option<String>,
    pub replay_gain: ReplayGain,
    pub audio: AudioInfo,
    pub has_embedded_art: bool,
    /// The tags could not be read; the other fields come from the path.
    pub tags_failed: bool,
}

/// ReplayGain 2.0 values (reference -18 LUFS).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct ReplayGain {
    pub track_gain_db: Option<f64>,
    /// Linear sample peak, 1.0 = full scale.
    pub track_peak: Option<f64>,
    pub album_gain_db: Option<f64>,
    pub album_peak: Option<f64>,
}

/// Stream properties.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct AudioInfo {
    /// Lowercase slug: `mp3`, `aac`, `alac`, `flac`, `opus`, `vorbis`, `pcm`, `dsd`…
    pub codec: Option<String>,
    pub lossless: Option<bool>,
    pub duration_ms: Option<u64>,
    pub sample_rate_hz: Option<u32>,
    pub bit_depth: Option<u8>,
    pub channels: Option<u8>,
    pub bitrate_kbps: Option<u32>,
}

/// A row of `files` whose track is missing or older than the file.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrackFile {
    pub file_id: i64,
    pub source_id: i64,
    /// Path relative to the source root (`/Artist/Album/01.flac`).
    pub path: String,
    pub size_bytes: i64,
    pub mtime_ms: Option<i64>,
}

/// Result of [`put_track`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PutTrack {
    pub track_id: i64,
    pub album_id: Option<i64>,
    /// The album the track was in before, if another one: refresh it too.
    pub previous_album_id: Option<i64>,
}

/// Where an album cover comes from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ArtKind {
    /// The picture embedded in the track file at the path.
    Embedded,
    /// An image file in the album folder.
    Folder,
}

/// Album cover reference: kind, source and path inside the source.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumArt {
    pub kind: ArtKind,
    pub source_id: i64,
    pub path: String,
}

/// A track of an album as cover selection needs it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlbumFile {
    pub source_id: i64,
    pub path: String,
    pub has_embedded_art: bool,
}

/// A genre with the number of present tracks.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Genre {
    pub id: i64,
    pub name: String,
    pub tracks: u32,
}

/// Rows removed by [`remove_orphans`].
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Orphans {
    pub albums: usize,
    pub artists: usize,
    pub genres: usize,
}

/// Case- and space-folded name used to match artists, albums and genres.
#[must_use]
pub fn fold(name: &str) -> String {
    name.split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

/// The album folder of a track path: its parent folder, or the one above a
/// disc subfolder (`CD1`, `Disc 2`, `disk03`). The root is `/`.
#[must_use]
pub fn album_folder(path: &str) -> &str {
    let parent = parent_of(path);
    if disc_folder_number(file_name(parent)).is_some() {
        parent_of(parent)
    } else {
        parent
    }
}

/// Disc number of a disc subfolder name (`CD1`, `Disc 2`, `disk03`).
#[must_use]
pub fn disc_folder_number(name: &str) -> Option<u32> {
    let lower = name.trim().to_ascii_lowercase();
    let rest = ["disc", "disk", "cd"]
        .iter()
        .find_map(|p| lower.strip_prefix(p))?;
    let digits = rest.trim_start_matches([' ', '_', '-', '.']);
    if digits.is_empty() || digits.len() > 3 || !digits.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    digits.parse().ok().filter(|n| *n > 0)
}

fn parent_of(path: &str) -> &str {
    match path.rfind('/') {
        Some(0) | None => "/",
        Some(i) => &path[..i],
    }
}

fn file_name(path: &str) -> &str {
    path.rsplit('/').next().unwrap_or(path)
}

/// Lowercase UUID, or `None` if `s` is not one.
fn mbid(s: Option<&str>) -> Option<String> {
    let s = s?.trim().to_ascii_lowercase();
    let ok = s.len() == 36
        && s.bytes().enumerate().all(|(i, b)| match i {
            8 | 13 | 18 | 23 => b == b'-',
            _ => b.is_ascii_hexdigit(),
        });
    ok.then_some(s)
}

fn clean(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

fn non_empty(names: &[String]) -> Vec<&str> {
    let mut out: Vec<&str> = Vec::new();
    for n in names.iter().map(|n| n.trim()).filter(|n| !n.is_empty()) {
        if !out.iter().any(|o| fold(o) == fold(n)) {
            out.push(n);
        }
    }
    out
}

/// Album key and title of a track, or `None` without an album title.
fn album_key(file: &TrackFile, tags: &TrackTags) -> Option<(String, String)> {
    let title = clean(tags.album.as_deref())?;
    if let Some(id) = mbid(tags.release_mbid.as_deref()) {
        return Some((format!("mb:{id}"), title.to_owned()));
    }
    let album_artists = non_empty(&tags.album_artists);
    let by = if !album_artists.is_empty() {
        Some(
            album_artists
                .iter()
                .map(|a| fold(a))
                .collect::<Vec<_>>()
                .join(";"),
        )
    } else if tags.compilation {
        Some(fold(VARIOUS_ARTISTS))
    } else {
        None
    };
    let key = match by {
        Some(by) => format!("aa:{by}|{}", fold(title)),
        None => format!(
            "dir:{}:{}|{}",
            file.source_id,
            album_folder(&file.path),
            fold(title)
        ),
    };
    Some((key, title.to_owned()))
}

/// Present files of a source without a track or changed since their tags
/// were read, by path. The caller filters audio files by extension.
pub fn stale_files(conn: &Connection, source_id: i64) -> Result<Vec<TrackFile>, Error> {
    let mut stmt = conn.prepare(
        "SELECT f.id, f.source_id, f.path, f.size_bytes, f.mtime_ms
         FROM files f LEFT JOIN music_tracks t ON t.file_id = f.id
         WHERE f.source_id = ?1 AND f.missing_since_ms IS NULL
           AND (t.id IS NULL OR t.tagged_size_bytes <> f.size_bytes
                OR t.tagged_mtime_ms IS NOT f.mtime_ms)
         ORDER BY f.path",
    )?;
    let rows = stmt.query_map([source_id], |r| {
        Ok(TrackFile {
            file_id: r.get(0)?,
            source_id: r.get(1)?,
            path: r.get(2)?,
            size_bytes: r.get(3)?,
            mtime_ms: r.get(4)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Writes (inserts or replaces) the track of `file` with its artists,
/// composers, genres and album. Run it inside a transaction: it makes
/// several statements. Call [`refresh_album`] for the returned album ids
/// once the batch is written.
pub fn put_track(
    conn: &Connection,
    file: &TrackFile,
    tags: &TrackTags,
    now_ms: i64,
) -> Result<PutTrack, Error> {
    let previous: Option<Option<i64>> = conn
        .query_row(
            "SELECT album_id FROM music_tracks WHERE file_id = ?1",
            [file.file_id],
            |r| r.get(0),
        )
        .optional()?;

    let album_id = match album_key(file, tags) {
        Some((key, title)) => Some(conn.query_row(
            "INSERT INTO music_albums (album_key, title, release_mbid, release_group_mbid,
                                       created_at_ms, updated_at_ms)
             VALUES (?1, ?2, ?3, ?4, ?5, ?5)
             ON CONFLICT (album_key) DO UPDATE SET
                 release_group_mbid = coalesce(music_albums.release_group_mbid,
                                               excluded.release_group_mbid),
                 updated_at_ms = excluded.updated_at_ms
             RETURNING id",
            params![
                key,
                title,
                mbid(tags.release_mbid.as_deref()),
                mbid(tags.release_group_mbid.as_deref()),
                now_ms
            ],
            |r| r.get::<_, i64>(0),
        )?),
        None => None,
    };

    let stem = file_name(&file.path)
        .rsplit_once('.')
        .map_or(file_name(&file.path), |(s, _)| s);
    let title = clean(tags.title.as_deref())
        .or(clean(Some(stem)))
        .unwrap_or("?");
    let album_artist_tag = non_empty(&tags.album_artists).join("; ");
    let rg = &tags.replay_gain;
    let a = &tags.audio;
    let track_id: i64 = conn.query_row(
        "INSERT INTO music_tracks (file_id, album_id, title, disc_number, disc_total, track_number,
             track_total, year, album_artist_tag, compilation_tag, recording_mbid, duration_ms, codec,
             lossless, sample_rate_hz, bit_depth, channels, bitrate_kbps, rg_track_gain_db,
             rg_track_peak, rg_album_gain_db, rg_album_peak, has_embedded_art, tags_failed,
             tagged_size_bytes, tagged_mtime_ms, tagged_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18,
                 ?19, ?20, ?21, ?22, ?23, ?24, ?25, ?26, ?27)
         ON CONFLICT (file_id) DO UPDATE SET
             album_id = excluded.album_id, title = excluded.title,
             disc_number = excluded.disc_number, disc_total = excluded.disc_total,
             track_number = excluded.track_number, track_total = excluded.track_total,
             year = excluded.year, album_artist_tag = excluded.album_artist_tag,
             compilation_tag = excluded.compilation_tag, recording_mbid = excluded.recording_mbid,
             duration_ms = excluded.duration_ms, codec = excluded.codec,
             lossless = excluded.lossless, sample_rate_hz = excluded.sample_rate_hz,
             bit_depth = excluded.bit_depth, channels = excluded.channels,
             bitrate_kbps = excluded.bitrate_kbps, rg_track_gain_db = excluded.rg_track_gain_db,
             rg_track_peak = excluded.rg_track_peak, rg_album_gain_db = excluded.rg_album_gain_db,
             rg_album_peak = excluded.rg_album_peak, has_embedded_art = excluded.has_embedded_art,
             tags_failed = excluded.tags_failed, tagged_size_bytes = excluded.tagged_size_bytes,
             tagged_mtime_ms = excluded.tagged_mtime_ms, tagged_at_ms = excluded.tagged_at_ms
         RETURNING id",
        params![
            file.file_id,
            album_id,
            title,
            tags.disc_number.filter(|n| *n > 0),
            tags.disc_total.filter(|n| *n > 0),
            tags.track_number.filter(|n| *n > 0),
            tags.track_total.filter(|n| *n > 0),
            tags.year.filter(|y| (1000..=3000).contains(y)),
            (!album_artist_tag.is_empty()).then_some(album_artist_tag),
            tags.compilation,
            mbid(tags.recording_mbid.as_deref()),
            a.duration_ms.map(|d| i64::try_from(d).unwrap_or(i64::MAX)),
            a.codec.as_deref(),
            a.lossless,
            a.sample_rate_hz.filter(|n| *n > 0),
            a.bit_depth.filter(|n| *n > 0),
            a.channels.filter(|n| *n > 0),
            a.bitrate_kbps.filter(|n| *n > 0),
            rg.track_gain_db.filter(|g| g.is_finite()),
            rg.track_peak.filter(|p| p.is_finite() && *p >= 0.0),
            rg.album_gain_db.filter(|g| g.is_finite()),
            rg.album_peak.filter(|p| p.is_finite() && *p >= 0.0),
            tags.has_embedded_art,
            tags.tags_failed,
            file.size_bytes,
            file.mtime_ms,
            now_ms
        ],
        |r| r.get(0),
    )?;

    conn.execute(
        "DELETE FROM music_track_artists WHERE track_id = ?1",
        [track_id],
    )?;
    let artists = non_empty(&tags.artists);
    let single = artists.len() == 1;
    for (role, names) in [
        ("artist", artists),
        ("composer", non_empty(&tags.composers)),
    ] {
        for (pos, name) in (0_i64..).zip(&names) {
            let (sort, id) = if single && role == "artist" {
                (
                    clean(tags.artist_sort_name.as_deref()),
                    mbid(tags.artist_mbid.as_deref()),
                )
            } else {
                (None, None)
            };
            let artist_id = ensure_artist(conn, name, sort, id.as_deref(), now_ms)?;
            conn.execute(
                "INSERT OR IGNORE INTO music_track_artists (track_id, artist_id, role, position)
                 VALUES (?1, ?2, ?3, ?4)",
                params![track_id, artist_id, role, pos],
            )?;
        }
    }

    conn.execute(
        "DELETE FROM music_track_genres WHERE track_id = ?1",
        [track_id],
    )?;
    for (pos, name) in (0_i64..).zip(non_empty(&tags.genres)) {
        let genre_id: i64 = conn.query_row(
            "INSERT INTO music_genres (name, name_key) VALUES (?1, ?2)
             ON CONFLICT (name_key) DO UPDATE SET name = music_genres.name
             RETURNING id",
            params![name, fold(name)],
            |r| r.get(0),
        )?;
        conn.execute(
            "INSERT OR IGNORE INTO music_track_genres (track_id, genre_id, position) VALUES (?1, ?2, ?3)",
            params![track_id, genre_id, pos],
        )?;
    }

    Ok(PutTrack {
        track_id,
        album_id,
        previous_album_id: previous.flatten().filter(|p| Some(*p) != album_id),
    })
}

fn ensure_artist(
    conn: &Connection,
    name: &str,
    sort_name: Option<&str>,
    mbid: Option<&str>,
    now_ms: i64,
) -> Result<i64, Error> {
    Ok(conn.query_row(
        "INSERT INTO music_artists (name, sort_name, name_key, mbid, created_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (name_key) DO UPDATE SET
             sort_name = coalesce(music_artists.sort_name, excluded.sort_name),
             mbid = coalesce(music_artists.mbid, excluded.mbid)
         RETURNING id",
        params![name, sort_name, fold(name), mbid, now_ms],
        |r| r.get(0),
    )?)
}

/// Recomputes an album from its tracks: album artists, compilation flag,
/// year (the most common one) and disc count. Returns `false` if the album
/// has no tracks (left for [`remove_orphans`]) or does not exist.
///
/// Album artists: the most common album artist tag; else
/// [`VARIOUS_ARTISTS`] if a track has the compilation flag; else the main
/// artist if all tracks share it, [`VARIOUS_ARTISTS`] (a compilation) if
/// they do not.
pub fn refresh_album(conn: &Connection, album_id: i64, now_ms: i64) -> Result<bool, Error> {
    let (tracks, compilation_tags): (i64, i64) = conn.query_row(
        "SELECT count(*), coalesce(max(compilation_tag), 0) FROM music_tracks WHERE album_id = ?1",
        [album_id],
        |r| Ok((r.get(0)?, r.get(1)?)),
    )?;
    if tracks == 0 {
        return Ok(false);
    }
    let album_artist_tag: Option<String> = conn
        .query_row(
            "SELECT album_artist_tag FROM music_tracks
             WHERE album_id = ?1 AND album_artist_tag IS NOT NULL
             GROUP BY album_artist_tag ORDER BY count(*) DESC, album_artist_tag LIMIT 1",
            [album_id],
            |r| r.get(0),
        )
        .optional()?;

    let mut compilation = compilation_tags != 0;
    let artist_ids: Vec<i64> = if let Some(tag) = album_artist_tag {
        let mut ids = Vec::new();
        for name in tag.split(';').map(str::trim).filter(|n| !n.is_empty()) {
            compilation |= fold(name) == fold(VARIOUS_ARTISTS);
            ids.push(ensure_artist(conn, name, None, None, now_ms)?);
        }
        ids
    } else if compilation {
        vec![ensure_artist(conn, VARIOUS_ARTISTS, None, None, now_ms)?]
    } else {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT ta.artist_id FROM music_track_artists ta
             JOIN music_tracks t ON t.id = ta.track_id
             WHERE t.album_id = ?1 AND ta.role = 'artist' AND ta.position = 0
             LIMIT 2",
        )?;
        let main: Vec<i64> = stmt
            .query_map([album_id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
        if main.len() > 1 {
            compilation = true;
            vec![ensure_artist(conn, VARIOUS_ARTISTS, None, None, now_ms)?]
        } else {
            main
        }
    };

    conn.execute(
        "DELETE FROM music_album_artists WHERE album_id = ?1",
        [album_id],
    )?;
    for (pos, artist_id) in (0_i64..).zip(&artist_ids) {
        conn.execute(
            "INSERT OR IGNORE INTO music_album_artists (album_id, artist_id, position) VALUES (?1, ?2, ?3)",
            params![album_id, artist_id, pos],
        )?;
    }
    let changed = conn.execute(
        "UPDATE music_albums SET
             compilation = ?2,
             year = (SELECT year FROM music_tracks WHERE album_id = ?1 AND year IS NOT NULL
                     GROUP BY year ORDER BY count(*) DESC, year LIMIT 1),
             disc_count = (SELECT nullif(max(coalesce(max(disc_total), 0), coalesce(max(disc_number), 0)), 0)
                           FROM music_tracks WHERE album_id = ?1),
             updated_at_ms = ?3
         WHERE id = ?1",
        params![album_id, compilation, now_ms],
    )?;
    Ok(changed == 1)
}

/// Tracks of an album, by disc and track number, for cover selection.
pub fn album_files(conn: &Connection, album_id: i64) -> Result<Vec<AlbumFile>, Error> {
    let mut stmt = conn.prepare(
        "SELECT f.source_id, f.path, t.has_embedded_art
         FROM music_tracks t JOIN files f ON f.id = t.file_id
         WHERE t.album_id = ?1 AND f.missing_since_ms IS NULL
         ORDER BY t.disc_number, t.track_number, f.path",
    )?;
    let rows = stmt.query_map([album_id], |r| {
        Ok(AlbumFile {
            source_id: r.get(0)?,
            path: r.get(1)?,
            has_embedded_art: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Sets (or clears) the cover of an album.
pub fn set_album_art(
    conn: &Connection,
    album_id: i64,
    art: Option<&AlbumArt>,
) -> Result<(), Error> {
    let kind = art.map(|a| match a.kind {
        ArtKind::Embedded => "embedded",
        ArtKind::Folder => "folder",
    });
    conn.execute(
        "UPDATE music_albums SET art_kind = ?2, art_source_id = ?3, art_path = ?4 WHERE id = ?1",
        params![
            album_id,
            kind,
            art.map(|a| a.source_id),
            art.map(|a| a.path.as_str())
        ],
    )?;
    Ok(())
}

/// The cover of an album, if any.
pub fn album_art(conn: &Connection, album_id: i64) -> Result<Option<AlbumArt>, Error> {
    let row: Option<(Option<String>, Option<i64>, Option<String>)> = conn
        .query_row(
            "SELECT art_kind, art_source_id, art_path FROM music_albums WHERE id = ?1",
            [album_id],
            |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
        )
        .optional()?;
    Ok(match row {
        Some((Some(kind), Some(source_id), Some(path))) => Some(AlbumArt {
            kind: if kind == "folder" {
                ArtKind::Folder
            } else {
                ArtKind::Embedded
            },
            source_id,
            path,
        }),
        _ => None,
    })
}

/// Deletes albums without tracks, then artists without tracks or albums and
/// genres without tracks.
pub fn remove_orphans(conn: &Connection) -> Result<Orphans, Error> {
    let albums = conn.execute(
        "DELETE FROM music_albums
         WHERE NOT EXISTS (SELECT 1 FROM music_tracks t WHERE t.album_id = music_albums.id)",
        [],
    )?;
    let artists = conn.execute(
        "DELETE FROM music_artists
         WHERE NOT EXISTS (SELECT 1 FROM music_track_artists x WHERE x.artist_id = music_artists.id)
           AND NOT EXISTS (SELECT 1 FROM music_album_artists x WHERE x.artist_id = music_artists.id)",
        [],
    )?;
    let genres = conn.execute(
        "DELETE FROM music_genres
         WHERE NOT EXISTS (SELECT 1 FROM music_track_genres x WHERE x.genre_id = music_genres.id)",
        [],
    )?;
    Ok(Orphans {
        albums,
        artists,
        genres,
    })
}

/// SQL condition: track `t` has a present file.
const PRESENT: &str =
    "EXISTS (SELECT 1 FROM files f WHERE f.id = t.file_id AND f.missing_since_ms IS NULL)";

/// Artists with present tracks or albums, by sort name.
pub fn artists(conn: &Connection) -> Result<Vec<Artist>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT a.id, a.name, a.sort_name, a.mbid FROM music_artists a
         WHERE EXISTS (SELECT 1 FROM music_track_artists x JOIN music_tracks t ON t.id = x.track_id
                       WHERE x.artist_id = a.id AND {PRESENT})
            OR EXISTS (SELECT 1 FROM music_album_artists x JOIN music_tracks t ON t.album_id = x.album_id
                       WHERE x.artist_id = a.id AND {PRESENT})
         ORDER BY coalesce(a.sort_name, a.name) COLLATE NOCASE, a.id"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(Artist {
            id: r.get(0)?,
            name: r.get(1)?,
            sort_name: r.get(2)?,
            mbid: r.get(3)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Albums with present tracks, by title.
pub fn albums(conn: &Connection) -> Result<Vec<Album>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT a.id, a.title, a.year, a.release_mbid FROM music_albums a
         WHERE EXISTS (SELECT 1 FROM music_tracks t WHERE t.album_id = a.id AND {PRESENT})
         ORDER BY a.title COLLATE NOCASE, a.id"
    ))?;
    let mut albums = stmt
        .query_map([], |r| {
            Ok(Album {
                id: r.get(0)?,
                title: r.get(1)?,
                artist_ids: Vec::new(),
                year: r
                    .get::<_, Option<i64>>(2)?
                    .and_then(|y| u16::try_from(y).ok()),
                mbid: r.get(3)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut artists = conn.prepare(
        "SELECT artist_id FROM music_album_artists WHERE album_id = ?1 ORDER BY position",
    )?;
    for album in &mut albums {
        album.artist_ids = artists
            .query_map([album.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    Ok(albums)
}

/// Present tracks of an album, by disc and track number.
pub fn album_tracks(conn: &Connection, album_id: i64) -> Result<Vec<Track>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT t.id, t.title, t.album_id, t.disc_number, t.track_number, t.duration_ms,
                t.recording_mbid, t.file_id
         FROM music_tracks t
         WHERE t.album_id = ?1 AND {PRESENT}
         ORDER BY coalesce(t.disc_number, 1), t.track_number, t.title COLLATE NOCASE"
    ))?;
    let mut tracks = stmt
        .query_map([album_id], |r| {
            Ok(Track {
                id: r.get(0)?,
                title: r.get(1)?,
                album_id: r.get(2)?,
                artist_ids: Vec::new(),
                disc_number: r.get(3)?,
                track_number: r.get(4)?,
                duration_ms: r
                    .get::<_, Option<i64>>(5)?
                    .and_then(|d| u64::try_from(d).ok()),
                recording_mbid: r.get(6)?,
                file_id: r.get(7)?,
            })
        })?
        .collect::<Result<Vec<_>, _>>()?;
    let mut artists = conn.prepare(
        "SELECT artist_id FROM music_track_artists
         WHERE track_id = ?1 AND role = 'artist' ORDER BY position",
    )?;
    for track in &mut tracks {
        track.artist_ids = artists
            .query_map([track.id], |r| r.get(0))?
            .collect::<Result<_, _>>()?;
    }
    Ok(tracks)
}

/// Genres with present tracks, by name.
pub fn genres(conn: &Connection) -> Result<Vec<Genre>, Error> {
    let mut stmt = conn.prepare(&format!(
        "SELECT g.id, g.name, count(*) FROM music_genres g
         JOIN music_track_genres x ON x.genre_id = g.id
         JOIN music_tracks t ON t.id = x.track_id
         WHERE {PRESENT}
         GROUP BY g.id ORDER BY g.name COLLATE NOCASE"
    ))?;
    let rows = stmt.query_map([], |r| {
        Ok(Genre {
            id: r.get(0)?,
            name: r.get(1)?,
            tracks: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn album_folder_skips_disc_subfolders() {
        assert_eq!(album_folder("/A/Album/01.flac"), "/A/Album");
        assert_eq!(album_folder("/A/Album/CD2/01.flac"), "/A/Album");
        assert_eq!(album_folder("/A/Album/Disc 1/01.flac"), "/A/Album");
        assert_eq!(album_folder("/01.flac"), "/");
        assert_eq!(album_folder("/CD1/01.flac"), "/");
        assert_eq!(disc_folder_number("disk03"), Some(3));
        assert_eq!(disc_folder_number("CD"), None);
        assert_eq!(disc_folder_number("Discovery"), None);
    }

    #[test]
    fn mbid_and_fold_normalize() {
        assert_eq!(
            mbid(Some(" B1A9C0E9-D987-4042-AE91-78D6A3267D69 ")).as_deref(),
            Some("b1a9c0e9-d987-4042-ae91-78d6a3267d69")
        );
        assert_eq!(mbid(Some("not-an-id")), None);
        assert_eq!(fold("  The   Beatles "), "the beatles");
    }
}
