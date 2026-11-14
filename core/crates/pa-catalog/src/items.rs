// SPDX-License-Identifier: MPL-2.0
//! Item views: movies, shows, episodes and tracks shaped like the release-0.1
//! `Library` of the app, and the FTS5 search over them.
//!
//! Only items with a file that is present (`missing_since_ms IS NULL`) are
//! listed; a work whose files went missing disappears until they come back.

use std::fmt;
use std::str::FromStr;

use pa_library::rusqlite::{Connection, OptionalExtension, Row, ToSql, params, params_from_iter};
use pa_library::search::{SearchKind, SearchOptions};

use crate::{Catalog, Error, uri};

/// Kind of an item.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ItemKind {
    Movie,
    /// A show or an anime series (a group of episodes).
    Show,
    Episode,
    Track,
}

/// Parsed item id: which catalog row an item is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ItemRef {
    /// `m<works.id>`.
    Movie(i64),
    /// `s<works.id>`.
    Show(i64),
    /// `e<episodes.id>`.
    Episode(i64),
    /// `t<music_tracks.id>`.
    Track(i64),
}

impl fmt::Display for ItemRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Movie(id) => write!(f, "m{id}"),
            Self::Show(id) => write!(f, "s{id}"),
            Self::Episode(id) => write!(f, "e{id}"),
            Self::Track(id) => write!(f, "t{id}"),
        }
    }
}

impl FromStr for ItemRef {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Error> {
        let bad = || Error::Invalid(format!("item id {s:?}"));
        let (prefix, rest) = s.split_at_checked(1).ok_or_else(bad)?;
        let id: i64 = rest.parse().map_err(|_| bad())?;
        Ok(match prefix {
            "m" => Self::Movie(id),
            "s" => Self::Show(id),
            "e" => Self::Episode(id),
            "t" => Self::Track(id),
            _ => return Err(bad()),
        })
    }
}

/// One entry of the library, as the app shows it.
#[derive(Debug, Clone, PartialEq)]
pub struct Item {
    pub id: ItemRef,
    pub kind: ItemKind,
    pub title: String,
    pub original_title: Option<String>,
    pub year: Option<i64>,
    pub overview: Option<String>,
    /// Episodes: the show item.
    pub show_id: Option<ItemRef>,
    pub season: Option<i64>,
    pub episode: Option<i64>,
    /// Tracks.
    pub artist: Option<String>,
    pub album: Option<String>,
    pub track_number: Option<i64>,
    /// Source of the file (shows: of their first episode).
    pub source_id: i64,
    /// File path relative to the source root. Shows: the show folder (the
    /// folder of the first episode, above a `Season N` folder).
    pub rel_path: String,
    /// Absolute local path for `file://` sources, `None` for network ones
    /// (the app resolves their mount).
    pub local_path: Option<String>,
    pub duration_ms: Option<i64>,
    pub match_confidence: Option<f64>,
    pub tmdb_id: Option<i64>,
    pub added_at_ms: Option<i64>,
    /// Poster (episodes: the show's), an https or `file://` URL.
    pub poster_url: Option<String>,
    /// Backdrop (episodes: the episode still).
    pub backdrop_url: Option<String>,
    /// Local poster or cover stored next to the files: LIB-007
    /// `group_assets` art for movies and shows (episodes: the show's), the
    /// folder cover of a music album for tracks. A path relative to the
    /// source root like [`rel_path`](Self::rel_path); the app opens it
    /// directly for local sources and through the stream proxy for network
    /// ones (LIB-019).
    pub local_poster: Option<String>,
}

/// Best artwork of a kind for a work (`w.id`) or episode (`e.id`).
macro_rules! art {
    ($owner:literal, $id:literal, $kind:literal) => {
        concat!(
            "(SELECT a.url FROM artwork a WHERE a.",
            $owner,
            " = ",
            $id,
            " AND a.kind = '",
            $kind,
            "' ORDER BY a.priority, a.id LIMIT 1)"
        )
    };
}

