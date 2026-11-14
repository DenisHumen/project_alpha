// SPDX-License-Identifier: MPL-2.0
//! In-memory index of the filterable library (LIB-009).
//!
//! Library queries filter and sort up to tens of thousands of works on
//! columns spread over `works`, `files` (JSON probe columns), `work_genres`
//! and `progress`. Doing that in SQL on every query costs a join of every
//! work with its files and a JSON parse per file; the index reads them once
//! into compact rows and answers queries in memory.
//!
//! The index is a snapshot tagged with the connection's `PRAGMA
//! data_version` (changes when another connection commits, e.g. a scan)
//! and `total_changes()` (changes this connection made, e.g. progress). A
//! query compares the tag and rebuilds a stale index first, so results are
//! never older than the database.

use std::collections::HashMap;
use std::ops::Range;

use pa_library::rusqlite::Connection;

use crate::Error;
use crate::items::ItemKind;
use crate::progress::PROFILE;
use crate::query::{
    FacetValue, Facets, HdrFilter, LibraryFilter, LibraryQuery, Resolution, SortField, WatchState,
    sort_key,
};

/// Database state an index was built from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Stamp {
    data_version: i64,
    total_changes: i64,
}

impl Stamp {
    pub(crate) fn of(conn: &Connection) -> Result<Self, Error> {
        let data_version = conn.query_row("PRAGMA data_version", [], |r| r.get(0))?;
        let total_changes = conn.query_row("SELECT total_changes()", [], |r| r.get(0))?;
        Ok(Self {
            data_version,
            total_changes,
        })
    }
}

/// HDR bits of a file.
const PROBED: u8 = 1;
const DV: u8 = 1 << 1;
const HDR10_PLUS: u8 = 1 << 2;
const HDR10: u8 = 1 << 3;
const HLG: u8 = 1 << 4;
const ANY_HDR: u8 = DV | HDR10_PLUS | HDR10 | HLG;

impl HdrFilter {
    fn matches(self, bits: u8) -> bool {
        match self {
            Self::Sdr => bits & PROBED != 0 && bits & ANY_HDR == 0,
            Self::AnyHdr => bits & ANY_HDR != 0,
            Self::Hdr10 => bits & HDR10 != 0,
            Self::Hdr10Plus => bits & HDR10_PLUS != 0,
            Self::DolbyVision => bits & DV != 0,
            Self::Hlg => bits & HLG != 0,
        }
    }
}

/// A present file of a work.
#[derive(Debug, Clone)]
struct FileRow {
    source: i64,
    resolution: Option<Resolution>,
    hdr: u8,
    /// Interned audio format slugs.
    audio: Vec<u32>,
    /// Interned lower-case audio languages.
    langs: Vec<u32>,
}

/// A movie or show with at least one present file.
#[derive(Debug, Clone)]
struct WorkRow {
    id: i64,
    kind: ItemKind,
    key: String,
    year: Option<i64>,
    added: Option<i64>,
    /// Longest present file.
    file_duration: Option<i64>,
    /// Provider runtime, when no file has a duration.
    runtime: Option<i64>,
    files: Range<usize>,
    /// Interned lower-case genre names.
    genres: Vec<u32>,
    watch: WatchState,
    last_watched: Option<i64>,
}

/// Strings shared by many rows (formats, languages, genres) as small ids.
#[derive(Debug, Default)]
struct Interner {
    ids: HashMap<String, u32>,
    names: Vec<String>,
}

impl Interner {
    fn id(&mut self, s: &str) -> u32 {
        if let Some(id) = self.ids.get(s) {
            return *id;
        }
        let id = u32::try_from(self.names.len()).unwrap_or(u32::MAX);
        self.ids.insert(s.to_owned(), id);
        self.names.push(s.to_owned());
        id
    }

    fn get(&self, s: &str) -> Option<u32> {
        self.ids.get(s).copied()
    }
}

/// The index; see the module docs.
#[derive(Debug, Default)]
pub(crate) struct Index {
    works: Vec<WorkRow>,
    files: Vec<FileRow>,
    strings: Interner,
    /// Display form of a lower-case genre id (the smallest spelling seen).
    genre_names: HashMap<u32, String>,
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

/// Watch state of movies (`work_id, state, last`) for profile `?1`: watched
/// if any key (TMDB or file) is watched, in progress if one has a position.
const MOVIE_WATCH_SQL: &str = "
SELECT work_id,
       CASE WHEN max(watched) = 1 THEN 2 WHEN max(position_ms) > 0 THEN 1 ELSE 0 END,
       max(updated_at_ms)
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
    JOIN works pw ON pw.id = pm.work_id AND pw.kind = 'movie'
   WHERE p.profile_id = ?1 AND p.media_key GLOB 'file:*'
) GROUP BY work_id";

