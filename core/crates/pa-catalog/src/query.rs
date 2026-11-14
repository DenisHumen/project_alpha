// SPDX-License-Identifier: MPL-2.0
//! Library queries (LIB-009): combinable filters, sorts, paging, the
//! alphabet index, facets for filter chips, folder browsing with "folders
//! first", per-view preferences and smart shelves.
//!
//! A [`LibraryQuery`] lists movies or shows. Filters combine with AND
//! between fields and OR inside a field (`genres: [Drama, Comedy]` is
//! "Drama or Comedy"); an empty field does not filter. File-level filters
//! (resolution, HDR, audio, source) match a work if any of its present
//! files matches. Video and audio filters read the LIB-006 probe columns
//! (`video_json`, `audio_langs`, `audio_formats`); unprobed files match
//! none of them.
//!
//! Watch state is the default profile's: a movie is watched if any of its
//! keys (TMDB or file) is watched, in progress if it has a position; a show
//! is watched when every present episode is, in progress when some are
//! watched or started.
//!
//! Titles sort by [`sort_key`]: case-insensitive for every script, `ё` as
//! `е`, leading English articles and punctuation skipped, numbers in
//! natural order (`Part 2` before `Part 10`). SQLite selects the matching
//! works; they are ordered here, so the key works for every script without
//! an ICU build of SQLite.

use std::cmp::Ordering;
use std::collections::BTreeMap;

use pa_library::rusqlite::Connection;
use pa_library::views;
use serde::{Deserialize, Serialize};

use crate::index::Index;
use crate::items::{self, Item, ItemKind, ItemRef};
use crate::progress::PROFILE;
use crate::{Catalog, Error, now_ms};

/// Watch state of a work for the default profile.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum WatchState {
    Unwatched,
    InProgress,
    Watched,
}

/// Resolution class of the main video stream, by coded size (scope films
/// count by width: 1920×800 is Full HD).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Resolution {
    /// Below 1200×700.
    Sd,
    /// 720p: from 1200 wide or 700 high.
    Hd,
    /// 1080p: from 1600 wide or 900 high.
    FullHd,
    /// 4K: from 3200 wide or 1800 high.
    Uhd,
}

impl Resolution {
    /// All classes, lowest first.
    pub const ALL: [Self; 4] = [Self::Sd, Self::Hd, Self::FullHd, Self::Uhd];

    /// Slug, as in JSON and SQL.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Sd => "sd",
            Self::Hd => "hd",
            Self::FullHd => "full_hd",
            Self::Uhd => "uhd",
        }
    }

    pub(crate) fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

/// Dynamic range filter.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HdrFilter {
    /// Probed video without HDR.
    Sdr,
    /// Any HDR format.
    AnyHdr,
    Hdr10,
    Hdr10Plus,
    DolbyVision,
    Hlg,
}

impl HdrFilter {
    /// Every value, for facets.
    pub const ALL: [Self; 6] = [
        Self::Sdr,
        Self::AnyHdr,
        Self::Hdr10,
        Self::Hdr10Plus,
        Self::DolbyVision,
        Self::Hlg,
    ];
}

/// Filters of a query. Fields combine with AND, values inside a field with
/// OR; empty fields do not filter.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct LibraryFilter {
    /// Genre names (case-insensitive), any of.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub genres: Vec<String>,
    /// Release year from (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year_min: Option<i64>,
    /// Release year to (inclusive).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub year_max: Option<i64>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub watched: Vec<WatchState>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub resolutions: Vec<Resolution>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub hdr: Vec<HdrFilter>,
    /// Audio format slugs (`truehd`, `atmos`, `dts_hd_ma`, `aac`…), any of.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub audio_formats: Vec<String>,
    /// Audio languages as stored by the probe (ISO 639-2, `rus`, `eng`).
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub audio_languages: Vec<String>,
    /// Source ids, any of.
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<i64>,
}

impl LibraryFilter {
    /// `true` if nothing is filtered.
    pub fn is_empty(&self) -> bool {
        *self == Self::default()
    }
}

