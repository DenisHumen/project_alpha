// SPDX-License-Identifier: MPL-2.0
//! Versions, favorites, collections and playlists (LIB-010) on the items of
//! the catalog, for the default profile.
//!
//! - **Versions** ([`Catalog::versions`]): the files of one work that each
//!   play the whole movie (or episode) — 4K and 1080p copies, a Director's
//!   Cut — are versions of one item; the library shows one card
//!   ([`Item::version_count`]). Parts of a split file and the files of one
//!   disc are one version. A movie has a default version
//!   ([`Catalog::set_default_version`]): the item plays it, else its first
//!   file. Progress is kept per item, so it is shared by the versions.
//! - **Favorites** ([`Catalog::set_favorite`]): movies and shows by work,
//!   episodes and tracks by their stable media key; each is shown in the
//!   Favorites row of Home unless unpinned
//!   ([`Catalog::set_favorite_pinned`]).
//! - **Collections** ([`Catalog::collections`]): the user's own and the TMDB
//!   collections the metadata run found (`belongs_to_collection`).
//! - **Video playlists** ([`Catalog::playlists`]): movies and episodes in
//!   order, by media key, so they survive rescans.

use pa_library::lists::{self, Entry, PlaylistKind};
use pa_library::rusqlite::{Connection, OptionalExtension, Row};
use pa_model::MediaKey;

pub use pa_library::lists::CollectionKind;

use crate::items::{self, Item, ItemRef};
use crate::progress::{PROFILE, item_of_key, key_of};
use crate::{Catalog, Error, now_ms};

/// One version of a movie or episode: a file (or a disc, or a split file)
/// that plays the whole item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Version {
    /// `files.id` of the version (the first file of a disc or a split file).
    pub file_id: i64,
    pub source_id: i64,
    /// Path relative to the source root.
    pub rel_path: String,
    /// Absolute local path for `file://` sources.
    pub local_path: Option<String>,
    pub size_bytes: i64,
    pub duration_ms: Option<i64>,
    /// `4K`, `1080p`, `720p`, `SD` from the probed stream, else as written
    /// in the name.
    pub resolution: Option<String>,
    /// HDR format as written in the name (`HDR10`, `DV`).
    pub hdr: Option<String>,
    /// Edition as written in the name (`Director's Cut`, `Extended`).
    pub edition: Option<String>,
    /// Release source as written (`BluRay`, `WEB-DL`).
    pub source: Option<String>,
    /// The version the item plays.
    pub is_default: bool,
}

impl Version {
    /// Short label: resolution, HDR and edition (`4K · HDR10 · Director's
    /// Cut`), else the release source, else the file name.
    #[must_use]
    pub fn label(&self) -> String {
        let mut parts: Vec<&str> = [&self.resolution, &self.hdr, &self.edition]
            .into_iter()
            .filter_map(|v| v.as_deref())
            .collect();
        if parts.is_empty()
            && let Some(s) = self.source.as_deref()
        {
            parts.push(s);
        }
        if parts.is_empty() {
            return self
                .rel_path
                .rsplit('/')
                .next()
                .unwrap_or(&self.rel_path)
                .to_owned();
        }
        parts.join(" · ")
    }

    /// Height class for ordering: probed height, else from the label.
    fn rank(&self) -> u32 {
        match self.resolution.as_deref() {
            Some("4K") => 4,
            Some("1080p") => 3,
            Some("720p") => 2,
            Some("SD") => 1,
            _ => 0,
        }
    }
}

