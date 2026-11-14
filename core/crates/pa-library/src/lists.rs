// SPDX-License-Identifier: MPL-2.0
//! Favorites, collections, playlists and default versions (LIB-010).
//!
//! - [`Entry`]: what a list holds — a work (a movie or a whole show) by
//!   `works.id`, or a playable item by its stable media key (an episode, a
//!   track, an unrecognized video). Keys survive rescans and move between
//!   devices; work ids are local.
//! - Favorites of a profile ([`add_favorite`], [`favorites`]), each either
//!   shown in the Favorites row of Home or not ([`set_favorite_pinned`]).
//! - Collections: `manual` ones a user makes ([`create_collection`]) and
//!   `external` ones from provider metadata, e.g. a TMDB collection
//!   ([`link_external_collection`], `external_ref = tmdb:collection:<id>`),
//!   visible to every profile.
//! - Playlists of playable items in order ([`create_playlist`],
//!   [`add_to_playlist`]): `video` (LIB-010) or `audio`.
//! - The default version of a work ([`set_default_version`]): the file that
//!   plays when a movie has several (4K, 1080p, Director's Cut).
//!
//! Items are resolved to rows by the caller (`pa-catalog`); this module only
//! stores references.

use rusqlite::{Connection, OptionalExtension, Row, params};

use crate::Error;

/// A reference held by a favorite, a collection or a playlist.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum Entry {
    /// `works.id`: a movie or a whole show.
    Work(i64),
    /// Stable media key (`tmdb:episode:…`, `file:…`).
    Media(String),
}

impl Entry {
    fn columns(&self) -> (Option<i64>, Option<&str>) {
        match self {
            Self::Work(w) => (Some(*w), None),
            Self::Media(k) => (None, Some(k.as_str())),
        }
    }

    fn from_columns(work: Option<i64>, key: Option<String>) -> rusqlite::Result<Self> {
        match (work, key) {
            (Some(w), None) => Ok(Self::Work(w)),
            (None, Some(k)) => Ok(Self::Media(k)),
            _ => Err(rusqlite::Error::InvalidQuery),
        }
    }
}

fn check_key(key: &str) -> Result<(), Error> {
    let ok = (3..=256).contains(&key.len())
        && key
            .split_once(':')
            .is_some_and(|(a, b)| !a.is_empty() && !b.is_empty());
    if ok {
        Ok(())
    } else {
        Err(Error::Invalid("media key"))
    }
}

fn check_entry(entry: &Entry) -> Result<(), Error> {
    match entry {
        Entry::Work(_) => Ok(()),
        Entry::Media(k) => check_key(k),
    }
}

fn check_name(name: &str, what: &'static str) -> Result<String, Error> {
    let name = name.trim();
    if name.is_empty() {
        Err(Error::Invalid(what))
    } else {
        Ok(name.to_owned())
    }
}

// ---------------------------------------------------------------------------
// favorites

/// A favorite of a profile.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Favorite {
    pub id: i64,
    pub entry: Entry,
    /// Shown in the Favorites row of Home.
    pub pinned_home: bool,
    pub created_at_ms: i64,
}

/// Adds `entry` to the favorites of `profile_id` (pinned to Home). `false`
/// if it already was a favorite (its pin is left as it was).
pub fn add_favorite(
    conn: &Connection,
    profile_id: i64,
    entry: &Entry,
    now_ms: i64,
) -> Result<bool, Error> {
    check_entry(entry)?;
    let (work, key) = entry.columns();
    let n = conn
        .prepare_cached(
            "INSERT OR IGNORE INTO favorites (profile_id, media_key, work_id, created_at_ms)
             VALUES (?1, ?2, ?3, ?4)",
        )?
        .execute(params![profile_id, key, work, now_ms])?;
    Ok(n > 0)
}

/// Removes `entry` from the favorites; `false` if it was none.
pub fn remove_favorite(conn: &Connection, profile_id: i64, entry: &Entry) -> Result<bool, Error> {
    let (work, key) = entry.columns();
    let n = conn
        .prepare_cached(
            "DELETE FROM favorites WHERE profile_id = ?1
               AND work_id IS ?2 AND media_key IS ?3",
        )?
        .execute(params![profile_id, work, key])?;
    Ok(n > 0)
}