/// Sort field of a query.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SortField {
    /// By [`sort_key`] of the title.
    #[default]
    Title,
    /// Release year.
    Year,
    /// Newest present file.
    Added,
    /// File duration (shows: episode runtime).
    Duration,
    /// Last playback of the default profile.
    LastWatched,
}

impl SortField {
    /// Every field.
    pub const ALL: [Self; 5] = [
        Self::Title,
        Self::Year,
        Self::Added,
        Self::Duration,
        Self::LastWatched,
    ];

    /// Slug, as stored in view preferences.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Title => "title",
            Self::Year => "year",
            Self::Added => "added",
            Self::Duration => "duration",
            Self::LastWatched => "last_watched",
        }
    }

    /// Field of a slug; unknown slugs (a newer build) sort by title.
    pub fn from_slug(s: &str) -> Self {
        Self::ALL
            .into_iter()
            .find(|f| f.as_str() == s)
            .unwrap_or_default()
    }
}

/// Sort of a query. Ties fall back to title (A→Z), year and id; missing
/// values go last in both directions.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(default)]
pub struct LibrarySort {
    pub field: SortField,
    pub descending: bool,
}

/// What to list: movies or shows, filtered and sorted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct LibraryQuery {
    /// [`ItemKind::Movie`] or [`ItemKind::Show`].
    pub kind: ItemKind,
    #[serde(default)]
    pub filter: LibraryFilter,
    #[serde(default)]
    pub sort: LibrarySort,
}

impl LibraryQuery {
    /// All items of `kind` by title.
    pub fn new(kind: ItemKind) -> Self {
        Self {
            kind,
            filter: LibraryFilter::default(),
            sort: LibrarySort::default(),
        }
    }

    /// The query as JSON (smart shelves, view filters).
    pub fn to_json(&self) -> String {
        serde_json::to_string(self).unwrap_or_else(|_| "{}".to_owned())
    }

    /// A query from [`to_json`](Self::to_json).
    pub fn from_json(json: &str) -> Result<Self, Error> {
        serde_json::from_str(json).map_err(|e| Error::Invalid(format!("query: {e}")))
    }
}

/// One page of a query.
#[derive(Debug, Clone, PartialEq)]
pub struct ItemPage {
    pub items: Vec<Item>,
    /// Items matching the query in total (all pages).
    pub total: usize,
    pub offset: usize,
}

/// One letter of the alphabet index.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AlphabetEntry {
    /// Upper-case letter, or `#` for digits and symbols.
    pub letter: String,
    /// Position of the first item with this letter in the title-sorted
    /// query.
    pub offset: usize,
    pub count: usize,
}

/// A value of a filter with the number of items that have it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FacetValue<T> {
    pub value: T,
    pub count: usize,
}

/// Filter values present in the library, for filter chips. Counts are of
/// items of the kind without any filter.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Facets {
    /// By name.
    pub genres: Vec<FacetValue<String>>,
    pub year_min: Option<i64>,
    pub year_max: Option<i64>,
    pub resolutions: Vec<FacetValue<Resolution>>,
    pub hdr: Vec<FacetValue<HdrFilter>>,
    /// Most common first.
    pub audio_formats: Vec<FacetValue<String>>,
    /// Most common first.
    pub audio_languages: Vec<FacetValue<String>>,
    /// By source id.
    pub sources: Vec<FacetValue<i64>>,
}

/// Saved presentation of a view (LIB-009): its own sort, "folders first"
/// and, optionally, its last filter.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ViewPrefs {
    /// View key (`movies`, `shows`, `folder:<source>`…), 1–128 characters.
    pub view: String,
    pub sort: LibrarySort,
    pub folders_first: bool,
    pub filter: Option<LibraryFilter>,
}

/// A smart shelf: a saved query, optionally pinned to Home.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SmartShelf {
    pub id: i64,
    pub name: String,
    pub query: LibraryQuery,
    pub pinned_home: bool,
    pub position: i64,
}

