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
//! natural order (`Part 2` before `Part 10`). The key is the SQL function
//! `pa_sort_key`, registered on the catalog connection.

use std::collections::BTreeMap;

use pa_library::rusqlite::functions::FunctionFlags;
use pa_library::rusqlite::types::Value;
use pa_library::rusqlite::{Connection, params_from_iter};
use pa_library::views;
use serde::{Deserialize, Serialize};

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

impl WatchState {
    fn code(self) -> i64 {
        match self {
            Self::Unwatched => 0,
            Self::InProgress => 1,
            Self::Watched => 2,
        }
    }
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

    fn from_slug(s: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|r| r.as_str() == s)
    }
}

/// Resolution class of `f.video_json`, `NULL` if unprobed or no video.
const RESOLUTION_SQL: &str = "(CASE
    WHEN f.video_json IS NULL THEN NULL
    WHEN ifnull(json_extract(f.video_json, '$.width'), 0) >= 3200
      OR ifnull(json_extract(f.video_json, '$.height'), 0) >= 1800 THEN 'uhd'
    WHEN ifnull(json_extract(f.video_json, '$.width'), 0) >= 1600
      OR ifnull(json_extract(f.video_json, '$.height'), 0) >= 900 THEN 'full_hd'
    WHEN ifnull(json_extract(f.video_json, '$.width'), 0) >= 1200
      OR ifnull(json_extract(f.video_json, '$.height'), 0) >= 700 THEN 'hd'
    WHEN ifnull(json_extract(f.video_json, '$.width'), 0) > 0 THEN 'sd'
  END)";

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

    /// SQL condition on `f` (no parameters).
    fn sql(self) -> &'static str {
        // `hdr` lists every format of the stream (`["dolby_vision","hdr10"]`
        // for DV 8.1), so a DV 8.1 file is also an HDR10 file.
        match self {
            Self::Sdr => {
                "(f.video_json IS NOT NULL AND ifnull(json_array_length(f.video_json, '$.hdr'), 0) = 0)"
            }
            Self::AnyHdr => "(ifnull(json_array_length(f.video_json, '$.hdr'), 0) > 0)",
            Self::Hdr10 => "(instr(json_extract(f.video_json, '$.hdr'), '\"hdr10\"') > 0)",
            Self::Hdr10Plus => "(instr(json_extract(f.video_json, '$.hdr'), '\"hdr10_plus\"') > 0)",
            Self::DolbyVision => {
                "(instr(json_extract(f.video_json, '$.hdr'), '\"dolby_vision\"') > 0)"
            }
            Self::Hlg => "(instr(json_extract(f.video_json, '$.hdr'), '\"hlg\"') > 0)",
        }
    }
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

/// Registers `pa_sort_key(text)` on a connection.
pub(crate) fn register_functions(conn: &Connection) -> Result<(), Error> {
    conn.create_scalar_function(
        "pa_sort_key",
        1,
        FunctionFlags::SQLITE_UTF8 | FunctionFlags::SQLITE_DETERMINISTIC,
        |ctx| {
            let title: Option<String> = ctx.get(0)?;
            Ok(title.map(|t| sort_key(&t)))
        },
    )?;
    Ok(())
}

// ---------------------------------------------------------------------------
// SQL

fn work_kinds(kind: ItemKind) -> Result<&'static str, Error> {
    match kind {
        ItemKind::Movie => Ok("w.kind = 'movie'"),
        ItemKind::Show => Ok("w.kind IN ('show', 'anime')"),
        other => Err(Error::Invalid(format!(
            "queries list movies or shows, not {other:?}"
        ))),
    }
}