/// Resolution label from the probed size, else from the name.
fn resolution(width: Option<i64>, height: Option<i64>, written: Option<String>) -> Option<String> {
    let (w, h) = (width.unwrap_or(0), height.unwrap_or(0));
    let probed = if w >= 3200 || h >= 1800 {
        Some("4K")
    } else if w >= 1600 || h >= 900 {
        Some("1080p")
    } else if w >= 1200 || h >= 700 {
        Some("720p")
    } else if w > 0 || h > 0 {
        Some("SD")
    } else {
        None
    };
    if let Some(p) = probed {
        return Some(p.to_owned());
    }
    let written = written?.trim().to_owned();
    Some(match written.to_ascii_lowercase().as_str() {
        "2160p" | "4k" | "uhd" | "4k uhd" => "4K".to_owned(),
        "1080p" | "1080i" => "1080p".to_owned(),
        "720p" => "720p".to_owned(),
        "480p" | "576p" | "sd" => "SD".to_owned(),
        _ if written.is_empty() => return None,
        _ => written,
    })
}

/// Present files of a work's live matches that are not extras, with what
/// the probe and the parser say about them.
const VERSIONS_SQL: &str = concat!(
    "
SELECT f.id, f.source_id, s.uri, f.path, f.size_bytes, f.duration_ms,
       json_extract(f.video_json, '$.width'), json_extract(f.video_json, '$.height'),
       json_extract(pr.hypotheses_json, '$[0].parsed.resolution'),
       json_extract(pr.hypotheses_json, '$[0].parsed.hdr'),
       json_extract(pr.hypotheses_json, '$[0].parsed.edition'),
       json_extract(pr.hypotheses_json, '$[0].parsed.source'),
       ",
    crate::items::version_key!(),
    "
FROM matches m
JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
JOIN sources s ON s.id = f.source_id
LEFT JOIN parse_results pr ON pr.file_id = f.id
WHERE m.state <> 'rejected'
  AND NOT EXISTS (SELECT 1 FROM file_group_members gm
                  WHERE gm.file_id = f.id AND gm.role IN ('extra', 'sample'))"
);

fn text(r: &Row<'_>, i: usize) -> pa_library::rusqlite::Result<Option<String>> {
    Ok(r.get::<_, Option<String>>(i)?
        .map(|s| s.trim().to_owned())
        .filter(|s| !s.is_empty()))
}

fn version_row(r: &Row<'_>) -> pa_library::rusqlite::Result<(Option<String>, Version)> {
    let uri: String = r.get(2)?;
    let rel = items::relative(&r.get::<_, String>(3)?);
    Ok((
        r.get(12)?,
        Version {
            file_id: r.get(0)?,
            source_id: r.get(1)?,
            local_path: items::local_path(&uri, &rel),
            rel_path: rel,
            size_bytes: r.get(4)?,
            duration_ms: r.get(5)?,
            resolution: resolution(r.get(6)?, r.get(7)?, text(r, 8)?),
            hdr: text(r, 9)?,
            edition: text(r, 10)?,
            source: text(r, 11)?,
            is_default: false,
        },
    ))
}

/// Versions of a movie (`Movie`) or an episode, best resolution first.
fn versions(conn: &Connection, item: ItemRef) -> Result<Vec<Version>, Error> {
    let (filter, id, default) = match item {
        ItemRef::Movie(w) => (
            "AND m.work_id = ?1",
            w,
            lists::default_version(conn, PROFILE, w)?,
        ),
        ItemRef::Episode(e) => (
            "AND m.id IN (SELECT me.match_id FROM match_episodes me WHERE me.episode_id = ?1)",
            e,
            None,
        ),
        other => {
            return Err(Error::Invalid(format!(
                "versions of a movie or episode, not {other}"
            )));
        }
    };
    let mut stmt = conn.prepare_cached(&format!("{VERSIONS_SQL} {filter} ORDER BY f.id"))?;
    let rows = stmt.query_map([id], version_row)?;
    let mut seen = std::collections::HashSet::new();
    let mut out: Vec<Version> = Vec::new();
    for row in rows {
        let (key, v) = row?;
        // Later parts of a split file and further files of a disc.
        if let Some(key) = key
            && seen.insert(key)
        {
            out.push(v);
        }
    }
    // The item plays the default version, else its first file.
    let chosen = default
        .filter(|d| out.iter().any(|v| v.file_id == *d))
        .or_else(|| out.first().map(|v| v.file_id));
    for v in &mut out {
        v.is_default = Some(v.file_id) == chosen;
    }
    out.sort_by(|a, b| b.rank().cmp(&a.rank()).then(a.file_id.cmp(&b.file_id)));
    Ok(out)
}

