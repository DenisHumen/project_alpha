// SPDX-License-Identifier: MPL-2.0
//! Adapter of the library's basic matcher (META-013) to [`Recognizer`]:
//! `pa-eval run --recognizer tmdb`.
//!
//! The pipeline is the one `pa-catalog` runs on a source, without the
//! database:
//!
//! 1. folder groups of the case's files (`pa_scanner::structure::analyze`,
//!    LIB-007: discs, split files, extras, samples);
//! 2. a plan per video (`pa_catalog::recognize::plan_files`: parser, group
//!    roles, NFO ids) and works by title key or NFO id
//!    (`pa_catalog::recognize::group_works`);
//! 3. per work, the TMDB search and ranking of the metadata run
//!    (`pa_catalog::metadata::match_search`) — a work with an NFO TMDB id
//!    is taken as it is.
//!
//! Provider calls are neutral `tmdb/search` requests (`kind`, `query`,
//! optional `year`; the shape of `pa_metadata`'s `TmdbClient` provider), so
//! the same recognizer runs against a tape recorded from TMDB
//! (`--record-tmdb`, kept locally: TMDB terms do not allow publishing its
//! data) or against [`SyntheticTmdb`], a stand-in search over a local
//! catalog of titles.

use std::collections::{HashMap, HashSet};

use pa_catalog::metadata::{WorkQuery, match_search};
use pa_catalog::pa_metadata::tmdb::{MediaKind, SearchResult};
use pa_catalog::recognize::{FilePlan, PlanRole, WorkPlan, group_works, plan_files};
use pa_recognition::title::normalize::fold;
use pa_recognition::title::{TitleRecord, WorkKind};
use pa_scanner::structure::{ListedFile, Role, StructureOptions, analyze};
use serde_json::{Value, json};

use crate::corpus::{CaseInput, Ids, Numbering, SeasonEpisode};
use crate::provider::{Provider, ProviderError, ProviderRequest};
use crate::recognizer::{FilePrediction, Recognizer, WorkGuess, from_product_parse};

/// The META-013 matcher over provider responses.
#[derive(Debug, Default)]
pub struct Tmdb;

/// Results of a neutral `tmdb/search` response as TMDB search results.
fn search_results(response: &Value, kind: MediaKind) -> Vec<SearchResult> {
    let Some(results) = response.get("results").and_then(Value::as_array) else {
        return Vec::new();
    };
    results
        .iter()
        .filter_map(|r| {
            let tmdb = r.get("ids")?.get("tmdb")?.as_str()?;
            let (k, id) = tmdb.split_once(':')?;
            if k != kind.as_str() {
                return None;
            }
            let text = |key: &str| r.get(key).and_then(Value::as_str).map(str::to_owned);
            Some(SearchResult {
                id: id.parse().ok()?,
                media_type: Some(kind.as_str().to_owned()),
                title: text("title"),
                original_title: text("original_title"),
                original_language: text("original_language"),
                release_date: r
                    .get("year")
                    .and_then(Value::as_u64)
                    .map(|y| format!("{y:04}-01-01")),
                origin_country: Vec::new(),
                genre_ids: Vec::new(),
                overview: None,
                poster_path: None,
                backdrop_path: None,
                popularity: r.get("popularity").and_then(Value::as_f64).unwrap_or(0.0),
                adult: false,
            })
        })
        .collect()
}