/// Watch state per work (`ws(work_id, state, last)`) for the default
/// profile, as a CTE. Bound to `?1` = profile.
fn watch_cte(kind: ItemKind) -> &'static str {
    match kind {
        ItemKind::Movie => {
            "ws AS (
  SELECT work_id,
         CASE WHEN max(watched) = 1 THEN 2 WHEN max(position_ms) > 0 THEN 1 ELSE 0 END AS state,
         max(updated_at_ms) AS last
  FROM (
    SELECT x.work_id, p.watched, p.position_ms, p.updated_at_ms
      FROM progress p
      JOIN external_ids x ON x.provider = 'tmdb' AND x.ext_kind = 'movie'
                         AND x.ext_id = substr(p.media_key, 12)
     WHERE p.profile_id = ?1 AND p.media_key GLOB 'tmdb:movie:*'
    UNION ALL
    SELECT pm.work_id, p.watched, p.position_ms, p.updated_at_ms
      FROM progress p
      JOIN files pf ON pf.fingerprint = substr(p.media_key, 6)
      JOIN matches pm ON pm.file_id = pf.id AND pm.state <> 'rejected'
     WHERE p.profile_id = ?1 AND p.media_key GLOB 'file:*'
  ) GROUP BY work_id)"
        }
        _ => {
            "tk AS (
  SELECT p.watched, p.position_ms, p.updated_at_ms,
         substr(p.media_key, 14, instr(substr(p.media_key, 14), ':') - 1) AS show,
         substr(p.media_key, 14 + instr(substr(p.media_key, 14), ':')) AS se
    FROM progress p
   WHERE p.profile_id = ?1 AND p.media_key GLOB 'tmdb:episode:*'),
eps AS (
  SELECT e.work_id, e.id AS episode_id, tk.watched, tk.position_ms, tk.updated_at_ms
    FROM tk
    JOIN external_ids x ON x.provider = 'tmdb' AND x.ext_kind = 'tv' AND x.ext_id = tk.show
    JOIN episodes e ON e.work_id = x.work_id
                   AND e.season_number = CAST(substr(tk.se, 1, instr(tk.se, ':') - 1) AS INTEGER)
                   AND e.episode_number = CAST(substr(tk.se, instr(tk.se, ':') + 1) AS INTEGER)
  UNION ALL
  SELECT pm.work_id, me.episode_id, p.watched, p.position_ms, p.updated_at_ms
    FROM progress p
    JOIN files pf ON pf.fingerprint = substr(p.media_key, 6)
    JOIN matches pm ON pm.file_id = pf.id AND pm.state <> 'rejected'
    JOIN match_episodes me ON me.match_id = pm.id
   WHERE p.profile_id = ?1 AND p.media_key GLOB 'file:*'),
ep AS (
  SELECT work_id, episode_id, max(watched) AS watched, max(position_ms) AS pos,
         max(updated_at_ms) AS last
    FROM eps GROUP BY episode_id),
ws AS (
  SELECT ep.work_id,
         CASE
           WHEN sum(ep.watched) >= (
             SELECT count(DISTINCT me2.episode_id)
               FROM matches m2
               JOIN files f2 ON f2.id = m2.file_id AND f2.missing_since_ms IS NULL
               JOIN match_episodes me2 ON me2.match_id = m2.id
              WHERE m2.work_id = ep.work_id AND m2.state <> 'rejected') THEN 2
           WHEN sum(ep.watched) > 0 OR max(ep.pos) > 0 THEN 1
           ELSE 0
         END AS state,
         max(ep.last) AS last
    FROM ep GROUP BY ep.work_id)"
        }
    }
}

/// SQL with its parameters; `?1` is always the profile.
struct Sql {
    text: String,
    params: Vec<Value>,
}

impl Sql {
    fn bind(&mut self, v: impl Into<Value>) -> String {
        self.params.push(v.into());
        format!("?{}", self.params.len())
    }

    fn bind_list<T: Clone + Into<Value>>(&mut self, values: &[T]) -> String {
        let marks: Vec<String> = values.iter().map(|v| self.bind(v.clone())).collect();
        marks.join(", ")
    }
}