/// Local poster of the movie file `f` (LIB-007 `group_assets`): art named
/// after the file (`<movie>-poster.jpg`, `<movie>.jpg`) first, then generic
/// names (`poster.jpg`, `folder.jpg`) of a single-movie folder or a disc.
macro_rules! movie_local_poster {
    () => {
        "(SELECT ga.path FROM file_group_members gm
   JOIN file_groups g ON g.id = gm.group_id
   JOIN group_assets ga ON ga.group_id = gm.group_id
  WHERE gm.file_id = f.id AND ga.role = 'art' AND ga.art_kind = 'poster'
    AND ga.season_number IS NULL
    AND (ga.target_path = f.path
         OR (ga.target_path IS NULL AND g.kind IN ('movie_folder', 'disc')))
  ORDER BY ga.target_path IS NULL, length(ga.path), ga.path LIMIT 1)"
    };
}

/// Local poster of a show folder: a generic poster (not season art) of the
/// group rooted at the folder. The folder of a show with `Season N`
/// subfolders is a group of its own, without videos (LIB-007).
const SHOW_LOCAL_POSTER_SQL: &str = "
SELECT ga.path FROM file_groups g
JOIN group_assets ga ON ga.group_id = g.id
WHERE g.source_id = ?1 AND g.root_path = ?2 AND ga.role = 'art' AND ga.art_kind = 'poster'
  AND ga.season_number IS NULL AND ga.target_path IS NULL
ORDER BY length(ga.path), ga.path LIMIT 1";

/// Fills [`Item::local_poster`] of shows and episodes from their show folder.
fn fill_show_posters(conn: &Connection, items: &mut [Item]) -> Result<(), Error> {
    let mut stmt = None;
    for item in items {
        let folder = match item.kind {
            ItemKind::Show => item.rel_path.clone(),
            ItemKind::Episode => show_folder(&item.rel_path),
            _ => continue,
        };
        if folder.is_empty() || item.local_poster.is_some() {
            continue;
        }
        let stmt = match &mut stmt {
            Some(s) => s,
            None => stmt.insert(conn.prepare_cached(SHOW_LOCAL_POSTER_SQL)?),
        };
        let path: Option<String> = stmt
            .query_row(params![item.source_id, format!("/{folder}")], |r| r.get(0))
            .optional()?;
        item.local_poster = local_rel(path);
    }
    Ok(())
}

fn is_season_folder(name: &str) -> bool {
    let n = name.to_lowercase();
    n.starts_with("season")
        || n.starts_with("сезон")
        || n == "specials"
        || (n.len() <= 4
            && n.starts_with('s')
            && n.len() > 1
            && n[1..].chars().all(|c| c.is_ascii_digit()))
        || n.ends_with(" сезон")
}

/// Folder of a show from the path of one of its episodes.
fn show_folder(rel: &str) -> String {
    let mut parts: Vec<&str> = rel.split('/').collect();
    parts.pop();
    if parts.last().is_some_and(|p| is_season_folder(p)) {
        parts.pop();
    }
    parts.join("/")
}

/// Absolute local path of `rel` in a source with `uri`.
fn local_path(uri: &str, rel: &str) -> Option<String> {
    let mut root = uri::file_uri_to_path(uri)?;
    if !rel.is_empty() {
        root.push(rel);
    }
    Some(root.to_string_lossy().into_owned())
}

/// `files.path` is a `VfsPath` (`/Movies/x.mkv`); items carry it without the
/// leading slash, relative to the root.
fn relative(path: &str) -> String {
    path.trim_start_matches('/').to_owned()
}

/// [`relative`] for an optional stored path.
fn local_rel(path: Option<String>) -> Option<String> {
    path.map(|p| relative(&p)).filter(|p| !p.is_empty())
}