/// An entry of a folder listing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FolderEntry {
    /// File or folder name.
    pub name: String,
    /// Path relative to the source root, without a leading slash.
    pub rel_path: String,
    pub is_folder: bool,
    /// Files: the movie, episode or track of the file, if recognized.
    pub item: Option<ItemRef>,
    /// Files: size; folders: total size of the media files inside.
    pub size_bytes: i64,
    /// Folders: media files inside, at any depth.
    pub file_count: usize,
    /// Files: when added; folders: the newest file inside.
    pub added_at_ms: i64,
    pub duration_ms: Option<i64>,
}

// ---------------------------------------------------------------------------
// sort key

/// Sort key of a title: lower case in every script, `ё` → `е`, leading
/// punctuation and English articles (`the`, `a`, `an`) skipped, digit runs
/// zero-padded to 10 digits so numbers sort naturally.
pub fn sort_key(title: &str) -> String {
    let lower: String = title
        .trim_start_matches(|c: char| !c.is_alphanumeric())
        .chars()
        .flat_map(char::to_lowercase)
        .map(|c| if c == 'ё' { 'е' } else { c })
        .collect();
    let mut rest = lower.as_str();
    for article in ["the ", "a ", "an "] {
        if let Some(after) = rest.strip_prefix(article) {
            let after = after.trim_start_matches(|c: char| !c.is_alphanumeric());
            if !after.is_empty() {
                rest = after;
            }
            break;
        }
    }
    let mut out = String::with_capacity(rest.len() + 8);
    let mut digits = String::new();
    let flush = |digits: &mut String, out: &mut String| {
        if !digits.is_empty() {
            let trimmed = digits.trim_start_matches('0');
            for _ in trimmed.len()..10 {
                out.push('0');
            }
            out.push_str(trimmed);
            digits.clear();
        }
    };
    for c in rest.chars() {
        if c.is_ascii_digit() {
            digits.push(c);
        } else {
            flush(&mut digits, &mut out);
            out.push(c);
        }
    }
    flush(&mut digits, &mut out);
    out
}

/// Letter of the alphabet index for a [`sort_key`].
fn index_letter(key: &str) -> String {
    match key.chars().next() {
        Some(c) if c.is_alphabetic() => c.to_uppercase().collect(),
        _ => "#".to_owned(),
    }
}

// ---------------------------------------------------------------------------
// queries

fn check_kind(query: &LibraryQuery) -> Result<(), Error> {
    match query.kind {
        ItemKind::Movie | ItemKind::Show => Ok(()),
        other => Err(Error::Invalid(format!(
            "queries list movies or shows, not {other:?}"
        ))),
    }
}

pub(crate) fn run_query(
    conn: &Connection,
    index: &Index,
    query: &LibraryQuery,
    offset: usize,
    limit: usize,
) -> Result<ItemPage, Error> {
    check_kind(query)?;
    let hits = index.select(query);
    let page: Vec<i64> = hits
        .iter()
        .skip(offset)
        .take(limit)
        .map(|(id, _)| *id)
        .collect();
    Ok(ItemPage {
        items: items::works_by_ids(conn, query.kind, &page)?,
        total: hits.len(),
        offset,
    })
}

pub(crate) fn alphabet(index: &Index, query: &LibraryQuery) -> Result<Vec<AlphabetEntry>, Error> {
    check_kind(query)?;
    let title_query = LibraryQuery {
        kind: query.kind,
        filter: query.filter.clone(),
        sort: LibrarySort {
            field: SortField::Title,
            descending: query.sort.field == SortField::Title && query.sort.descending,
        },
    };
    let mut out: Vec<AlphabetEntry> = Vec::new();
    for (n, (_, key)) in index.select(&title_query).iter().enumerate() {
        let letter = index_letter(key);
        match out.last_mut() {
            Some(last) if last.letter == letter => last.count += 1,
            _ => out.push(AlphabetEntry {
                letter,
                offset: n,
                count: 1,
            }),
        }
    }
    Ok(out)
}