/// `true` if `entry` is a favorite of `profile_id`.
pub fn is_favorite(conn: &Connection, profile_id: i64, entry: &Entry) -> Result<bool, Error> {
    let (work, key) = entry.columns();
    Ok(conn
        .prepare_cached(
            "SELECT 1 FROM favorites WHERE profile_id = ?1
               AND work_id IS ?2 AND media_key IS ?3",
        )?
        .query_row(params![profile_id, work, key], |_| Ok(()))
        .optional()?
        .is_some())
}

/// Shows (`true`) or hides a favorite in the Favorites row of Home; `false`
/// if `entry` is not a favorite.
pub fn set_favorite_pinned(
    conn: &Connection,
    profile_id: i64,
    entry: &Entry,
    pinned: bool,
) -> Result<bool, Error> {
    let (work, key) = entry.columns();
    let n = conn
        .prepare_cached(
            "UPDATE favorites SET pinned_home = ?4 WHERE profile_id = ?1
               AND work_id IS ?2 AND media_key IS ?3",
        )?
        .execute(params![profile_id, work, key, pinned])?;
    Ok(n > 0)
}

fn favorite_row(r: &Row<'_>) -> rusqlite::Result<Favorite> {
    Ok(Favorite {
        id: r.get(0)?,
        entry: Entry::from_columns(r.get(1)?, r.get(2)?)?,
        pinned_home: r.get(3)?,
        created_at_ms: r.get(4)?,
    })
}

/// Favorites of `profile_id`, most recently added first; `pinned_only`:
/// those shown on Home.
pub fn favorites(
    conn: &Connection,
    profile_id: i64,
    pinned_only: bool,
) -> Result<Vec<Favorite>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, work_id, media_key, pinned_home, created_at_ms FROM favorites
         WHERE profile_id = ?1 AND (?2 = 0 OR pinned_home = 1)
         ORDER BY created_at_ms DESC, id DESC",
    )?;
    let rows = stmt.query_map(params![profile_id, pinned_only], favorite_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

// ---------------------------------------------------------------------------
// collections

/// Kind of a collection (`collections.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum CollectionKind {
    /// Made by a user.
    Manual,
    /// A saved filter (`rule_json`); smart shelves of LIB-009 are separate.
    Smart,
    /// Imported from a provider (`external_ref`, e.g. a TMDB collection).
    External,
}

impl CollectionKind {
    fn parse(s: &str) -> rusqlite::Result<Self> {
        Ok(match s {
            "manual" => Self::Manual,
            "smart" => Self::Smart,
            "external" => Self::External,
            _ => return Err(rusqlite::Error::InvalidQuery),
        })
    }
}

/// A collection with the number of its entries.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Collection {
    pub id: i64,
    /// `None`: shared by every profile.
    pub profile_id: Option<i64>,
    pub name: String,
    pub kind: CollectionKind,
    /// Source of an external collection (`tmdb:collection:10`).
    pub external_ref: Option<String>,
    pub item_count: i64,
    pub updated_at_ms: i64,
}

const COLLECTION_SQL: &str = "
SELECT c.id, c.profile_id, c.name, c.kind, c.external_ref,
       (SELECT count(*) FROM collection_items i WHERE i.collection_id = c.id), c.updated_at_ms
FROM collections c";

fn collection_row(r: &Row<'_>) -> rusqlite::Result<Collection> {
    Ok(Collection {
        id: r.get(0)?,
        profile_id: r.get(1)?,
        name: r.get(2)?,
        kind: CollectionKind::parse(&r.get::<_, String>(3)?)?,
        external_ref: r.get(4)?,
        item_count: r.get(5)?,
        updated_at_ms: r.get(6)?,
    })
}

/// One collection.
pub fn collection(conn: &Connection, id: i64) -> Result<Option<Collection>, Error> {
    Ok(conn
        .prepare_cached(&format!("{COLLECTION_SQL} WHERE c.id = ?1"))?
        .query_row([id], collection_row)
        .optional()?)
}