/// A collection as the app lists it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CollectionInfo {
    pub id: i64,
    pub name: String,
    pub kind: CollectionKind,
    /// `tmdb:collection:<id>` for a TMDB collection.
    pub external_ref: Option<String>,
    /// Entries, including those whose files are missing now.
    pub item_count: i64,
}

impl From<lists::Collection> for CollectionInfo {
    fn from(c: lists::Collection) -> Self {
        Self {
            id: c.id,
            name: c.name,
            kind: c.kind,
            external_ref: c.external_ref,
            item_count: c.item_count,
        }
    }
}

/// A video playlist.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PlaylistInfo {
    pub id: i64,
    pub name: String,
    /// Entries, including those whose files are missing now.
    pub item_count: i64,
    pub updated_at_ms: i64,
}

impl From<lists::Playlist> for PlaylistInfo {
    fn from(p: lists::Playlist) -> Self {
        Self {
            id: p.id,
            name: p.name,
            item_count: p.item_count,
            updated_at_ms: p.updated_at_ms,
        }
    }
}

/// What lists store for `item`: the work of a movie or show, the media key
/// of an episode or track.
fn entry_of(conn: &Connection, item: ItemRef) -> Result<Option<Entry>, Error> {
    Ok(match item {
        ItemRef::Movie(w) | ItemRef::Show(w) => Some(Entry::Work(w)),
        ItemRef::Episode(_) | ItemRef::Track(_) => {
            key_of(conn, item)?.map(|(k, _)| Entry::Media(k.to_string()))
        }
    })
}

fn require_entry(conn: &Connection, item: ItemRef) -> Result<Entry, Error> {
    entry_of(conn, item)?
        .ok_or_else(|| Error::Invalid(format!("{item} has no stable key yet (not hashed)")))
}

/// The item a media key belongs to.
fn item_of_media(conn: &Connection, key: &str) -> Result<Option<ItemRef>, Error> {
    match key.parse::<MediaKey>() {
        Ok(k) => item_of_key(conn, &k),
        Err(_) => Ok(None),
    }
}

/// The present item of a list entry, `None` if it is gone or its files are
/// missing.
fn item_of_entry(conn: &Connection, entry: &Entry) -> Result<Option<Item>, Error> {
    let id = match entry {
        Entry::Work(w) => {
            let kind: Option<String> = conn
                .prepare_cached("SELECT kind FROM works WHERE id = ?1")?
                .query_row([w], |r| r.get(0))
                .optional()?;
            match kind.as_deref() {
                Some("movie") => ItemRef::Movie(*w),
                Some(_) => ItemRef::Show(*w),
                None => return Ok(None),
            }
        }
        Entry::Media(k) => match item_of_media(conn, k)? {
            Some(id) => id,
            None => return Ok(None),
        },
    };
    items::item(conn, id)
}

/// Present items of `entries`, in order, each once.
fn items_of(conn: &Connection, entries: &[Entry]) -> Result<Vec<Item>, Error> {
    let mut seen = std::collections::HashSet::new();
    let mut out = Vec::new();
    for e in entries {
        if let Some(item) = item_of_entry(conn, e)?
            && seen.insert(item.id)
        {
            out.push(item);
        }
    }
    Ok(out)
}