/// Normalizes a folder path: no leading or trailing slashes.
fn clean_dir(dir: &str) -> String {
    dir.trim_matches('/').to_owned()
}

pub(crate) fn browse(
    conn: &Connection,
    source_id: i64,
    dir: &str,
    sort: LibrarySort,
    folders_first: bool,
) -> Result<Vec<FolderEntry>, Error> {
    let dir = clean_dir(dir);
    // `files.path` is `/dir/name`; children of `/dir` lie in (`/dir/`, `/dir0`).
    let (lo, hi) = if dir.is_empty() {
        ("/".to_owned(), "0".to_owned())
    } else {
        (format!("/{dir}/"), format!("/{dir}0"))
    };
    let mut stmt = conn.prepare_cached(
        "SELECT f.path, f.size_bytes, f.added_at_ms, f.duration_ms,
                (SELECT CASE WHEN w.kind = 'movie' THEN 'm' || w.id END
                   FROM matches m JOIN works w ON w.id = m.work_id
                  WHERE m.file_id = f.id AND m.state <> 'rejected'),
                (SELECT 'e' || min(me.episode_id)
                   FROM matches m JOIN match_episodes me ON me.match_id = m.id
                  WHERE m.file_id = f.id AND m.state <> 'rejected'),
                (SELECT 't' || t.id FROM music_tracks t WHERE t.file_id = f.id)
         FROM files f
         WHERE f.source_id = ?1 AND f.path > ?2 AND f.path < ?3 AND f.missing_since_ms IS NULL",
    )?;
    let mut folders: BTreeMap<String, FolderEntry> = BTreeMap::new();
    let mut files = Vec::new();
    let mut rows = stmt.query(pa_library::rusqlite::params![source_id, lo, hi])?;
    while let Some(r) = rows.next()? {
        let path: String = r.get(0)?;
        let size: i64 = r.get(1)?;
        let added: i64 = r.get(2)?;
        let rest = &path[lo.len()..];
        if let Some((name, _)) = rest.split_once('/') {
            let e = folders
                .entry(name.to_owned())
                .or_insert_with(|| FolderEntry {
                    name: name.to_owned(),
                    rel_path: if dir.is_empty() {
                        name.to_owned()
                    } else {
                        format!("{dir}/{name}")
                    },
                    is_folder: true,
                    item: None,
                    size_bytes: 0,
                    file_count: 0,
                    added_at_ms: 0,
                    duration_ms: None,
                });
            e.size_bytes += size;
            e.file_count += 1;
            e.added_at_ms = e.added_at_ms.max(added);
        } else {
            let item = [r.get::<_, Option<String>>(4)?, r.get(5)?, r.get(6)?]
                .into_iter()
                .flatten()
                .find_map(|id| id.parse::<ItemRef>().ok());
            files.push(FolderEntry {
                name: rest.to_owned(),
                rel_path: path.trim_start_matches('/').to_owned(),
                is_folder: false,
                item,
                size_bytes: size,
                file_count: 1,
                added_at_ms: added,
                duration_ms: r.get(3)?,
            });
        }
    }
    let mut keyed: Vec<(String, FolderEntry)> = folders
        .into_values()
        .chain(files)
        .map(|e| (sort_key(&e.name), e))
        .collect();
    keyed.sort_by(|(ka, a), (kb, b)| {
        let group = if folders_first {
            b.is_folder.cmp(&a.is_folder)
        } else {
            Ordering::Equal
        };
        let by_field = match sort.field {
            SortField::Added => a.added_at_ms.cmp(&b.added_at_ms),
            SortField::Duration => match (a.duration_ms, b.duration_ms) {
                (Some(x), Some(y)) => x.cmp(&y),
                // Missing values last in both directions.
                (Some(_), None) => return group.then(Ordering::Less),
                (None, Some(_)) => return group.then(Ordering::Greater),
                (None, None) => Ordering::Equal,
            },
            _ => Ordering::Equal,
        };
        let by_name = ka.cmp(kb).then_with(|| a.name.cmp(&b.name));
        let ordered = match (sort.field, sort.descending) {
            (SortField::Title, true) => by_name.reverse(),
            (SortField::Added | SortField::Duration, true) => by_field.reverse().then(by_name),
            _ => by_field.then(by_name),
        };
        group.then(ordered)
    });
    Ok(keyed.into_iter().map(|(_, e)| e).collect())
}