/// Collections `profile_id` sees (its own and the shared ones), by name.
pub fn collections(conn: &Connection, profile_id: i64) -> Result<Vec<Collection>, Error> {
    let mut stmt = conn.prepare_cached(&format!(
        "{COLLECTION_SQL} WHERE c.profile_id IS NULL OR c.profile_id = ?1
         ORDER BY c.name COLLATE NOCASE, c.id"
    ))?;
    let rows = stmt.query_map([profile_id], collection_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Collections that hold `entry`, by name.
pub fn collections_of(
    conn: &Connection,
    profile_id: i64,
    entry: &Entry,
) -> Result<Vec<Collection>, Error> {
    let (work, key) = entry.columns();
    let mut stmt = conn.prepare_cached(&format!(
        "{COLLECTION_SQL} WHERE (c.profile_id IS NULL OR c.profile_id = ?1)
           AND EXISTS (SELECT 1 FROM collection_items i WHERE i.collection_id = c.id
                         AND i.work_id IS ?2 AND i.media_key IS ?3)
         ORDER BY c.name COLLATE NOCASE, c.id"
    ))?;
    let rows = stmt.query_map(params![profile_id, work, key], collection_row)?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Creates a manual collection of `profile_id` (`None`: shared).
pub fn create_collection(
    conn: &Connection,
    profile_id: Option<i64>,
    name: &str,
    now_ms: i64,
) -> Result<Collection, Error> {
    let name = check_name(name, "collection name")?;
    conn.prepare_cached(
        "INSERT INTO collections (profile_id, name, kind, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, 'manual', ?3, ?3)",
    )?
    .execute(params![profile_id, name, now_ms])?;
    collection(conn, conn.last_insert_rowid())?.ok_or(Error::Invalid("collection"))
}

/// Renames a collection; `false` if it does not exist.
pub fn rename_collection(
    conn: &Connection,
    id: i64,
    name: &str,
    now_ms: i64,
) -> Result<bool, Error> {
    let name = check_name(name, "collection name")?;
    Ok(conn
        .prepare_cached("UPDATE collections SET name = ?2, updated_at_ms = ?3 WHERE id = ?1")?
        .execute(params![id, name, now_ms])?
        > 0)
}

/// Deletes a collection with its entries; `false` if it did not exist.
pub fn delete_collection(conn: &Connection, id: i64) -> Result<bool, Error> {
    Ok(conn
        .prepare_cached("DELETE FROM collections WHERE id = ?1")?
        .execute([id])?
        > 0)
}

/// Entries of a collection in order.
pub fn collection_entries(conn: &Connection, id: i64) -> Result<Vec<Entry>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT work_id, media_key FROM collection_items WHERE collection_id = ?1
         ORDER BY position, id",
    )?;
    let rows = stmt.query_map([id], |r| Entry::from_columns(r.get(0)?, r.get(1)?))?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Appends `entry` to a collection; `false` if it is already there.
pub fn add_to_collection(
    conn: &Connection,
    id: i64,
    entry: &Entry,
    now_ms: i64,
) -> Result<bool, Error> {
    check_entry(entry)?;
    let (work, key) = entry.columns();
    let n = conn
        .prepare_cached(
            "INSERT OR IGNORE INTO collection_items (collection_id, position, work_id, media_key,
                                                     added_at_ms)
             SELECT ?1, ifnull(max(position) + 1, 0), ?2, ?3, ?4
             FROM collection_items WHERE collection_id = ?1",
        )?
        .execute(params![id, work, key, now_ms])?;
    if n > 0 {
        touch_collection(conn, id, now_ms)?;
    }
    Ok(n > 0)
}

/// Removes `entry` from a collection; `false` if it was not there.
pub fn remove_from_collection(
    conn: &Connection,
    id: i64,
    entry: &Entry,
    now_ms: i64,
) -> Result<bool, Error> {
    let (work, key) = entry.columns();
    let n = conn
        .prepare_cached(
            "DELETE FROM collection_items WHERE collection_id = ?1
               AND work_id IS ?2 AND media_key IS ?3",
        )?
        .execute(params![id, work, key])?;
    if n > 0 {
        touch_collection(conn, id, now_ms)?;
    }
    Ok(n > 0)
}

fn touch_collection(conn: &Connection, id: i64, now_ms: i64) -> Result<(), Error> {
    conn.prepare_cached("UPDATE collections SET updated_at_ms = ?2 WHERE id = ?1")?
        .execute(params![id, now_ms])?;
    Ok(())
}

/// Puts `work` into the shared external collection `external_ref` (e.g.
/// `tmdb:collection:10`), creating it with `name` or renaming it to `name`.
/// Returns the collection id.
pub fn link_external_collection(
    conn: &Connection,
    external_ref: &str,
    name: &str,
    work: i64,
    now_ms: i64,
) -> Result<i64, Error> {
    if external_ref.is_empty() {
        return Err(Error::Invalid("external collection ref"));
    }
    let name = check_name(name, "collection name")?;
    let existing: Option<(i64, String)> = conn
        .prepare_cached("SELECT id, name FROM collections WHERE external_ref = ?1")?
        .query_row([external_ref], |r| Ok((r.get(0)?, r.get(1)?)))
        .optional()?;
    let id = match existing {
        Some((id, old)) => {
            if old != name {
                rename_collection(conn, id, &name, now_ms)?;
            }
            id
        }
        None => {
            conn.prepare_cached(
                "INSERT INTO collections (profile_id, name, kind, external_ref, created_at_ms,
                                          updated_at_ms)
                 VALUES (NULL, ?1, 'external', ?2, ?3, ?3)",
            )?
            .execute(params![name, external_ref, now_ms])?;
            conn.last_insert_rowid()
        }
    };
    add_to_collection(conn, id, &Entry::Work(work), now_ms)?;
    Ok(id)
}