/// `SELECT w.id, <columns> ... GROUP BY w.id` for a query.
/// `order` is appended verbatim.
fn grouped(query: &LibraryQuery, columns: &str, order: Option<&str>) -> Result<Sql, Error> {
    let f = &query.filter;
    let mut sql = Sql {
        text: String::new(),
        params: vec![Value::Integer(PROFILE)],
    };
    let mut conds = vec![work_kinds(query.kind)?.to_owned()];
    if let Some(y) = f.year_min {
        let m = sql.bind(y);
        conds.push(format!("w.year >= {m}"));
    }
    if let Some(y) = f.year_max {
        let m = sql.bind(y);
        conds.push(format!("w.year <= {m}"));
    }
    if !f.genres.is_empty() {
        let names = sql.bind_list(&f.genres);
        conds.push(format!(
            "EXISTS (SELECT 1 FROM work_genres g WHERE g.work_id = w.id \
             AND g.name COLLATE NOCASE IN ({names}))"
        ));
    }
    if !f.sources.is_empty() {
        let ids = sql.bind_list(&f.sources);
        conds.push(format!("f.source_id IN ({ids})"));
    }
    if !f.resolutions.is_empty() {
        let slugs: Vec<&str> = f.resolutions.iter().map(|r| r.as_str()).collect();
        let marks = sql.bind_list(&slugs.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>());
        conds.push(format!("{RESOLUTION_SQL} IN ({marks})"));
    }
    if !f.hdr.is_empty() {
        let any: Vec<&str> = f.hdr.iter().map(|h| h.sql()).collect();
        conds.push(format!("({})", any.join(" OR ")));
    }
    if !f.audio_formats.is_empty() {
        let marks = sql.bind_list(&f.audio_formats);
        conds.push(format!(
            "EXISTS (SELECT 1 FROM json_each(f.audio_formats) j WHERE j.value IN ({marks}))"
        ));
    }
    if !f.audio_languages.is_empty() {
        let langs: Vec<String> = f.audio_languages.iter().map(|l| l.to_lowercase()).collect();
        let marks = sql.bind_list(&langs);
        conds.push(format!(
            "EXISTS (SELECT 1 FROM json_each(f.audio_langs) j WHERE lower(j.value) IN ({marks}))"
        ));
    }
    let needs_watch =
        !f.watched.is_empty() || query.sort.field == SortField::LastWatched;
    if !f.watched.is_empty() {
        let codes: Vec<i64> = f.watched.iter().map(|w| w.code()).collect();
        let marks = sql.bind_list(&codes);
        conds.push(format!("ifnull(ws.state, 0) IN ({marks})"));
    }
    let (with, join) = if needs_watch {
        (
            format!("WITH {}\n", watch_cte(query.kind)),
            "LEFT JOIN ws ON ws.work_id = w.id",
        )
    } else {
        // `?1` must be used for the numbering to hold.
        (String::new(), "")
    };
    let profile_guard = if needs_watch { "" } else { "AND ?1 IS NOT NULL" };
    sql.text = format!(
        "{with}SELECT w.id, {columns}
FROM works w
JOIN matches m ON m.work_id = w.id AND m.state <> 'rejected'
JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
{join}
WHERE {} {profile_guard}
GROUP BY w.id
{}",
        conds.join("\n  AND "),
        order.unwrap_or("")
    );
    Ok(sql)
}

/// `ORDER BY` of a sort.
fn order_by(sort: LibrarySort) -> String {
    let dir = if sort.descending { "DESC" } else { "ASC" };
    // Ties of other fields read A→Z whatever the direction.
    let title = "pa_sort_key(w.title), w.year, w.id";
    match sort.field {
        SortField::Title => format!("ORDER BY pa_sort_key(w.title) {dir}, w.year {dir}, w.id {dir}"),
        SortField::Year => format!("ORDER BY w.year {dir} NULLS LAST, {title}"),
        SortField::Added => format!("ORDER BY max(f.added_at_ms) {dir} NULLS LAST, {title}"),
        SortField::Duration => format!(
            "ORDER BY ifnull(max(f.duration_ms), w.runtime_ms) {dir} NULLS LAST, {title}"
        ),
        SortField::LastWatched => format!("ORDER BY max(ws.last) {dir} NULLS LAST, {title}"),
    }
}