// ---------------------------------------------------------------------------
// Catalog

fn shelf_of(s: views::SmartShelf) -> Result<SmartShelf, Error> {
    Ok(SmartShelf {
        id: s.id,
        name: s.name,
        query: LibraryQuery::from_json(&s.query_json)?,
        pinned_home: s.pinned_home,
        position: s.position,
    })
}

impl Catalog {
    /// One page of a query: `limit` items from `offset`, and the total.
    pub fn query(
        &self,
        query: &LibraryQuery,
        offset: usize,
        limit: usize,
    ) -> Result<ItemPage, Error> {
        let conn = self.lock();
        let index = self.index(&conn)?;
        run_query(&conn, &index, query, offset, limit)
    }

    /// Alphabet index of a query in title order (descending if the query
    /// sorts by title descending): letters in list order with the offset of
    /// their first item.
    pub fn alphabet_index(&self, query: &LibraryQuery) -> Result<Vec<AlphabetEntry>, Error> {
        let conn = self.lock();
        let index = self.index(&conn)?;
        alphabet(&index, query)
    }

    /// Filter values present among the movies or shows.
    pub fn facets(&self, kind: ItemKind) -> Result<Facets, Error> {
        check_kind(&LibraryQuery::new(kind))?;
        let conn = self.lock();
        Ok(self.index(&conn)?.facets(kind))
    }

    /// Entries of folder `dir` (relative to the source root, `""` for the
    /// root) of a source: subfolders and media files, sorted by `sort`
    /// (title = name), folders first if asked.
    pub fn browse(
        &self,
        source_id: i64,
        dir: &str,
        sort: LibrarySort,
        folders_first: bool,
    ) -> Result<Vec<FolderEntry>, Error> {
        browse(&self.lock(), source_id, dir, sort, folders_first)
    }

    /// Saved preferences of a view, or the defaults (title, ascending,
    /// folders first, no filter).
    pub fn view_prefs(&self, view: &str) -> Result<ViewPrefs, Error> {
        let conn = self.lock();
        let saved = views::view_prefs(&conn, PROFILE, view)?
            .unwrap_or_else(|| views::ViewPrefs::default_for(view));
        Ok(ViewPrefs {
            view: saved.view,
            sort: LibrarySort {
                field: SortField::from_slug(&saved.sort),
                descending: saved.descending,
            },
            folders_first: saved.folders_first,
            // A filter a newer build wrote and this one cannot read is dropped.
            filter: saved
                .filter_json
                .and_then(|j| serde_json::from_str(&j).ok()),
        })
    }

    /// Saves the preferences of `prefs.view`.
    pub fn set_view_prefs(&self, prefs: &ViewPrefs) -> Result<(), Error> {
        let filter_json = match &prefs.filter {
            Some(f) => Some(serde_json::to_string(f).map_err(|e| Error::Invalid(e.to_string()))?),
            None => None,
        };
        views::set_view_prefs(
            &self.lock(),
            PROFILE,
            &views::ViewPrefs {
                view: prefs.view.clone(),
                sort: prefs.sort.field.as_str().to_owned(),
                descending: prefs.sort.descending,
                folders_first: prefs.folders_first,
                filter_json,
                updated_at_ms: now_ms(),
            },
        )?;
        Ok(())
    }

    /// Smart shelves by position; with `pinned_only`, those on Home.
    /// Shelves whose query this build cannot read are skipped.
    pub fn shelves(&self, pinned_only: bool) -> Result<Vec<SmartShelf>, Error> {
        let rows = views::shelves(&self.lock(), PROFILE, pinned_only)?;
        Ok(rows.into_iter().filter_map(|s| shelf_of(s).ok()).collect())
    }