/// Removes `work` from every external collection whose ref starts with
/// `prefix` except `keep` (the work moved to another provider collection or
/// lost it), and deletes external collections left empty.
pub fn unlink_external_collections(
    conn: &Connection,
    prefix: &str,
    work: i64,
    keep: Option<&str>,
) -> Result<(), Error> {
    conn.prepare_cached(
        "DELETE FROM collection_items WHERE work_id = ?1 AND collection_id IN
           (SELECT id FROM collections WHERE kind = 'external'
              AND substr(external_ref, 1, length(?2)) = ?2 AND external_ref IS NOT ?3)",
    )?
    .execute(params![work, prefix, keep])?;
    conn.prepare_cached(
        "DELETE FROM collections WHERE kind = 'external'
           AND NOT EXISTS (SELECT 1 FROM collection_items i WHERE i.collection_id = collections.id)",
    )?
    .execute([])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// playlists

/// Kind of a playlist (`playlists.kind`).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PlaylistKind {
    Audio,
    Video,
}

impl PlaylistKind {
    /// The value stored in the database.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Audio => "audio",
            Self::Video => "video",
        }
    }

    fn parse(s: &str) -> rusqlite::Result<Self> {
        Ok(match s {
            "audio" => Self::Audio,
            "video" => Self::Video,
            _ => return Err(rusqlite::Error::InvalidQuery),
        })
    }
}

/// A playlist with the number of its items.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Playlist {
    pub id: i64,
    pub profile_id: i64,
    pub name: String,
    pub kind: PlaylistKind,
    pub item_count: i64,
    pub updated_at_ms: i64,
}

/// An item of a playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistItem {
    /// `playlist_items.id`.
    pub id: i64,
    pub media_key: String,
    pub added_at_ms: i64,
}

const PLAYLIST_SQL: &str = "
SELECT p.id, p.profile_id, p.name, p.kind,
       (SELECT count(*) FROM playlist_items i WHERE i.playlist_id = p.id), p.updated_at_ms
FROM playlists p";

fn playlist_row(r: &Row<'_>) -> rusqlite::Result<Playlist> {
    Ok(Playlist {
        id: r.get(0)?,
        profile_id: r.get(1)?,
        name: r.get(2)?,
        kind: PlaylistKind::parse(&r.get::<_, String>(3)?)?,
        item_count: r.get(4)?,
        updated_at_ms: r.get(5)?,
    })
}

/// One playlist.
pub fn playlist(conn: &Connection, id: i64) -> Result<Option<Playlist>, Error> {
    Ok(conn
        .prepare_cached(&format!("{PLAYLIST_SQL} WHERE p.id = ?1"))?
        .query_row([id], playlist_row)
        .optional()?)
}

