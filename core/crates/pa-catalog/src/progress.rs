// SPDX-License-Identifier: MPL-2.0
//! Progress of items on the default profile, through `pa_library::progress`
//! (LIB-011): position reports, "Continue watching" with Up Next.
//!
//! Progress is stored under the stable media key of an item: `tmdb:movie:` /
//! `tmdb:episode:` when the work has a TMDB id, otherwise `file:` with the
//! fingerprint of the item's file ([`crate::fingerprint`]).

use pa_library::progress::{
    self, ContinueQuery, ContinueReason, LocalOnly, PositionReport, Thresholds,
};
use pa_library::rusqlite::{Connection, OptionalExtension, params};
use pa_model::{MediaKey, MediaKind};

use crate::items::{self, Item, ItemRef};
use crate::{Catalog, Error, LOCAL_DEVICE_ID};

/// The profile of the app until profiles arrive (LIB-012).
const PROFILE: i64 = 1;

/// Playback state of an item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemProgress {
    pub item: ItemRef,
    pub position_ms: i64,
    /// 0 if unknown.
    pub duration_ms: i64,
    pub updated_at_ms: i64,
    pub watched: bool,
}

/// One entry of "Continue watching".
#[derive(Debug, Clone, PartialEq)]
pub struct ContinueItem {
    pub item: Item,
    pub progress: ItemProgress,
    /// The next episode after a finished one (Up Next), not a started item.
    pub up_next: bool,
}

/// `file:` key of the file with this id, if it is fingerprinted.
fn file_key(conn: &Connection, file_id: i64) -> Result<Option<MediaKey>, Error> {
    let fp: Option<String> = conn
        .query_row(
            "SELECT fingerprint FROM files WHERE id = ?1",
            [file_id],
            |r| r.get(0),
        )
        .optional()?
        .flatten();
    Ok(fp.map(|fingerprint| MediaKey::File { fingerprint }))
}

/// The key progress of `item` is stored under, and its kind.
pub(crate) fn key_of(conn: &Connection, item: ItemRef) -> Result<Option<(MediaKey, MediaKind)>, Error> {
    Ok(match item {
        ItemRef::Movie(work) => {
            let tmdb: Option<String> = conn
                .query_row(
                    "SELECT ext_id FROM external_ids
                     WHERE work_id = ?1 AND provider = 'tmdb' AND ext_kind = 'movie'",
                    [work],
                    |r| r.get(0),
                )
                .optional()?;
            if let Some(id) = tmdb.and_then(|t| t.parse().ok()) {
                Some((MediaKey::TmdbMovie { id }, MediaKind::Movie))
            } else {
                let file: Option<i64> = conn
                    .query_row(
                        "SELECT f.id FROM matches m JOIN files f ON f.id = m.file_id
                         WHERE m.work_id = ?1 AND m.state <> 'rejected' AND f.fingerprint IS NOT NULL
                         ORDER BY f.missing_since_ms IS NOT NULL, f.id LIMIT 1",
                        [work],
                        |r| r.get(0),
                    )
                    .optional()?;
                match file {
                    Some(f) => file_key(conn, f)?.map(|k| (k, MediaKind::Movie)),
                    None => None,
                }
            }
        }
        ItemRef::Episode(ep) => progress::episode_keys(conn, ep)?
            .into_iter()
            .next()
            .map(|k| (k, MediaKind::Episode)),
        ItemRef::Track(track) => {
            let file: Option<i64> = conn
                .query_row(
                    "SELECT file_id FROM music_tracks WHERE id = ?1",
                    [track],
                    |r| r.get(0),
                )
                .optional()?;
            match file {
                Some(f) => file_key(conn, f)?.map(|k| (k, MediaKind::Track)),
                None => None,
            }
        }
        ItemRef::Show(_) => None,
    })
}

/// The item a stored key belongs to.
fn item_of_key(conn: &Connection, key: &MediaKey) -> Result<Option<ItemRef>, Error> {
    if let Some(ep) = progress::episode_for_key(conn, key)? {
        return Ok(Some(ItemRef::Episode(ep)));
    }
    Ok(match key {
        MediaKey::TmdbMovie { id } => conn
            .query_row(
                "SELECT work_id FROM external_ids
                 WHERE provider = 'tmdb' AND ext_kind = 'movie' AND ext_id = ?1",
                [id.to_string()],
                |r| r.get(0),
            )
            .optional()?
            .map(ItemRef::Movie),
        MediaKey::File { fingerprint } => {
            let row: Option<(i64, Option<i64>, Option<String>)> = conn
                .query_row(
                    "SELECT f.id, m.work_id, w.kind FROM files f
                     LEFT JOIN matches m ON m.file_id = f.id AND m.state <> 'rejected'
                     LEFT JOIN works w ON w.id = m.work_id
                     WHERE f.fingerprint = ?1
                     ORDER BY f.missing_since_ms IS NOT NULL, f.id LIMIT 1",
                    [fingerprint],
                    |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?)),
                )
                .optional()?;
            match row {
                Some((_, Some(work), Some(kind))) if kind == "movie" => Some(ItemRef::Movie(work)),
                Some((file, _, _)) => conn
                    .query_row(
                        "SELECT id FROM music_tracks WHERE file_id = ?1",
                        [file],
                        |r| r.get(0),
                    )
                    .optional()?
                    .map(ItemRef::Track),
                None => None,
            }
        }
        _ => None,
    })
}