    /// Saves a new smart shelf at the end of the list.
    pub fn create_shelf(
        &self,
        name: &str,
        query: &LibraryQuery,
        pinned_home: bool,
    ) -> Result<SmartShelf, Error> {
        check_kind(query)?;
        let s = views::create_shelf(
            &self.lock(),
            PROFILE,
            name,
            &query.to_json(),
            pinned_home,
            now_ms(),
        )?;
        shelf_of(s)
    }

    /// Changes a shelf.
    pub fn update_shelf(
        &self,
        id: i64,
        name: &str,
        query: &LibraryQuery,
        pinned_home: bool,
    ) -> Result<(), Error> {
        check_kind(query)?;
        if !views::update_shelf(
            &self.lock(),
            id,
            name,
            &query.to_json(),
            pinned_home,
            now_ms(),
        )? {
            return Err(Error::NotFound(format!("shelf {id}")));
        }
        Ok(())
    }

    /// Deletes a shelf (no error if it is gone).
    pub fn delete_shelf(&self, id: i64) -> Result<(), Error> {
        views::delete_shelf(&self.lock(), id)?;
        Ok(())
    }

    /// Orders the shelves as `ids`; unlisted ones follow in their order.
    pub fn reorder_shelves(&self, ids: &[i64]) -> Result<(), Error> {
        views::reorder_shelves(&mut self.lock(), PROFILE, ids, now_ms())?;
        Ok(())
    }

    /// One page of a shelf's query.
    pub fn shelf_items(&self, id: i64, offset: usize, limit: usize) -> Result<ItemPage, Error> {
        let conn = self.lock();
        let shelf =
            views::shelf(&conn, id)?.ok_or_else(|| Error::NotFound(format!("shelf {id}")))?;
        let index = self.index(&conn)?;
        run_query(
            &conn,
            &index,
            &LibraryQuery::from_json(&shelf.query_json)?,
            offset,
            limit,
        )
    }

    /// Genres of a movie or show item.
    pub fn genres(&self, item: ItemRef) -> Result<Vec<String>, Error> {
        match item {
            ItemRef::Movie(w) | ItemRef::Show(w) => Ok(views::genres(&self.lock(), w)?),
            _ => Ok(Vec::new()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sort_keys() {
        assert_eq!(sort_key("The Matrix"), "matrix");
        assert_eq!(sort_key("A"), "a");
        assert_eq!(sort_key("«Ёлки»"), "елки»");
        assert!(sort_key("Part 2") < sort_key("Part 10"));
        assert!(sort_key("абв") < sort_key("Яблоко"));
        assert!(sort_key("Ёлки") < sort_key("Жара"));
        assert!(sort_key("zebra") < sort_key("Абв"));
        assert_eq!(index_letter(&sort_key("ёлки")), "Е");
        assert_eq!(index_letter(&sort_key("12 стульев")), "#");
        assert_eq!(index_letter(&sort_key("the office")), "O");
    }

    #[test]
    fn query_json_round_trips_and_tolerates_missing_fields() {
        let q = LibraryQuery {
            kind: ItemKind::Show,
            filter: LibraryFilter {
                genres: vec!["Drama".into()],
                year_min: Some(2000),
                watched: vec![WatchState::Unwatched],
                hdr: vec![HdrFilter::DolbyVision],
                ..LibraryFilter::default()
            },
            sort: LibrarySort {
                field: SortField::Added,
                descending: true,
            },
        };
        assert_eq!(LibraryQuery::from_json(&q.to_json()).unwrap(), q);
        let minimal = LibraryQuery::from_json(r#"{"kind":"movie"}"#).unwrap();
        assert_eq!(minimal, LibraryQuery::new(ItemKind::Movie));
        assert!(LibraryQuery::from_json("{").is_err());
        assert_eq!(SortField::from_slug("rating_v2"), SortField::Title);
    }
}