const MOVIE_SQL: &str = concat!(
    "
SELECT w.id, w.title, w.original_title, w.year, w.overview,
       (SELECT x.ext_id FROM external_ids x
         WHERE x.work_id = w.id AND x.provider = 'tmdb' AND x.ext_kind = 'movie'),
       f.source_id, s.uri, f.path, f.duration_ms, m.confidence, f.added_at_ms, min(f.id), ",
    art!("work_id", "w.id", "poster"),
    ", ",
    art!("work_id", "w.id", "backdrop"),
    ", ",
    movie_local_poster!(),
    "
FROM works w
JOIN matches m ON m.work_id = w.id AND m.state <> 'rejected'
JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
WHERE w.kind = 'movie'
  AND NOT EXISTS (SELECT 1 FROM file_group_members gm
                  WHERE gm.file_id = f.id AND gm.role IN ('extra', 'sample', 'ignore'))"
);

const SHOW_SQL: &str = concat!(
    "
SELECT w.id, w.title, w.original_title, w.year, w.overview,
       (SELECT x.ext_id FROM external_ids x
         WHERE x.work_id = w.id AND x.provider = 'tmdb' AND x.ext_kind = 'tv'),
       f.source_id, s.uri, min(f.path), NULL, m.confidence,
       (SELECT max(f2.added_at_ms) FROM matches m2 JOIN files f2 ON f2.id = m2.file_id
         WHERE m2.work_id = w.id AND m2.state <> 'rejected' AND f2.missing_since_ms IS NULL) AS added, ",
    art!("work_id", "w.id", "poster"),
    ", ",
    art!("work_id", "w.id", "backdrop"),
    "
FROM works w
JOIN matches m ON m.work_id = w.id AND m.state <> 'rejected'
JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
WHERE w.kind IN ('show', 'anime')
  AND NOT EXISTS (SELECT 1 FROM file_group_members gm
                  WHERE gm.file_id = f.id AND gm.role IN ('extra', 'sample', 'ignore'))"
);

const EPISODE_SQL: &str = concat!(
    "
SELECT e.id, e.title, e.work_id, e.season_number, ifnull(e.episode_number, e.absolute_number),
       f.source_id, s.uri, f.path, f.duration_ms, m.confidence, f.added_at_ms, min(f.id),
       e.overview, ",
    art!("work_id", "e.work_id", "poster"),
    ", ",
    art!("episode_id", "e.id", "still"),
    "
FROM episodes e
JOIN match_episodes me ON me.episode_id = e.id
JOIN matches m ON m.id = me.match_id AND m.state <> 'rejected'
JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
WHERE 1"
);

const TRACK_SQL: &str = "
SELECT t.id, t.title, t.track_number, t.duration_ms, t.year, a.title,
       (SELECT ar.name FROM music_track_artists ta JOIN music_artists ar ON ar.id = ta.artist_id
         WHERE ta.track_id = t.id AND ta.role = 'artist' ORDER BY ta.position LIMIT 1) AS artist,
       f.source_id, s.uri, f.path, f.added_at_ms,
       CASE WHEN a.art_kind = 'folder' AND a.art_source_id = f.source_id THEN a.art_path END
FROM music_tracks t
JOIN files f ON f.id = t.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
LEFT JOIN music_albums a ON a.id = t.album_id
WHERE 1";

fn tmdb_id(v: Option<String>) -> Option<i64> {
    v.and_then(|s| s.parse().ok())
}

fn movie_row(r: &Row<'_>) -> pa_library::rusqlite::Result<Item> {
    let uri: String = r.get(7)?;
    let rel = relative(&r.get::<_, String>(8)?);
    Ok(Item {
        id: ItemRef::Movie(r.get(0)?),
        kind: ItemKind::Movie,
        title: r.get(1)?,
        original_title: r.get(2)?,
        year: r.get(3)?,
        overview: r.get(4)?,
        tmdb_id: tmdb_id(r.get(5)?),
        source_id: r.get(6)?,
        local_path: local_path(&uri, &rel),
        rel_path: rel,
        duration_ms: r.get(9)?,
        match_confidence: r.get(10)?,
        added_at_ms: r.get(11)?,
        poster_url: r.get(13)?,
        backdrop_url: r.get(14)?,
        local_poster: local_rel(r.get(15)?),
        show_id: None,
        season: None,
        episode: None,
        artist: None,
        album: None,
        track_number: None,
    })
}