/// The ids and confidence of a work, from its NFO ids or the TMDB search.
fn match_work(work: &WorkPlan, provider: &mut dyn Provider) -> Option<(Ids, f64)> {
    let mut ids = Ids::default();
    for (provider_name, kind, id) in work.external_ids() {
        match provider_name.as_str() {
            "tmdb" => ids.tmdb = Some(format!("{kind}:{id}")),
            "imdb" => ids.imdb = Some(id),
            "tvdb" => ids.tvdb = Some(id),
            "anidb" => ids.anidb = id.parse().ok(),
            _ => {}
        }
    }
    if ids.tmdb.is_some() {
        return Some((ids, 1.0));
    }
    let query = WorkQuery {
        kind: work.kind.to_owned(),
        year: work.year,
        titles: std::iter::once(&work.title)
            .chain(&work.alt_titles)
            .cloned()
            .collect(),
    };
    let found = match_search::<std::convert::Infallible>(&query, &mut |kind, q, year| {
        let mut request = ProviderRequest::new("tmdb", "search")
            .param("kind", kind.as_str())
            .param("query", q);
        if let Some(y) = year {
            request = request.param("year", y.to_string());
        }
        // A failed or missing response is "nothing found" (tape misses are
        // counted by the runner).
        Ok(provider
            .call(&request)
            .map(|v| search_results(&v, kind))
            .unwrap_or_default())
    })
    .unwrap_or_else(|e| match e {})?;
    let kind = if work.is_series() { "tv" } else { "movie" };
    ids.tmdb = Some(format!("{kind}:{}", found.result.id));
    Some((ids, found.score))
}

fn numbering(plan: &FilePlan) -> Vec<Numbering> {
    let n = |v: Option<i64>| v.and_then(|v| u32::try_from(v).ok());
    plan.episodes
        .iter()
        .filter_map(|e| match (n(e.season), n(e.episode), n(e.absolute)) {
            (Some(season), Some(episode), _) => Some(Numbering {
                tmdb: Some(SeasonEpisode { season, episode }),
                ..Default::default()
            }),
            (_, None, Some(absolute)) => Some(Numbering {
                tvdb_absolute: Some(absolute),
                ..Default::default()
            }),
            _ => None,
        })
        .collect()
}

impl Recognizer for Tmdb {
    fn name(&self) -> &str {
        "tmdb-v0"
    }

    fn recognize(&mut self, case: &CaseInput, provider: &mut dyn Provider) -> Vec<FilePrediction> {
        let listed: Vec<ListedFile> = case
            .files
            .iter()
            .map(|f| ListedFile {
                path: f.path.clone(),
                size: f.size.unwrap_or(0),
                duration_ms: f.duration_ms,
            })
            .collect();
        let groups = analyze(&listed, &StructureOptions::default());
        let roles: HashMap<&str, Role> = groups
            .iter()
            .flat_map(|g| &g.entries)
            .map(|e| (e.path.as_str(), e.role))
            .collect();
        // Videos the library recognizes: group videos (disc images and
        // streams included) and loose video files.
        let videos: Vec<&str> = case
            .files
            .iter()
            .map(|f| f.path.as_str())
            .filter(|p| match roles.get(p) {
                Some(role) => matches!(
                    role,
                    Role::Main | Role::Part | Role::Extra | Role::Sample | Role::Ignore
                ),
                None => pa_scanner::media_kind(p) == Some(pa_scanner::MediaKind::Video),
            })
            .collect();
        let plans = plan_files(&videos, &groups);
        let (works, of) = group_works(&plans);
        let matched: Vec<Option<(Ids, f64)>> =
            works.iter().map(|w| match_work(w, provider)).collect();

        let at: HashMap<&str, usize> = videos.iter().enumerate().map(|(i, p)| (*p, i)).collect();
        case.files
            .iter()
            .map(|f| {
                let mut p = FilePrediction {
                    path: f.path.clone(),
                    parse: Some(from_product_parse(
                        &pa_recognition::parse::parse(&f.path).best().parsed,
                    )),
                    ..Default::default()
                };
                let Some(&i) = at.get(f.path.as_str()) else {
                    return p;
                };
                let (Some(plan), Some(w)) = (&plans[i], of[i]) else {
                    return p;
                };
                if let Some((ids, confidence)) = &matched[w] {
                    p.work = Some(WorkGuess {
                        kind: Some(works[w].kind.to_owned()),
                        ids: ids.clone(),
                    });
                    p.confidence = *confidence;
                    if plan.role != PlanRole::Extra {
                        p.episodes = numbering(plan);
                    }
                }
                p
            })
            .collect()
    }
}