/// Playlists of `profile_id` (of one kind, or all), most recently changed
/// first.
pub fn playlists(
    conn: &Connection,
    profile_id: i64,
    kind: Option<PlaylistKind>,
) -> Result<Vec<Playlist>, Error> {
    let mut stmt = conn.prepare_cached(&format!(
        "{PLAYLIST_SQL} WHERE p.profile_id = ?1 AND (?2 IS NULL OR p.kind = ?2)
         ORDER BY p.updated_at_ms DESC, p.id DESC"
    ))?;
    let rows = stmt.query_map(
        params![profile_id, kind.map(PlaylistKind::as_str)],
        playlist_row,
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Creates an empty playlist.
pub fn create_playlist(
    conn: &Connection,
    profile_id: i64,
    name: &str,
    kind: PlaylistKind,
    now_ms: i64,
) -> Result<Playlist, Error> {
    let name = check_name(name, "playlist name")?;
    conn.prepare_cached(
        "INSERT INTO playlists (profile_id, name, kind, created_at_ms, updated_at_ms)
         VALUES (?1, ?2, ?3, ?4, ?4)",
    )?
    .execute(params![profile_id, name, kind.as_str(), now_ms])?;
    playlist(conn, conn.last_insert_rowid())?.ok_or(Error::Invalid("playlist"))
}

/// Renames a playlist; `false` if it does not exist.
pub fn rename_playlist(conn: &Connection, id: i64, name: &str, now_ms: i64) -> Result<bool, Error> {
    let name = check_name(name, "playlist name")?;
    Ok(conn
        .prepare_cached("UPDATE playlists SET name = ?2, updated_at_ms = ?3 WHERE id = ?1")?
        .execute(params![id, name, now_ms])?
        > 0)
}

/// Deletes a playlist with its items; `false` if it did not exist.
pub fn delete_playlist(conn: &Connection, id: i64) -> Result<bool, Error> {
    Ok(conn
        .prepare_cached("DELETE FROM playlists WHERE id = ?1")?
        .execute([id])?
        > 0)
}

/// Items of a playlist in order.
pub fn playlist_items(conn: &Connection, id: i64) -> Result<Vec<PlaylistItem>, Error> {
    let mut stmt = conn.prepare_cached(
        "SELECT id, media_key, added_at_ms FROM playlist_items WHERE playlist_id = ?1
         ORDER BY position, id",
    )?;
    let rows = stmt.query_map([id], |r| {
        Ok(PlaylistItem {
            id: r.get(0)?,
            media_key: r.get(1)?,
            added_at_ms: r.get(2)?,
        })
    })?;
    Ok(rows.collect::<Result<_, _>>()?)
}

/// Appends `media_key` to a playlist unless it is already there; `false` if
/// it was (a playlist holds an item once).
pub fn add_to_playlist(
    conn: &Connection,
    id: i64,
    media_key: &str,
    now_ms: i64,
) -> Result<bool, Error> {
    check_key(media_key)?;
    if playlist(conn, id)?.is_none() {
        return Err(Error::Invalid("playlist"));
    }
    let n = conn
        .prepare_cached(
            "INSERT INTO playlist_items (playlist_id, position, media_key, added_at_ms)
             SELECT ?1, ifnull((SELECT max(position) + 1 FROM playlist_items WHERE playlist_id = ?1), 0),
                    ?2, ?3
             WHERE NOT EXISTS (SELECT 1 FROM playlist_items WHERE playlist_id = ?1 AND media_key = ?2)",
        )?
        .execute(params![id, media_key, now_ms])?;
    if n > 0 {
        touch_playlist(conn, id, now_ms)?;
    }
    Ok(n > 0)
}

/// Removes `media_key` from a playlist; `false` if it was not there.
pub fn remove_from_playlist(
    conn: &Connection,
    id: i64,
    media_key: &str,
    now_ms: i64,
) -> Result<bool, Error> {
    let n = conn
        .prepare_cached("DELETE FROM playlist_items WHERE playlist_id = ?1 AND media_key = ?2")?
        .execute(params![id, media_key])?;
    if n > 0 {
        touch_playlist(conn, id, now_ms)?;
    }
    Ok(n > 0)
}

/// Puts the items of a playlist in the order of `item_ids`
/// ([`PlaylistItem::id`]); items not listed keep their order after them.
pub fn reorder_playlist(
    conn: &mut Connection,
    id: i64,
    item_ids: &[i64],
    now_ms: i64,
) -> Result<(), Error> {
    let tx = conn.transaction()?;
    let current: Vec<i64> = playlist_items(&tx, id)?.iter().map(|i| i.id).collect();
    let mut order: Vec<i64> = Vec::with_capacity(current.len());
    for i in item_ids {
        if current.contains(i) && !order.contains(i) {
            order.push(*i);
        }
    }
    for i in current {
        if !order.contains(&i) {
            order.push(i);
        }
    }
    {
        let mut stmt = tx.prepare_cached(
            "UPDATE playlist_items SET position = ?2 WHERE id = ?1 AND position <> ?2",
        )?;
        for (pos, item) in order.iter().enumerate() {
            stmt.execute(params![item, i64::try_from(pos).unwrap_or(i64::MAX)])?;
        }
    }
    touch_playlist(&tx, id, now_ms)?;
    tx.commit()?;
    Ok(())
}

fn touch_playlist(conn: &Connection, id: i64, now_ms: i64) -> Result<(), Error> {
    conn.prepare_cached("UPDATE playlists SET updated_at_ms = ?2 WHERE id = ?1")?
        .execute(params![id, now_ms])?;
    Ok(())
}

// ---------------------------------------------------------------------------
// default versions

/// The default version (`files.id`) of `work` for `profile_id`, if one was
/// chosen.
pub fn default_version(conn: &Connection, profile_id: i64, work: i64) -> Result<Option<i64>, Error> {
    Ok(conn
        .prepare_cached("SELECT file_id FROM version_prefs WHERE profile_id = ?1 AND work_id = ?2")?
        .query_row(params![profile_id, work], |r| r.get(0))
        .optional()?)
}

/// Makes `file` the default version of `work` for `profile_id`, or clears
/// the choice (`None`: the first file plays). The caller checks that the
/// file is a version of the work.
pub fn set_default_version(
    conn: &Connection,
    profile_id: i64,
    work: i64,
    file: Option<i64>,
    now_ms: i64,
) -> Result<(), Error> {
    match file {
        Some(f) => conn
            .prepare_cached(
                "INSERT INTO version_prefs (profile_id, work_id, file_id, updated_at_ms)
                 VALUES (?1, ?2, ?3, ?4)
                 ON CONFLICT (profile_id, work_id) DO UPDATE
                 SET file_id = excluded.file_id, updated_at_ms = excluded.updated_at_ms",
            )?
            .execute(params![profile_id, work, f, now_ms])?,
        None => conn
            .prepare_cached("DELETE FROM version_prefs WHERE profile_id = ?1 AND work_id = ?2")?
            .execute(params![profile_id, work])?,
    };
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::open_in_memory;

    fn setup() -> Connection {
        let conn = open_in_memory().unwrap();
        conn.execute_batch(
            "INSERT INTO sources (id, kind, name, uri, created_at_ms)
                 VALUES (1, 'local', 'M', 'file:///m', 0);
             INSERT INTO files (id, source_id, path, size_bytes, added_at_ms)
                 VALUES (10, 1, '/a.mkv', 1, 0), (11, 1, '/b.mkv', 1, 0);
             INSERT INTO works (id, kind, title, created_at_ms, updated_at_ms)
                 VALUES (1, 'movie', 'A', 0, 0), (2, 'movie', 'B', 0, 0);",
        )
        .unwrap();
        conn
    }

    #[test]
    fn favorites_add_pin_and_remove() {
        let conn = setup();
        let work = Entry::Work(1);
        let ep = Entry::Media("tmdb:episode:1:1:1".into());
        assert!(add_favorite(&conn, 1, &work, 10).unwrap());
        assert!(!add_favorite(&conn, 1, &work, 11).unwrap());
        assert!(add_favorite(&conn, 1, &ep, 12).unwrap());
        assert!(add_favorite(&conn, 1, &Entry::Media("bad".into()), 13).is_err());
        assert!(is_favorite(&conn, 1, &work).unwrap());
        let all = favorites(&conn, 1, false).unwrap();
        assert_eq!(
            all.iter().map(|f| f.entry.clone()).collect::<Vec<_>>(),
            vec![ep.clone(), work.clone()]
        );
        assert!(set_favorite_pinned(&conn, 1, &ep, false).unwrap());
        let pinned = favorites(&conn, 1, true).unwrap();
        assert_eq!(pinned.len(), 1);
        assert_eq!(pinned[0].entry, work);
        assert!(remove_favorite(&conn, 1, &work).unwrap());
        assert!(!remove_favorite(&conn, 1, &work).unwrap());
        assert!(!is_favorite(&conn, 1, &work).unwrap());
    }

    #[test]
    fn collections_hold_works_in_order() {
        let conn = setup();
        let c = create_collection(&conn, Some(1), " Mine ", 1).unwrap();
        assert_eq!(c.name, "Mine");
        assert_eq!(c.kind, CollectionKind::Manual);
        assert!(create_collection(&conn, Some(1), "  ", 1).is_err());
        assert!(add_to_collection(&conn, c.id, &Entry::Work(2), 2).unwrap());
        assert!(add_to_collection(&conn, c.id, &Entry::Work(1), 3).unwrap());
        assert!(!add_to_collection(&conn, c.id, &Entry::Work(1), 4).unwrap());
        assert_eq!(
            collection_entries(&conn, c.id).unwrap(),
            vec![Entry::Work(2), Entry::Work(1)]
        );
        assert_eq!(collection(&conn, c.id).unwrap().unwrap().item_count, 2);
        assert!(remove_from_collection(&conn, c.id, &Entry::Work(2), 5).unwrap());
        assert_eq!(collections_of(&conn, 1, &Entry::Work(1)).unwrap().len(), 1);
        assert!(rename_collection(&conn, c.id, "Ours", 6).unwrap());
        assert!(delete_collection(&conn, c.id).unwrap());
        assert!(collections(&conn, 1).unwrap().is_empty());
    }

    #[test]
    fn external_collections_are_shared_and_follow_the_provider() {
        let conn = setup();
        let id = link_external_collection(&conn, "tmdb:collection:10", "Saga", 1, 1).unwrap();
        assert_eq!(
            link_external_collection(&conn, "tmdb:collection:10", "Saga 2", 2, 2).unwrap(),
            id
        );
        let c = collection(&conn, id).unwrap().unwrap();
        assert_eq!((c.name.as_str(), c.profile_id, c.kind), ("Saga 2", None, CollectionKind::External));
        assert_eq!(c.item_count, 2);
        // Visible to any profile.
        assert_eq!(collections(&conn, 7).unwrap().len(), 1);
        // Work 1 lost the collection, work 2 moved to another one.
        unlink_external_collections(&conn, "tmdb:collection:", 1, None).unwrap();
        unlink_external_collections(&conn, "tmdb:collection:", 2, Some("tmdb:collection:11"))
            .unwrap();
        assert!(collection(&conn, id).unwrap().is_none());
    }

    #[test]
    fn playlists_keep_order_and_hold_an_item_once() {
        let mut conn = setup();
        let p = create_playlist(&conn, 1, "Night", PlaylistKind::Video, 1).unwrap();
        assert!(add_to_playlist(&conn, p.id, "tmdb:movie:603", 2).unwrap());
        assert!(add_to_playlist(&conn, p.id, "file:ab12", 3).unwrap());
        assert!(!add_to_playlist(&conn, p.id, "file:ab12", 4).unwrap());
        assert!(add_to_playlist(&conn, p.id + 1, "file:ab12", 4).is_err());
        let items = playlist_items(&conn, p.id).unwrap();
        assert_eq!(
            items.iter().map(|i| i.media_key.as_str()).collect::<Vec<_>>(),
            ["tmdb:movie:603", "file:ab12"]
        );
        reorder_playlist(&mut conn, p.id, &[items[1].id], 5).unwrap();
        let keys: Vec<_> = playlist_items(&conn, p.id)
            .unwrap()
            .into_iter()
            .map(|i| i.media_key)
            .collect();
        assert_eq!(keys, ["file:ab12", "tmdb:movie:603"]);
        assert_eq!(
            playlists(&conn, 1, Some(PlaylistKind::Video)).unwrap()[0].item_count,
            2
        );
        assert!(playlists(&conn, 1, Some(PlaylistKind::Audio)).unwrap().is_empty());
        assert!(remove_from_playlist(&conn, p.id, "file:ab12", 6).unwrap());
        assert!(rename_playlist(&conn, p.id, "Late", 7).unwrap());
        assert!(delete_playlist(&conn, p.id).unwrap());
    }

    #[test]
    fn default_version_is_set_cleared_and_follows_the_file() {
        let conn = setup();
        assert_eq!(default_version(&conn, 1, 1).unwrap(), None);
        set_default_version(&conn, 1, 1, Some(11), 1).unwrap();
        assert_eq!(default_version(&conn, 1, 1).unwrap(), Some(11));
        set_default_version(&conn, 1, 1, Some(10), 2).unwrap();
        assert_eq!(default_version(&conn, 1, 1).unwrap(), Some(10));
        set_default_version(&conn, 1, 1, None, 3).unwrap();
        assert_eq!(default_version(&conn, 1, 1).unwrap(), None);
        set_default_version(&conn, 1, 1, Some(11), 4).unwrap();
        conn.execute("DELETE FROM files WHERE id = 11", []).unwrap();
        assert_eq!(default_version(&conn, 1, 1).unwrap(), None);
    }
}