fn show_row(r: &Row<'_>) -> pa_library::rusqlite::Result<Item> {
    let uri: String = r.get(7)?;
    let rel = show_folder(&relative(&r.get::<_, String>(8)?));
    Ok(Item {
        id: ItemRef::Show(r.get(0)?),
        kind: ItemKind::Show,
        title: r.get(1)?,
        original_title: r.get(2)?,
        year: r.get(3)?,
        overview: r.get(4)?,
        tmdb_id: tmdb_id(r.get(5)?),
        source_id: r.get(6)?,
        local_path: local_path(&uri, &rel),
        rel_path: rel,
        duration_ms: None,
        match_confidence: r.get(10)?,
        added_at_ms: r.get(11)?,
        poster_url: r.get(12)?,
        backdrop_url: r.get(13)?,
        // Filled from the show folder by `fill_show_posters`.
        local_poster: None,
        show_id: None,
        season: None,
        episode: None,
        artist: None,
        album: None,
        track_number: None,
    })
}

fn episode_row(r: &Row<'_>) -> pa_library::rusqlite::Result<Item> {
    let uri: String = r.get(6)?;
    let rel = relative(&r.get::<_, String>(7)?);
    Ok(Item {
        id: ItemRef::Episode(r.get(0)?),
        kind: ItemKind::Episode,
        title: r.get::<_, Option<String>>(1)?.unwrap_or_default(),
        show_id: Some(ItemRef::Show(r.get(2)?)),
        season: r.get(3)?,
        episode: r.get(4)?,
        source_id: r.get(5)?,
        local_path: local_path(&uri, &rel),
        rel_path: rel,
        duration_ms: r.get(8)?,
        match_confidence: r.get(9)?,
        added_at_ms: r.get(10)?,
        overview: r.get(12)?,
        poster_url: r.get(13)?,
        backdrop_url: r.get(14)?,
        local_poster: None,
        original_title: None,
        year: None,
        tmdb_id: None,
        artist: None,
        album: None,
        track_number: None,
    })
}

fn track_row(r: &Row<'_>) -> pa_library::rusqlite::Result<Item> {
    let uri: String = r.get(8)?;
    let rel = relative(&r.get::<_, String>(9)?);
    Ok(Item {
        id: ItemRef::Track(r.get(0)?),
        kind: ItemKind::Track,
        title: r.get(1)?,
        track_number: r.get(2)?,
        duration_ms: r.get(3)?,
        year: r.get(4)?,
        album: r.get(5)?,
        artist: r.get(6)?,
        source_id: r.get(7)?,
        local_path: local_path(&uri, &rel),
        rel_path: rel,
        added_at_ms: r.get(10)?,
        match_confidence: Some(1.0),
        original_title: None,
        overview: None,
        show_id: None,
        season: None,
        episode: None,
        tmdb_id: None,
        poster_url: None,
        backdrop_url: None,
        local_poster: local_rel(r.get(11)?),
    })
}