impl Catalog {
    /// Stored progress of an item.
    pub fn progress(&self, item: ItemRef) -> Result<Option<ItemProgress>, Error> {
        let conn = self.lock();
        let Some((key, _)) = key_of(&conn, item)? else {
            return Ok(None);
        };
        Ok(progress::get(&conn, PROFILE, &key)?.map(|p| ItemProgress {
            item,
            position_ms: i64::try_from(p.position_ms).unwrap_or(i64::MAX),
            duration_ms: p
                .duration_ms
                .map_or(0, |d| i64::try_from(d).unwrap_or(i64::MAX)),
            updated_at_ms: p.updated_at_ms,
            watched: p.watched,
        }))
    }

    /// Records a position report of the player (and "watched" when asked or
    /// past 95 %). Returns `false` if the item has no key yet (its file is
    /// not fingerprinted): nothing is stored.
    pub fn save_progress(
        &self,
        item: ItemRef,
        position_ms: i64,
        duration_ms: i64,
        updated_at_ms: i64,
        watched: bool,
    ) -> Result<bool, Error> {
        let mut conn = self.lock();
        let Some((key, kind)) = key_of(&conn, item)? else {
            return Ok(false);
        };
        let mut sink = LocalOnly::new(LOCAL_DEVICE_ID);
        let report = PositionReport {
            profile_id: PROFILE,
            key: key.clone(),
            kind,
            position_ms: u64::try_from(position_ms.max(0)).unwrap_or(0),
            duration_ms: u64::try_from(duration_ms).ok().filter(|d| *d > 0),
        };
        let th = Thresholds::default();
        let state = progress::report_position(&mut conn, &mut sink, &th, &report, updated_at_ms)?;
        if watched && !state.watched {
            progress::mark_watched(&mut conn, &mut sink, PROFILE, &key, updated_at_ms)?;
        }
        if duration_ms > 0 {
            // The duration the player saw fills the file until probing (LIB-006).
            let files: &str = match item {
                ItemRef::Movie(_) => {
                    "UPDATE files SET duration_ms = ?2 WHERE duration_ms IS NULL AND id IN
                       (SELECT m.file_id FROM matches m WHERE m.work_id = ?1 AND m.state <> 'rejected')"
                }
                ItemRef::Episode(_) => {
                    "UPDATE files SET duration_ms = ?2 WHERE duration_ms IS NULL AND id IN
                       (SELECT m.file_id FROM match_episodes me JOIN matches m ON m.id = me.match_id
                         WHERE me.episode_id = ?1 AND m.state <> 'rejected')"
                }
                ItemRef::Track(_) | ItemRef::Show(_) => "",
            };
            let id = match item {
                ItemRef::Movie(i) | ItemRef::Episode(i) | ItemRef::Track(i) | ItemRef::Show(i) => i,
            };
            if !files.is_empty() {
                conn.execute(files, params![id, duration_ms])?;
            }
        }
        Ok(true)
    }

    /// "Continue watching": started items between 2 % and 95 % and Up Next
    /// episodes, latest activity first.
    pub fn continue_watching(&self, limit: usize) -> Result<Vec<ContinueItem>, Error> {
        let conn = self.lock();
        let query = ContinueQuery {
            kinds: vec![MediaKind::Movie, MediaKind::Episode, MediaKind::Video],
            up_next: true,
            limit,
        };
        let entries = progress::continue_watching(&conn, &Thresholds::default(), PROFILE, &query)?;
        let mut out = Vec::with_capacity(entries.len());
        for e in entries {
            let id = match &e.episode {
                Some(ep) => Some(ItemRef::Episode(ep.episode_id)),
                None => item_of_key(&conn, &e.item.media_key)?,
            };
            let Some(id) = id else { continue };
            let Some(item) = items::item(&conn, id)? else {
                continue;
            };
            out.push(ContinueItem {
                progress: ItemProgress {
                    item: id,
                    position_ms: i64::try_from(e.item.position_ms).unwrap_or(i64::MAX),
                    duration_ms: e
                        .item
                        .duration_ms
                        .or(item.duration_ms.and_then(|d| u64::try_from(d).ok()))
                        .map_or(0, |d| i64::try_from(d).unwrap_or(i64::MAX)),
                    updated_at_ms: e.item.updated_at_ms,
                    watched: e.item.watched,
                },
                item,
                up_next: e.reason == ContinueReason::UpNext,
            });
        }
        Ok(out)
    }
}