fn to_usize(n: i64) -> usize {
    usize::try_from(n).unwrap_or(0)
}

pub(crate) fn run_query(
    conn: &Connection,
    query: &LibraryQuery,
    offset: usize,
    limit: usize,
) -> Result<ItemPage, Error> {
    // All matching ids in order, then the page: one pass gives the total
    // without a window function or a second count query.
    let sql = grouped(query, "NULL", Some(&order_by(query.sort)))?;
    let mut stmt = conn.prepare_cached(&sql.text)?;
    let mut ids: Vec<i64> = Vec::new();
    let mut rows = stmt.query(params_from_iter(sql.params.iter()))?;
    while let Some(r) = rows.next()? {
        ids.push(r.get(0)?);
    }
    drop(rows);
    let total = ids.len();
    let page: Vec<i64> = ids.into_iter().skip(offset).take(limit).collect();
    let items = items::works_by_ids(conn, query.kind, &page)?;
    Ok(ItemPage {
        items,
        total,
        offset,
    })
}

pub(crate) fn alphabet(conn: &Connection, query: &LibraryQuery) -> Result<Vec<AlphabetEntry>, Error> {
    let descending = query.sort.field == SortField::Title && query.sort.descending;
    let sort = LibrarySort {
        field: SortField::Title,
        descending,
    };
    let sql = grouped(query, "pa_sort_key(w.title)", Some(&order_by(sort)))?;
    let mut stmt = conn.prepare_cached(&sql.text)?;
    let mut rows = stmt.query(params_from_iter(sql.params.iter()))?;
    let mut out: Vec<AlphabetEntry> = Vec::new();
    let mut n = 0;
    while let Some(r) = rows.next()? {
        let key: String = r.get(1)?;
        let letter = index_letter(&key);
        match out.last_mut() {
            Some(last) if last.letter == letter => last.count += 1,
            _ => out.push(AlphabetEntry {
                letter,
                offset: n,
                count: 1,
            }),
        }
        n += 1;
    }
    Ok(out)
}