/// Media keys a video playlist gets for `item`: a movie or an episode, or
/// every episode of a show in order.
fn playlist_keys(conn: &Connection, item: ItemRef) -> Result<Vec<String>, Error> {
    let targets = match item {
        ItemRef::Movie(_) | ItemRef::Episode(_) => vec![item],
        ItemRef::Show(w) => items::episodes(conn, w)?
            .into_iter()
            .map(|e| e.id)
            .collect(),
        ItemRef::Track(_) => {
            return Err(Error::Invalid(format!(
                "{item}: video playlists hold movies and episodes"
            )));
        }
    };
    let mut keys = Vec::new();
    for t in targets {
        if let Some((k, _)) = key_of(conn, t)? {
            keys.push(k.to_string());
        }
    }
    if keys.is_empty() {
        return Err(Error::Invalid(format!(
            "{item} has no stable key yet (not hashed)"
        )));
    }
    Ok(keys)
}

fn require_playlist(conn: &Connection, id: i64) -> Result<lists::Playlist, Error> {
    match lists::playlist(conn, id)? {
        Some(p) if p.profile_id == PROFILE && p.kind == PlaylistKind::Video => Ok(p),
        _ => Err(Error::NotFound(format!("playlist {id}"))),
    }
}

fn require_collection(conn: &Connection, id: i64) -> Result<lists::Collection, Error> {
    match lists::collection(conn, id)? {
        Some(c) if c.profile_id.is_none_or(|p| p == PROFILE) => Ok(c),
        _ => Err(Error::NotFound(format!("collection {id}"))),
    }
}

impl Catalog {
    // -- versions ------------------------------------------------------------

    /// Versions of a movie or episode item, best resolution first; the one
    /// the item plays has `is_default`.
    pub fn versions(&self, item: ItemRef) -> Result<Vec<Version>, Error> {
        versions(&self.lock(), item)
    }

    /// Makes `file_id` (a [`Version::file_id`] of the movie) its default
    /// version, or goes back to the first file (`None`).
    pub fn set_default_version(&self, item: ItemRef, file_id: Option<i64>) -> Result<(), Error> {
        let ItemRef::Movie(work) = item else {
            return Err(Error::Invalid(format!(
                "default versions are per movie, not {item}"
            )));
        };
        let conn = self.lock();
        if let Some(f) = file_id
            && !versions(&conn, item)?.iter().any(|v| v.file_id == f)
        {
            return Err(Error::NotFound(format!("version {f} of {item}")));
        }
        lists::set_default_version(&conn, PROFILE, work, file_id, now_ms())?;
        Ok(())
    }

    // -- favorites -----------------------------------------------------------

    /// Favorite items, most recently added first; `pinned_only`: those shown
    /// on Home. Favorites whose files are missing are left out.
    pub fn favorites(&self, pinned_only: bool) -> Result<Vec<Item>, Error> {
        let conn = self.lock();
        let entries: Vec<Entry> = lists::favorites(&conn, PROFILE, pinned_only)?
            .into_iter()
            .map(|f| f.entry)
            .collect();
        items_of(&conn, &entries)
    }

    /// `true` if `item` is a favorite.
    pub fn is_favorite(&self, item: ItemRef) -> Result<bool, Error> {
        let conn = self.lock();
        match entry_of(&conn, item)? {
            Some(e) => Ok(lists::is_favorite(&conn, PROFILE, &e)?),
            None => Ok(false),
        }
    }

    /// Adds `item` to the favorites (`on`) or removes it. Returns whether
    /// anything changed.
    pub fn set_favorite(&self, item: ItemRef, on: bool) -> Result<bool, Error> {
        let conn = self.lock();
        let entry = require_entry(&conn, item)?;
        Ok(if on {
            lists::add_favorite(&conn, PROFILE, &entry, now_ms())?
        } else {
            lists::remove_favorite(&conn, PROFILE, &entry)?
        })
    }

    /// Shows a favorite in the Favorites row of Home or hides it there;
    /// `false` if `item` is not a favorite.
    pub fn set_favorite_pinned(&self, item: ItemRef, pinned: bool) -> Result<bool, Error> {
        let conn = self.lock();
        let entry = require_entry(&conn, item)?;
        Ok(lists::set_favorite_pinned(&conn, PROFILE, &entry, pinned)?)
    }

    // -- collections ---------------------------------------------------------