type RowFn = fn(&Row<'_>) -> pa_library::rusqlite::Result<Item>;

fn query(
    conn: &Connection,
    base: &str,
    filter: &str,
    tail: &str,
    args: &[&dyn ToSql],
    row: RowFn,
) -> Result<Vec<Item>, Error> {
    let sql = format!("{base} {filter} {tail}");
    let mut stmt = conn.prepare_cached(&sql)?;
    let rows = stmt.query_map(params_from_iter(args.iter()), row)?;
    let mut items: Vec<Item> = rows.collect::<Result<_, _>>()?;
    fill_show_posters(conn, &mut items)?;
    Ok(items)
}

/// Movies (`ItemKind::Movie`) or shows (`ItemKind::Show`) with these work
/// ids, in the order of `ids`; ids without a present file are left out.
pub(crate) fn works_by_ids(
    conn: &Connection,
    kind: ItemKind,
    ids: &[i64],
) -> Result<Vec<Item>, Error> {
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let json = serde_json::to_string(ids).map_err(|e| Error::Invalid(e.to_string()))?;
    let filter = "AND w.id IN (SELECT value FROM json_each(?1))";
    let found = match kind {
        ItemKind::Movie => query(
            conn,
            MOVIE_SQL,
            filter,
            "GROUP BY w.id",
            &[&json],
            movie_row,
        )?,
        ItemKind::Show => query(conn, SHOW_SQL, filter, "GROUP BY w.id", &[&json], show_row)?,
        other => return Err(Error::Invalid(format!("not a work kind: {other:?}"))),
    };
    let mut by_id: std::collections::HashMap<ItemRef, Item> =
        found.into_iter().map(|i| (i.id, i)).collect();
    Ok(ids
        .iter()
        .filter_map(|id| {
            by_id.remove(&match kind {
                ItemKind::Movie => ItemRef::Movie(*id),
                _ => ItemRef::Show(*id),
            })
        })
        .collect())
}

pub(crate) fn movies(conn: &Connection) -> Result<Vec<Item>, Error> {
    query(
        conn,
        MOVIE_SQL,
        "",
        "GROUP BY w.id ORDER BY w.title COLLATE NOCASE, w.year, w.id",
        &[],
        movie_row,
    )
}

pub(crate) fn shows(conn: &Connection) -> Result<Vec<Item>, Error> {
    query(
        conn,
        SHOW_SQL,
        "",
        "GROUP BY w.id ORDER BY w.title COLLATE NOCASE, w.id",
        &[],
        show_row,
    )
}

pub(crate) fn episodes(conn: &Connection, work_id: i64) -> Result<Vec<Item>, Error> {
    query(
        conn,
        EPISODE_SQL,
        "AND e.work_id = ?1",
        "GROUP BY e.id ORDER BY ifnull(e.season_number, 1), \
         ifnull(e.episode_number, e.absolute_number), f.path, e.id",
        &[&work_id],
        episode_row,
    )
}

pub(crate) fn tracks(conn: &Connection) -> Result<Vec<Item>, Error> {
    query(
        conn,
        TRACK_SQL,
        "",
        "ORDER BY artist COLLATE NOCASE, a.title COLLATE NOCASE, t.disc_number, \
         t.track_number, t.title COLLATE NOCASE, t.id",
        &[],
        track_row,
    )
}

pub(crate) fn item(conn: &Connection, id: ItemRef) -> Result<Option<Item>, Error> {
    let found = match id {
        ItemRef::Movie(w) => query(
            conn,
            MOVIE_SQL,
            "AND w.id = ?1",
            "GROUP BY w.id",
            &[&w],
            movie_row,
        )?,
        ItemRef::Show(w) => query(
            conn,
            SHOW_SQL,
            "AND w.id = ?1",
            "GROUP BY w.id",
            &[&w],
            show_row,
        )?,
        ItemRef::Episode(e) => query(
            conn,
            EPISODE_SQL,
            "AND e.id = ?1",
            "GROUP BY e.id",
            &[&e],
            episode_row,
        )?,
        ItemRef::Track(t) => query(conn, TRACK_SQL, "AND t.id = ?1", "", &[&t], track_row)?,
    };
    Ok(found.into_iter().next())
}

impl Catalog {
    /// Movies with a present file, by title.
    pub fn movies(&self) -> Result<Vec<Item>, Error> {
        movies(&self.lock())
    }

    /// Shows and anime series with a present episode file, by title.
    pub fn shows(&self) -> Result<Vec<Item>, Error> {
        shows(&self.lock())
    }

    /// Episodes of a show item (`s<work>`), in season and episode order.
    pub fn episodes(&self, show: ItemRef) -> Result<Vec<Item>, Error> {
        match show {
            ItemRef::Show(w) => episodes(&self.lock(), w),
            other => Err(Error::Invalid(format!("not a show: {other}"))),
        }
    }

    /// Music tracks by artist, album, disc and track number.
    pub fn tracks(&self) -> Result<Vec<Item>, Error> {
        tracks(&self.lock())
    }

    /// Movies or shows, most recently added first.
    pub fn recently_added(&self, kind: ItemKind, limit: usize) -> Result<Vec<Item>, Error> {
        let conn = self.lock();
        let limit = i64::try_from(limit).unwrap_or(i64::MAX);
        match kind {
            ItemKind::Movie => query(
                &conn,
                MOVIE_SQL,
                "",
                "GROUP BY w.id ORDER BY f.added_at_ms DESC, w.title LIMIT ?1",
                &[&limit],
                movie_row,
            ),
            ItemKind::Show => query(
                &conn,
                SHOW_SQL,
                "",
                "GROUP BY w.id ORDER BY added DESC, w.title LIMIT ?1",
                &[&limit],
                show_row,
            ),
            ItemKind::Episode => query(
                &conn,
                EPISODE_SQL,
                "",
                "GROUP BY e.id ORDER BY f.added_at_ms DESC, e.id LIMIT ?1",
                &[&limit],
                episode_row,
            ),
            ItemKind::Track => query(
                &conn,
                TRACK_SQL,
                "",
                "ORDER BY f.added_at_ms DESC, t.id LIMIT ?1",
                &[&limit],
                track_row,
            ),
        }
    }

    /// One item, `None` if it does not exist or has no present file.
    pub fn item(&self, id: ItemRef) -> Result<Option<Item>, Error> {
        item(&self.lock(), id)
    }

    /// FTS5 search (LIB-008) over movies, shows, episodes and tracks, best
    /// first. Artists, albums and collections are not items and are left
    /// out.
    pub fn search(&self, text: &str, limit: usize) -> Result<Vec<Item>, Error> {
        let conn = self.lock();
        let opts = SearchOptions {
            limit: limit.clamp(1, pa_library::search::MAX_LIMIT),
            kinds: vec![
                SearchKind::Movie,
                SearchKind::Show,
                SearchKind::Anime,
                SearchKind::Episode,
                SearchKind::Track,
            ],
            profile_id: None,
        };
        let hits = pa_library::search::search(&conn, text, &opts)?;
        let mut out = Vec::with_capacity(hits.len());
        for hit in hits {
            let id = match hit.kind {
                SearchKind::Movie => ItemRef::Movie(hit.id),
                SearchKind::Show | SearchKind::Anime => ItemRef::Show(hit.id),
                SearchKind::Episode => ItemRef::Episode(hit.id),
                SearchKind::Track => ItemRef::Track(hit.id),
                _ => continue,
            };
            if let Some(found) = item(&conn, id)? {
                out.push(found);
            }
        }
        Ok(out)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn item_ids_round_trip() {
        for id in [
            ItemRef::Movie(1),
            ItemRef::Show(22),
            ItemRef::Episode(333),
            ItemRef::Track(4),
        ] {
            assert_eq!(id.to_string().parse::<ItemRef>().unwrap(), id);
        }
        assert!("x1".parse::<ItemRef>().is_err());
        assert!("m".parse::<ItemRef>().is_err());
        assert!("".parse::<ItemRef>().is_err());
    }

    #[test]
    fn show_folder_skips_season_folders() {
        assert_eq!(
            show_folder("Shows/Dark/Season 1/Dark.S01E01.mkv"),
            "Shows/Dark"
        );
        assert_eq!(show_folder("Dark/S02/x.mkv"), "Dark");
        assert_eq!(show_folder("Dark/x.mkv"), "Dark");
        assert_eq!(show_folder("x.mkv"), "");
    }
}