pub(crate) fn facets(conn: &Connection, kind: ItemKind) -> Result<Facets, Error> {
    let kinds = work_kinds(kind)?;
    let present = format!(
        "FROM works w
         JOIN matches m ON m.work_id = w.id AND m.state <> 'rejected'
         JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
         WHERE {kinds}"
    );
    let mut out = Facets::default();

    let mut stmt = conn.prepare_cached(&format!(
        "SELECT min(g.name), count(DISTINCT g.work_id) FROM work_genres g
         WHERE g.work_id IN (SELECT w.id {present})
         GROUP BY g.name COLLATE NOCASE ORDER BY g.name COLLATE NOCASE"
    ))?;
    out.genres = stmt
        .query_map([], |r| {
            Ok(FacetValue {
                value: r.get(0)?,
                count: to_usize(r.get(1)?),
            })
        })?
        .collect::<Result<_, _>>()?;

    (out.year_min, out.year_max) = conn
        .prepare_cached(&format!("SELECT min(w.year), max(w.year) {present}"))?
        .query_row([], |r| Ok((r.get(0)?, r.get(1)?)))?;

    let mut stmt = conn.prepare_cached(&format!(
        "SELECT res, count(DISTINCT id) FROM (SELECT w.id AS id, {RESOLUTION_SQL} AS res {present})
         WHERE res IS NOT NULL GROUP BY res"
    ))?;
    let mut res: Vec<FacetValue<Resolution>> = stmt
        .query_map([], |r| Ok((r.get::<_, String>(0)?, to_usize(r.get(1)?))))?
        .filter_map(|r| match r {
            Ok((slug, count)) => Resolution::from_slug(&slug).map(|value| Ok(FacetValue { value, count })),
            Err(e) => Some(Err(e)),
        })
        .collect::<Result<_, _>>()?;
    res.sort_by_key(|f| f.value);
    out.resolutions = res;

    let hdr_cols: Vec<String> = HdrFilter::ALL
        .iter()
        .map(|h| format!("count(DISTINCT CASE WHEN {} THEN w.id END)", h.sql()))
        .collect();
    let counts: Vec<i64> = conn
        .prepare_cached(&format!("SELECT {} {present}", hdr_cols.join(", ")))?
        .query_row([], |r| (0..HdrFilter::ALL.len()).map(|i| r.get(i)).collect())?;
    out.hdr = HdrFilter::ALL
        .iter()
        .zip(counts)
        .filter(|(_, c)| *c > 0)
        .map(|(h, c)| FacetValue {
            value: *h,
            count: to_usize(c),
        })
        .collect();

    for (column, target) in [
        ("f.audio_formats", &mut out.audio_formats),
        ("f.audio_langs", &mut out.audio_languages),
    ] {
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT j.value, count(DISTINCT w.id) {present_from}, json_each({column}) j
             WHERE {kinds} AND j.type = 'text'
             GROUP BY j.value ORDER BY 2 DESC, j.value",
            present_from = present.split("WHERE").next().unwrap_or_default(),
        ))?;
        *target = stmt
            .query_map([], |r| {
                Ok(FacetValue {
                    value: r.get(0)?,
                    count: to_usize(r.get(1)?),
                })
            })?
            .collect::<Result<_, _>>()?;
    }

    let mut stmt = conn.prepare_cached(&format!(
        "SELECT f.source_id, count(DISTINCT w.id) {present} GROUP BY f.source_id ORDER BY f.source_id"
    ))?;
    out.sources = stmt
        .query_map([], |r| {
            Ok(FacetValue {
                value: r.get(0)?,
                count: to_usize(r.get(1)?),
            })
        })?
        .collect::<Result<_, _>>()?;
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
            let e = folders.entry(name.to_owned()).or_insert_with(|| FolderEntry {
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
            std::cmp::Ordering::Equal
        };
        let by_field = match sort.field {
            SortField::Added => a.added_at_ms.cmp(&b.added_at_ms),
            SortField::Duration => match (a.duration_ms, b.duration_ms) {
                (Some(x), Some(y)) => x.cmp(&y),
                // Missing values last in both directions.
                (Some(_), None) => return group.then(std::cmp::Ordering::Less),
                (None, Some(_)) => return group.then(std::cmp::Ordering::Greater),
                (None, None) => std::cmp::Ordering::Equal,
            },
            _ => std::cmp::Ordering::Equal,
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

fn check_kind(query: &LibraryQuery) -> Result<(), Error> {
    work_kinds(query.kind).map(|_| ())
}

impl Catalog {
    /// One page of a query: `limit` items from `offset`, and the total.
    pub fn query(&self, query: &LibraryQuery, offset: usize, limit: usize) -> Result<ItemPage, Error> {
        run_query(&self.lock(), query, offset, limit)
    }

    /// Alphabet index of a query in title order (descending if the query
    /// sorts by title descending): letters in list order with the offset of
    /// their first item.
    pub fn alphabet_index(&self, query: &LibraryQuery) -> Result<Vec<AlphabetEntry>, Error> {
        alphabet(&self.lock(), query)
    }

    /// Filter values present among the movies or shows.
    pub fn facets(&self, kind: ItemKind) -> Result<Facets, Error> {
        facets(&self.lock(), kind)
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
        if !views::update_shelf(&self.lock(), id, name, &query.to_json(), pinned_home, now_ms())? {
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
        let shelf = views::shelf(&conn, id)?.ok_or_else(|| Error::NotFound(format!("shelf {id}")))?;
        run_query(&conn, &LibraryQuery::from_json(&shelf.query_json)?, offset, limit)
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