/// Lowercase words of a title, accents folded (TMDB search ignores case
/// and accents).
fn words(title: &str) -> Vec<String> {
    fold(title)
        .split(|c: char| !c.is_alphanumeric())
        .filter(|w| !w.is_empty())
        .map(str::to_owned)
        .collect()
}

#[derive(Debug, Clone)]
struct Entry {
    /// `movie` or `tv`.
    kind: &'static str,
    /// `movie:603`.
    tmdb: String,
    year: Option<u16>,
    titles: Vec<String>,
    words: Vec<HashSet<String>>,
    compact: Vec<String>,
}

/// A stand-in for TMDB search over a local catalog of title records
/// (`baselines/v0/title-catalog.jsonl`): **not TMDB data**, only the corpus
/// works with their ids, original/English and Russian titles and years.
///
/// It answers neutral `tmdb/search` requests the way TMDB search behaves
/// for the matcher: a record is found when every word of the query is a
/// word of one of its titles (or the titles are equal without spaces and
/// punctuation), case and accents ignored; `year` keeps only records of
/// that year; results are newest first (a crude stand-in for popularity),
/// at most 20, with the main title as `original_title` and the Russian one
/// as `title` (the app searches in `ru-RU`). Other endpoints are
/// [`ProviderError::NotFound`]. Remakes and same-named works compete as
/// they would on TMDB; works TMDB knows and the catalog does not (and
/// transliterated or alternative titles TMDB would also match) do not, so
/// numbers on it are an optimistic estimate.
#[derive(Debug, Clone, Default)]
pub struct SyntheticTmdb {
    entries: Vec<Entry>,
}

impl SyntheticTmdb {
    /// Builds the search from records with a TMDB id.
    #[must_use]
    pub fn new(records: &[TitleRecord]) -> Self {
        let entries = records
            .iter()
            .filter_map(|r| {
                let tmdb = r.ids.get("tmdb")?;
                let kind = match tmdb.split_once(':')?.0 {
                    "movie" => "movie",
                    "tv" => "tv",
                    _ => return None,
                };
                if (kind == "movie") != (r.kind == WorkKind::Movie) || r.titles.is_empty() {
                    return None;
                }
                Some(Entry {
                    kind,
                    tmdb: tmdb.clone(),
                    year: r.year,
                    titles: r.titles.clone(),
                    words: r
                        .titles
                        .iter()
                        .map(|t| words(t).into_iter().collect())
                        .collect(),
                    compact: r.titles.iter().map(|t| words(t).concat()).collect(),
                })
            })
            .collect();
        Self { entries }
    }

    /// Number of searchable records.
    #[must_use]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// `true` without records.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    fn search(&self, kind: &str, query: &str, year: Option<u16>) -> Value {
        let q = words(query);
        let compact = q.concat();
        let mut found: Vec<&Entry> = if q.is_empty() {
            Vec::new()
        } else {
            self.entries
                .iter()
                .filter(|e| e.kind == kind)
                .filter(|e| year.is_none() || e.year == year)
                .filter(|e| {
                    e.words.iter().any(|w| q.iter().all(|x| w.contains(x)))
                        || e.compact.contains(&compact)
                })
                .collect()
        };
        found.sort_by(|a, b| b.year.cmp(&a.year).then(a.tmdb.cmp(&b.tmdb)));
        found.truncate(20);
        let results: Vec<Value> = found
            .iter()
            .map(|e| {
                json!({
                    "kind": if e.kind == "movie" { "movie" } else { "show" },
                    "title": e.titles.get(1).unwrap_or(&e.titles[0]),
                    "original_title": e.titles[0],
                    "year": e.year,
                    "popularity": 0.0,
                    "ids": { "tmdb": e.tmdb },
                })
            })
            .collect();
        json!({ "results": results })
    }
}