    /// The user's collections and the TMDB ones, by name.
    pub fn collections(&self) -> Result<Vec<CollectionInfo>, Error> {
        Ok(lists::collections(&self.lock(), PROFILE)?
            .into_iter()
            .filter(|c| c.kind != CollectionKind::Smart)
            .map(Into::into)
            .collect())
    }

    /// Collections that hold `item` (a movie or show: its work).
    pub fn item_collections(&self, item: ItemRef) -> Result<Vec<CollectionInfo>, Error> {
        let conn = self.lock();
        let Some(entry) = entry_of(&conn, item)? else {
            return Ok(Vec::new());
        };
        Ok(lists::collections_of(&conn, PROFILE, &entry)?
            .into_iter()
            .map(Into::into)
            .collect())
    }

    /// Present items of a collection in order.
    pub fn collection_items(&self, id: i64) -> Result<Vec<Item>, Error> {
        let conn = self.lock();
        require_collection(&conn, id)?;
        let entries = lists::collection_entries(&conn, id)?;
        items_of(&conn, &entries)
    }

    /// Creates an empty collection of the user.
    pub fn create_collection(&self, name: &str) -> Result<CollectionInfo, Error> {
        Ok(lists::create_collection(&self.lock(), Some(PROFILE), name, now_ms())?.into())
    }

    pub fn rename_collection(&self, id: i64, name: &str) -> Result<(), Error> {
        let conn = self.lock();
        require_collection(&conn, id)?;
        lists::rename_collection(&conn, id, name, now_ms())?;
        Ok(())
    }

    pub fn delete_collection(&self, id: i64) -> Result<(), Error> {
        let conn = self.lock();
        require_collection(&conn, id)?;
        lists::delete_collection(&conn, id)?;
        Ok(())
    }

    /// Appends `item` to a collection; `false` if it is already there.
    pub fn add_to_collection(&self, id: i64, item: ItemRef) -> Result<bool, Error> {
        let conn = self.lock();
        require_collection(&conn, id)?;
        let entry = require_entry(&conn, item)?;
        Ok(lists::add_to_collection(&conn, id, &entry, now_ms())?)
    }

    /// Removes `item` from a collection; `false` if it was not there.
    pub fn remove_from_collection(&self, id: i64, item: ItemRef) -> Result<bool, Error> {
        let conn = self.lock();
        require_collection(&conn, id)?;
        let entry = require_entry(&conn, item)?;
        Ok(lists::remove_from_collection(&conn, id, &entry, now_ms())?)
    }

    // -- playlists -----------------------------------------------------------

    /// Video playlists, most recently changed first.
    pub fn playlists(&self) -> Result<Vec<PlaylistInfo>, Error> {
        Ok(
            lists::playlists(&self.lock(), PROFILE, Some(PlaylistKind::Video))?
                .into_iter()
                .map(Into::into)
                .collect(),
        )
    }

    /// Creates an empty video playlist.
    pub fn create_playlist(&self, name: &str) -> Result<PlaylistInfo, Error> {
        Ok(lists::create_playlist(&self.lock(), PROFILE, name, PlaylistKind::Video, now_ms())?.into())
    }

    pub fn rename_playlist(&self, id: i64, name: &str) -> Result<(), Error> {
        let conn = self.lock();
        require_playlist(&conn, id)?;
        lists::rename_playlist(&conn, id, name, now_ms())?;
        Ok(())
    }

    pub fn delete_playlist(&self, id: i64) -> Result<(), Error> {
        let conn = self.lock();
        require_playlist(&conn, id)?;
        lists::delete_playlist(&conn, id)?;
        Ok(())
    }

    /// Present items of a playlist in order.
    pub fn playlist_items(&self, id: i64) -> Result<Vec<Item>, Error> {
        let conn = self.lock();
        require_playlist(&conn, id)?;
        let entries: Vec<Entry> = lists::playlist_items(&conn, id)?
            .into_iter()
            .map(|i| Entry::Media(i.media_key))
            .collect();
        items_of(&conn, &entries)
    }