/// Watch state of shows for profile `?1`: watched when every present
/// episode is, in progress when some are watched or started. Episode keys
/// are `tmdb:episode:<show>:<season>:<episode>` or `file:<fingerprint>`.
const SHOW_WATCH_SQL: &str = "
WITH tk AS (
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
    FROM eps GROUP BY episode_id)
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
       END,
       max(ep.last)
  FROM ep GROUP BY ep.work_id";

fn json_strings(json: Option<String>) -> Vec<String> {
    json.and_then(|j| serde_json::from_str::<Vec<serde_json::Value>>(&j).ok())
        .map(|v| {
            v.into_iter()
                .filter_map(|x| x.as_str().map(str::to_owned))
                .collect()
        })
        .unwrap_or_default()
}

fn hdr_bits(probed: bool, hdr: Option<&str>) -> u8 {
    let mut bits = if probed { PROBED } else { 0 };
    for s in json_strings(hdr.map(str::to_owned)) {
        bits |= match s.as_str() {
            "dolby_vision" => DV,
            "hdr10_plus" => HDR10_PLUS,
            "hdr10" => HDR10,
            "hlg" => HLG,
            _ => 0,
        };
    }
    bits
}

impl Index {
    /// Reads the index from the database.
    pub(crate) fn build(conn: &Connection) -> Result<Self, Error> {
        let mut index = Self::default();

        // Present files of movies and shows, grouped by work.
        let mut stmt = conn.prepare_cached(&format!(
            "SELECT m.work_id, w.kind, w.title, w.year, w.runtime_ms,
                    f.source_id, f.added_at_ms, f.duration_ms, f.video_json IS NOT NULL,
                    {RESOLUTION_SQL}, json_extract(f.video_json, '$.hdr'),
                    f.audio_formats, f.audio_langs
             FROM matches m
             JOIN files f ON f.id = m.file_id AND f.missing_since_ms IS NULL
             JOIN works w ON w.id = m.work_id
             WHERE m.state <> 'rejected'
             ORDER BY m.work_id"
        ))?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let work_id: i64 = r.get(0)?;
            if index.works.last().is_none_or(|w| w.id != work_id) {
                let kind = match r.get::<_, String>(1)?.as_str() {
                    "movie" => ItemKind::Movie,
                    _ => ItemKind::Show,
                };
                let title: String = r.get(2)?;
                let at = index.files.len();
                index.works.push(WorkRow {
                    id: work_id,
                    kind,
                    key: sort_key(&title),
                    year: r.get(3)?,
                    added: None,
                    file_duration: None,
                    runtime: r.get(4)?,
                    files: at..at,
                    genres: Vec::new(),
                    watch: WatchState::Unwatched,
                    last_watched: None,
                });
            }
            let resolution = r
                .get::<_, Option<String>>(9)?
                .as_deref()
                .and_then(Resolution::from_slug);
            let hdr = hdr_bits(r.get(8)?, r.get::<_, Option<String>>(10)?.as_deref());
            let audio = json_strings(r.get(11)?)
                .iter()
                .map(|a| index.strings.id(a))
                .collect();
            let langs = json_strings(r.get(12)?)
                .iter()
                .map(|l| index.strings.id(&l.to_lowercase()))
                .collect();
            index.files.push(FileRow {
                source: r.get(5)?,
                resolution,
                hdr,
                audio,
                langs,
            });
            let added: i64 = r.get(6)?;
            let duration: Option<i64> = r.get(7)?;
            let end = index.files.len();
            let work = index.works.last_mut().expect("pushed above");
            work.files.end = end;
            work.added = Some(work.added.map_or(added, |a| a.max(added)));
            if let Some(d) = duration {
                work.file_duration = Some(work.file_duration.map_or(d, |old| old.max(d)));
            }
        }
        drop(rows);
        let at: HashMap<i64, usize> = index
            .works
            .iter()
            .enumerate()
            .map(|(i, w)| (w.id, i))
            .collect();