impl Provider for SyntheticTmdb {
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError> {
        if request.provider != "tmdb" || request.endpoint != "search" {
            return Err(ProviderError::NotFound(request.key()));
        }
        let get = |k: &str| request.params.get(k).map(String::as_str);
        let kind = match get("kind") {
            Some("movie") => "movie",
            Some("tv") => "tv",
            other => {
                return Err(ProviderError::BadRequest(format!(
                    "synthetic tmdb/search needs kind movie|tv, got {other:?}"
                )));
            }
        };
        let query = get("query").unwrap_or_default();
        let year = match get("year") {
            Some(y) => Some(
                y.parse()
                    .map_err(|_| ProviderError::BadRequest(format!("bad year {y:?}")))?,
            ),
            None => None,
        };
        Ok(self.search(kind, query, year))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn record(tmdb: &str, kind: WorkKind, year: u16, titles: &[&str]) -> TitleRecord {
        TitleRecord {
            ids: [("tmdb".to_owned(), tmdb.to_owned())].into(),
            kind,
            year: Some(year),
            titles: titles.iter().map(|t| (*t).to_owned()).collect(),
            ..Default::default()
        }
    }

    fn synthetic() -> SyntheticTmdb {
        SyntheticTmdb::new(&[
            record("movie:841", WorkKind::Movie, 1984, &["Dune", "Дюна"]),
            record("movie:438631", WorkKind::Movie, 2021, &["Dune", "Дюна"]),
            record(
                "movie:101",
                WorkKind::Movie,
                1994,
                &["Léon: The Professional", "Леон"],
            ),
            record("tv:70523", WorkKind::Series, 2017, &["Dark", "Тьма"]),
        ])
    }

    fn ids(v: &Value) -> Vec<&str> {
        v["results"]
            .as_array()
            .unwrap()
            .iter()
            .map(|r| r["ids"]["tmdb"].as_str().unwrap())
            .collect()
    }

    #[test]
    fn synthetic_search_matches_words_kind_and_year() {
        let s = synthetic();
        assert_eq!(
            ids(&s.search("movie", "dune", None)),
            ["movie:438631", "movie:841"]
        );
        assert_eq!(ids(&s.search("movie", "Dune", Some(1984))), ["movie:841"]);
        assert_eq!(ids(&s.search("movie", "Leon", None)), ["movie:101"]);
        assert_eq!(ids(&s.search("movie", "Дюна", None)).len(), 2);
        assert!(ids(&s.search("movie", "Dark", None)).is_empty());
        assert_eq!(ids(&s.search("tv", "Тьма", None)), ["tv:70523"]);
        assert!(ids(&s.search("movie", "", None)).is_empty());
    }

    #[test]
    fn the_matcher_picks_the_year_and_groups_the_folder() {
        let mut s = synthetic();
        let case = CaseInput {
            files: [
                "Movies/Dune (2021)/Dune.2021.2160p.mkv",
                "Movies/Dune (2021)/Dune (2021)-trailer.mkv",
                "Movies/Dune (2021)/sample.mkv",
                "Shows/Dark/Season 1/Dark.S01E02.mkv",
            ]
            .iter()
            .map(|p| crate::corpus::InputFile {
                path: (*p).to_owned(),
                ..Default::default()
            })
            .collect(),
        };
        let out = Tmdb.recognize(&case, &mut s);
        let tmdb = |i: usize| {
            out[i]
                .work
                .as_ref()
                .and_then(|w| w.ids.tmdb.clone())
                .unwrap_or_default()
        };
        assert_eq!(tmdb(0), "movie:438631");
        assert_eq!(tmdb(1), "movie:438631", "the trailer is the movie's");
        assert_eq!(tmdb(2), "movie:438631", "the sample is the movie's");
        assert!(out[1].episodes.is_empty());
        assert_eq!(tmdb(3), "tv:70523");
        assert_eq!(
            out[3].episodes[0].tmdb,
            Some(SeasonEpisode {
                season: 1,
                episode: 2
            })
        );
        assert!(out[0].confidence >= 0.92, "{}", out[0].confidence);
    }
}