    /// Appends a movie, an episode or (every episode of) a show to a
    /// playlist; items already there are skipped. Returns how many were
    /// added.
    pub fn add_to_playlist(&self, id: i64, item: ItemRef) -> Result<usize, Error> {
        let conn = self.lock();
        require_playlist(&conn, id)?;
        let now = now_ms();
        let mut added = 0;
        for key in playlist_keys(&conn, item)? {
            if lists::add_to_playlist(&conn, id, &key, now)? {
                added += 1;
            }
        }
        Ok(added)
    }

    /// Removes a movie or episode (or every episode of a show) from a
    /// playlist; returns how many entries went.
    pub fn remove_from_playlist(&self, id: i64, item: ItemRef) -> Result<usize, Error> {
        let conn = self.lock();
        require_playlist(&conn, id)?;
        let now = now_ms();
        let mut removed = 0;
        for key in playlist_keys(&conn, item)? {
            if lists::remove_from_playlist(&conn, id, &key, now)? {
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Puts the items of a playlist in the order of `items`; others keep
    /// their order after them.
    pub fn reorder_playlist(&self, id: i64, items: &[ItemRef]) -> Result<(), Error> {
        let mut conn = self.lock();
        require_playlist(&conn, id)?;
        let stored = lists::playlist_items(&conn, id)?;
        let mut order = Vec::new();
        for item in items {
            if let Some((key, _)) = key_of(&conn, *item)? {
                let key = key.to_string();
                order.extend(stored.iter().filter(|s| s.media_key == key).map(|s| s.id));
            }
        }
        lists::reorder_playlist(&mut conn, id, &order, now_ms())?;
        Ok(())
    }
}

/// Links a movie to its TMDB collection (or removes a link it lost); run
/// by the metadata write in the same transaction.
pub(crate) fn link_tmdb_collection(
    conn: &Connection,
    work: i64,
    collection: Option<(u64, &str)>,
    now_ms: i64,
) -> Result<(), Error> {
    const PREFIX: &str = "tmdb:collection:";
    match collection {
        Some((id, name)) if !name.trim().is_empty() => {
            let r = format!("{PREFIX}{id}");
            lists::unlink_external_collections(conn, PREFIX, work, Some(&r))?;
            lists::link_external_collection(conn, &r, name, work, now_ms)?;
        }
        Some(_) => {}
        None => lists::unlink_external_collections(conn, PREFIX, work, None)?,
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn v(resolution: Option<&str>, edition: Option<&str>, source: Option<&str>) -> Version {
        Version {
            file_id: 1,
            source_id: 1,
            rel_path: "Movies/Dune.mkv".into(),
            local_path: None,
            size_bytes: 0,
            duration_ms: None,
            resolution: resolution.map(Into::into),
            hdr: None,
            edition: edition.map(Into::into),
            source: source.map(Into::into),
            is_default: false,
        }
    }

    #[test]
    fn labels_name_resolution_edition_or_the_file() {
        assert_eq!(
            v(Some("4K"), Some("Director's Cut"), Some("BluRay")).label(),
            "4K · Director's Cut"
        );
        assert_eq!(v(None, None, Some("WEB-DL")).label(), "WEB-DL");
        assert_eq!(v(None, None, None).label(), "Dune.mkv");
    }

    #[test]
    fn resolution_prefers_the_probe() {
        assert_eq!(resolution(Some(3840), Some(1600), None).as_deref(), Some("4K"));
        assert_eq!(resolution(Some(1920), Some(800), None).as_deref(), Some("1080p"));
        assert_eq!(
            resolution(Some(1280), Some(720), Some("2160p".into())).as_deref(),
            Some("720p")
        );
        assert_eq!(resolution(None, None, Some("2160p".into())).as_deref(), Some("4K"));
        assert_eq!(resolution(None, None, Some("1080p".into())).as_deref(), Some("1080p"));
        assert_eq!(resolution(None, None, None), None);
    }
}