        let mut stmt = conn.prepare_cached("SELECT work_id, name FROM work_genres")?;
        let mut rows = stmt.query([])?;
        while let Some(r) = rows.next()? {
            let Some(&i) = at.get(&r.get::<_, i64>(0)?) else {
                continue;
            };
            let name: String = r.get(1)?;
            let id = index.strings.id(&name.to_lowercase());
            let shown = index.genre_names.entry(id).or_insert_with(|| name.clone());
            if name < *shown {
                *shown = name;
            }
            let genres = &mut index.works[i].genres;
            if !genres.contains(&id) {
                genres.push(id);
            }
        }
        drop(rows);

        for (sql, kind) in [
            (MOVIE_WATCH_SQL, ItemKind::Movie),
            (SHOW_WATCH_SQL, ItemKind::Show),
        ] {
            let mut stmt = conn.prepare_cached(sql)?;
            let mut rows = stmt.query([PROFILE])?;
            while let Some(r) = rows.next()? {
                let Some(&i) = at.get(&r.get::<_, i64>(0)?) else {
                    continue;
                };
                let work = &mut index.works[i];
                if work.kind != kind {
                    continue;
                }
                work.watch = match r.get::<_, i64>(1)? {
                    2 => WatchState::Watched,
                    1 => WatchState::InProgress,
                    _ => WatchState::Unwatched,
                };
                work.last_watched = r.get(2)?;
            }
        }
        Ok(index)
    }

    fn file_matches(
        &self,
        file: &FileRow,
        f: &LibraryFilter,
        audio: &[u32],
        langs: &[u32],
    ) -> bool {
        (f.sources.is_empty() || f.sources.contains(&file.source))
            && (f.resolutions.is_empty()
                || file.resolution.is_some_and(|r| f.resolutions.contains(&r)))
            && (f.hdr.is_empty() || f.hdr.iter().any(|h| h.matches(file.hdr)))
            && (f.audio_formats.is_empty() || file.audio.iter().any(|a| audio.contains(a)))
            && (f.audio_languages.is_empty() || file.langs.iter().any(|l| langs.contains(l)))
    }

    /// Works of `query` in its order; their ids and sort keys.
    pub(crate) fn select(&self, query: &LibraryQuery) -> Vec<(i64, &str)> {
        let f = &query.filter;
        let ids = |names: &[String], lower: bool| -> Vec<u32> {
            names
                .iter()
                .filter_map(|n| {
                    if lower {
                        self.strings.get(&n.to_lowercase())
                    } else {
                        self.strings.get(n)
                    }
                })
                .collect()
        };
        let genres = ids(&f.genres, true);
        let audio = ids(&f.audio_formats, false);
        let langs = ids(&f.audio_languages, true);
        let file_filter = !f.sources.is_empty()
            || !f.resolutions.is_empty()
            || !f.hdr.is_empty()
            || !f.audio_formats.is_empty()
            || !f.audio_languages.is_empty();
        let mut hits: Vec<&WorkRow> = self
            .works
            .iter()
            .filter(|w| {
                w.kind == query.kind
                    && f.year_min.is_none_or(|y| w.year.is_some_and(|wy| wy >= y))
                    && f.year_max.is_none_or(|y| w.year.is_some_and(|wy| wy <= y))
                    && (f.genres.is_empty() || w.genres.iter().any(|g| genres.contains(g)))
                    && (f.watched.is_empty() || f.watched.contains(&w.watch))
                    && (!file_filter
                        || self.files[w.files.clone()]
                            .iter()
                            .any(|file| self.file_matches(file, f, &audio, &langs)))
            })
            .collect();
        let by_title = |a: &WorkRow, b: &WorkRow| {
            a.key
                .cmp(&b.key)
                .then(a.year.cmp(&b.year))
                .then(a.id.cmp(&b.id))
        };
        let descending = query.sort.descending;
        let value = |w: &WorkRow| match query.sort.field {
            SortField::Title => None,
            SortField::Year => w.year,
            SortField::Added => w.added,
            SortField::Duration => w.file_duration.or(w.runtime),
            SortField::LastWatched => w.last_watched,
        };
        if query.sort.field == SortField::Title {
            hits.sort_unstable_by(|a, b| {
                let o = by_title(a, b);
                if descending { o.reverse() } else { o }
            });
        } else {
            // Missing values last in both directions; ties A→Z.
            hits.sort_unstable_by(|a, b| {
                let o = match (value(a), value(b)) {
                    (Some(x), Some(y)) if descending => y.cmp(&x),
                    (Some(x), Some(y)) => x.cmp(&y),
                    (Some(_), None) => std::cmp::Ordering::Less,
                    (None, Some(_)) => std::cmp::Ordering::Greater,
                    (None, None) => std::cmp::Ordering::Equal,
                };
                o.then_with(|| by_title(a, b))
            });
        }
        hits.into_iter().map(|w| (w.id, w.key.as_str())).collect()
    }

    /// Filter values present among the works of `kind`.
    pub(crate) fn facets(&self, kind: ItemKind) -> Facets {
        let mut out = Facets::default();
        let mut genres: HashMap<u32, usize> = HashMap::new();
        let mut res: HashMap<Resolution, usize> = HashMap::new();
        let mut hdr: HashMap<HdrFilter, usize> = HashMap::new();
        let mut audio: HashMap<u32, usize> = HashMap::new();
        let mut langs: HashMap<u32, usize> = HashMap::new();
        let mut sources: HashMap<i64, usize> = HashMap::new();
        for w in self.works.iter().filter(|w| w.kind == kind) {
            if let Some(y) = w.year {
                out.year_min = Some(out.year_min.map_or(y, |m: i64| m.min(y)));
                out.year_max = Some(out.year_max.map_or(y, |m: i64| m.max(y)));
            }
            for g in &w.genres {
                *genres.entry(*g).or_default() += 1;
            }
            let files = &self.files[w.files.clone()];
            // Each value counts a work once.
            let mut seen_res: Vec<Resolution> = Vec::new();
            let mut seen_audio: Vec<u32> = Vec::new();
            let mut seen_langs: Vec<u32> = Vec::new();
            let mut seen_sources: Vec<i64> = Vec::new();
            let mut hdr_bits_any = 0u8;
            let mut sdr = false;
            for file in files {
                if let Some(r) = file.resolution
                    && !seen_res.contains(&r)
                {
                    seen_res.push(r);
                }
                for a in &file.audio {
                    if !seen_audio.contains(a) {
                        seen_audio.push(*a);
                    }
                }
                for l in &file.langs {
                    if !seen_langs.contains(l) {
                        seen_langs.push(*l);
                    }
                }
                if !seen_sources.contains(&file.source) {
                    seen_sources.push(file.source);
                }
                hdr_bits_any |= file.hdr;
                sdr |= HdrFilter::Sdr.matches(file.hdr);
            }
            for r in seen_res {
                *res.entry(r).or_default() += 1;
            }
            for a in seen_audio {
                *audio.entry(a).or_default() += 1;
            }
            for l in seen_langs {
                *langs.entry(l).or_default() += 1;
            }
            for s in seen_sources {
                *sources.entry(s).or_default() += 1;
            }
            for h in HdrFilter::ALL {
                let has = match h {
                    HdrFilter::Sdr => sdr,
                    other => other.matches(hdr_bits_any),
                };
                if has {
                    *hdr.entry(h).or_default() += 1;
                }
            }
        }
        let name = |id: &u32| self.strings.names[*id as usize].clone();
        out.genres = genres
            .into_iter()
            .map(|(id, count)| FacetValue {
                value: self
                    .genre_names
                    .get(&id)
                    .cloned()
                    .unwrap_or_else(|| name(&id)),
                count,
            })
            .collect();
        out.genres.sort_by_key(|g| g.value.to_lowercase());
        out.resolutions = Resolution::ALL
            .into_iter()
            .filter_map(|r| res.get(&r).map(|&count| FacetValue { value: r, count }))
            .collect();
        out.hdr = HdrFilter::ALL
            .into_iter()
            .filter_map(|h| hdr.get(&h).map(|&count| FacetValue { value: h, count }))
            .collect();
        let by_count = |m: HashMap<u32, usize>| {
            let mut v: Vec<FacetValue<String>> = m
                .into_iter()
                .map(|(id, count)| FacetValue {
                    value: name(&id),
                    count,
                })
                .collect();
            v.sort_by(|a, b| b.count.cmp(&a.count).then_with(|| a.value.cmp(&b.value)));
            v
        };
        out.audio_formats = by_count(audio);
        out.audio_languages = by_count(langs);
        let mut sources: Vec<FacetValue<i64>> = sources
            .into_iter()
            .map(|(value, count)| FacetValue { value, count })
            .collect();
        sources.sort_by_key(|s| s.value);
        out.sources = sources;
        out
    }
}
