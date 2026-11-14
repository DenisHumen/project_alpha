// SPDX-License-Identifier: MPL-2.0
//! Provider metadata for recognized works (META-013, basic matcher).
//!
//! [`Catalog::fetch_metadata`] takes every work with a live match that has
//! no fresh metadata ([`MetadataOptions`]) and:
//!
//! 1. **TMDB.** A work with a TMDB id (a user fix, an NFO file or an
//!    earlier match) is read by id; a work with only an IMDb or TVDB id
//!    from an NFO file is looked up by it (`/find`). Otherwise TMDB is
//!    searched by its titles (main first) and
//!    year — `/search/movie` for movies, `/search/tv` for shows and anime —
//!    and the results are ranked with the local title index
//!    ([`pa_recognition::title::TitleIndex`]: title tiers, year ±1, kind);
//!    the best one is taken if it scores at least [`ACCEPT`]
//!    ([`match_search`], also run offline by `pa-eval`). Details,
//!    translations (titles in the metadata language, English and Russian)
//!    and the seasons of the work's episodes are fetched.
//! 2. **Anime** (works of kind `anime`): AniList by id or by title (same
//!    ranking) for romaji / English / native titles, the MAL id and a cover;
//!    Shikimori by MAL id for the Russian title and synopsis. They fill what
//!    TMDB did not (no TMDB match, no Russian overview).
//! 3. **Writing** (one transaction per work): external ids, titles (the
//!    provider title becomes `main`, local titles stay as synonyms so files
//!    named the old way still join the work), overview, year, runtime,
//!    status, artwork (TMDB poster and backdrop, AniList cover; episode
//!    stills), episode titles, overviews, air dates and TMDB ids. Matches
//!    decided by the parser become `decided_by = scorer`,
//!    `model_version = scorer_v0` with the match score as confidence and
//!    `auto_review` below [`AUTO`]; user decisions are left alone. A TMDB id
//!    already owned by another work merges the two works (two folders of
//!    one show named differently).
//!
//! The network runs on a few worker threads without holding the database;
//! results are written one work at a time, so the library stays readable.
//! A rejected TMDB key stops the run; network failures mark the work
//! `failed` (retried on a later run) and several in a row stop the run.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, mpsc};

use pa_library::rusqlite::{Connection, OptionalExtension, Transaction, params};
use pa_metadata::anilist::{AniListClient, Media};
use pa_metadata::cache::ProviderCache;
use pa_metadata::fetch::FetchError;
use pa_metadata::shikimori::{RussianInfo, ShikimoriClient};
use pa_metadata::tmdb::{
    self, Details, ExternalSource, MediaKind, SearchResult, Season, TmdbClient, TmdbError,
};
use pa_recognition::parse::MediaHint;
use pa_recognition::title::{TitleIndex, TitleQuery, TitleRecord, WorkKind};
use pa_scanner::CancelToken;
use serde_json::json;

use crate::{Catalog, Error};

/// Lowest ranking score of a search result that is accepted as the work.
pub const ACCEPT: f64 = 0.75;

/// Lowest confidence of a match that stays `auto`; below it the match is
/// `auto_review` ("check this", research 03 §4.2 [7]).
pub const AUTO: f64 = 0.92;

/// `matches.model_version` of decisions made here.
pub const MODEL_VERSION: &str = "scorer_v0";

/// Title queries tried per work.
const MAX_QUERIES: usize = 3;

/// Network failures in a row that end a run (offline).
const MAX_FAILURES_IN_A_ROW: usize = 5;

/// Worker threads doing network requests.
const WORKERS: usize = 4;

/// TMDB image sizes stored in `artwork.url`.
const POSTER_SIZE: &str = "w500";
const BACKDROP_SIZE: &str = "w1280";
const STILL_SIZE: &str = "w300";

const DAY_MS: i64 = 24 * 3600 * 1000;

/// The provider clients a metadata run uses; any of them may be absent.
#[derive(Clone, Default)]
pub struct MetadataProviders {
    pub tmdb: Option<Arc<TmdbClient>>,
    pub anilist: Option<Arc<AniListClient>>,
    pub shikimori: Option<Arc<ShikimoriClient>>,
}

impl std::fmt::Debug for MetadataProviders {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MetadataProviders")
            .field("tmdb", &self.tmdb.is_some())
            .field("anilist", &self.anilist.is_some())
            .field("shikimori", &self.shikimori.is_some())
            .finish()
    }
}

impl MetadataProviders {
    /// Live clients (HTTPS) sharing one response cache: TMDB with the
    /// build's project key or `tmdb_user_key`, in `language` (`ru-RU`), and
    /// the keyless AniList and Shikimori.
    pub fn live(
        cache: Arc<dyn ProviderCache>,
        tmdb_user_key: Option<&str>,
        language: &str,
    ) -> Self {
        let config = tmdb::TmdbConfig {
            language: language.to_owned(),
            ..Default::default()
        };
        Self {
            tmdb: Some(Arc::new(TmdbClient::live(
                config,
                tmdb_user_key,
                cache.clone(),
            ))),
            anilist: Some(Arc::new(AniListClient::live(
                Default::default(),
                cache.clone(),
            ))),
            shikimori: Some(Arc::new(ShikimoriClient::live(Default::default(), cache))),
        }
    }

    /// The metadata language: TMDB's, `ru-RU` without TMDB.
    pub fn language(&self) -> String {
        self.tmdb
            .as_ref()
            .map_or_else(|| "ru-RU".to_owned(), |t| t.config().language.clone())
    }

    /// TMDB, if it has a key.
    fn tmdb(&self) -> Option<&TmdbClient> {
        self.tmdb.as_deref().filter(|t| t.key_source().is_some())
    }
}

/// When works are asked for again.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MetadataOptions {
    /// Matched works are refreshed after this long (well below TMDB's
    /// six-month cache limit).
    pub refresh_after_ms: i64,
    /// Works no provider knew are retried after this long.
    pub retry_not_found_after_ms: i64,
    /// Works whose fetch failed (network) are retried after this long.
    pub retry_failed_after_ms: i64,
}

impl Default for MetadataOptions {
    fn default() -> Self {
        Self {
            refresh_after_ms: 30 * DAY_MS,
            retry_not_found_after_ms: 7 * DAY_MS,
            retry_failed_after_ms: 3600 * 1000,
        }
    }
}

/// Result of [`Catalog::fetch_metadata`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct MetadataReport {
    /// Works that needed metadata.
    pub works: usize,
    pub matched: usize,
    pub not_found: usize,
    pub failed: usize,
    /// Works merged into another work with the same TMDB id.
    pub merged: usize,
    /// The run was cancelled or stopped early.
    pub stopped: bool,
    /// Why the run stopped early (rejected key, offline), if it did.
    pub error: Option<String>,
}

/// A work as the matcher sees it.
#[derive(Debug, Clone)]
struct WorkInfo {
    id: i64,
    kind: String,
    year: Option<i64>,
    /// All titles, main first.
    titles: Vec<String>,
    tmdb_id: Option<u64>,
    /// IMDb / TVDB ids (from NFO files), looked up when there is no TMDB id.
    imdb_id: Option<String>,
    tvdb_id: Option<String>,
    anilist_id: Option<u64>,
    /// Season numbers of numbered episodes.
    seasons: Vec<u32>,
}

impl WorkInfo {
    fn query(&self) -> WorkQuery {
        WorkQuery {
            kind: self.kind.clone(),
            year: self.year,
            titles: self.titles.clone(),
        }
    }
}

/// A work as a provider search sees it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WorkQuery {
    /// `movie`, `show` or `anime`.
    pub kind: String,
    pub year: Option<i64>,
    /// Titles to search by, main first; the first [`MAX_QUERIES`] are tried.
    pub titles: Vec<String>,
}

/// The search result a work was matched to ([`match_search`]).
#[derive(Debug, Clone, PartialEq)]
pub struct SearchMatch {
    pub result: SearchResult,
    /// Ranking score, at least [`ACCEPT`]; the match confidence.
    pub score: f64,
    /// Title tier of the ranking (`exact`, `loose`…).
    pub tier: String,
    /// The title the result was found by.
    pub query: String,
}

/// What TMDB said about a work.
#[derive(Debug)]
struct TmdbData {
    kind: MediaKind,
    details: Details,
    /// `(ISO 639-1, title)` from translations.
    titles: Vec<(String, String)>,
    /// Seasons of the work's numbered episodes.
    seasons: Vec<Season>,
}

#[derive(Debug)]
struct AnimeData {
    media: Media,
    russian: Option<RussianInfo>,
}

/// The fetched metadata of one work.
#[derive(Debug, Default)]
struct Fetched {
    tmdb: Option<TmdbData>,
    anime: Option<AnimeData>,
    /// Match confidence when a search decided; `None` when the work was
    /// read by a known id (the decision stays as it was).
    confidence: Option<f64>,
    explanation: serde_json::Value,
}

#[derive(Debug)]
enum Outcome {
    Matched(Box<Fetched>),
    NotFound,
    Failed(String),
    /// The run must stop (rejected key).
    Fatal(String),
}

/// A provider failure: transient (retry later) or fatal (stop the run).
enum Failure {
    Transient(String),
    Fatal(String),
}

fn tmdb_failure(e: TmdbError) -> Failure {
    match e {
        TmdbError::Unauthorized => Failure::Fatal("TMDB rejected the API key".into()),
        e => Failure::Transient(format!("TMDB: {e}")),
    }
}

fn fetch_failure(e: FetchError) -> Failure {
    Failure::Transient(e.to_string())
}

/// Works that need metadata, oldest first.
fn pending(
    conn: &Connection,
    language: &str,
    options: &MetadataOptions,
    now: i64,
) -> Result<Vec<i64>, Error> {
    let mut stmt = conn.prepare(
        "SELECT w.id FROM works w
         LEFT JOIN work_metadata wm ON wm.work_id = w.id
         WHERE EXISTS (SELECT 1 FROM matches m WHERE m.work_id = w.id AND m.state <> 'rejected')
           AND (wm.work_id IS NULL
                OR wm.language <> ?1
                OR (wm.outcome = 'matched' AND wm.fetched_at_ms < ?2 - ?3)
                OR (wm.outcome = 'not_found' AND wm.fetched_at_ms < ?2 - ?4)
                OR (wm.outcome = 'failed' AND wm.fetched_at_ms < ?2 - ?5)
                OR (wm.outcome = 'matched' AND EXISTS (
                      SELECT 1 FROM matches m JOIN files f ON f.id = m.file_id
                      WHERE m.work_id = w.id AND m.state <> 'rejected'
                        AND f.added_at_ms > wm.fetched_at_ms)))
         ORDER BY w.id",
    )?;
    let rows = stmt.query_map(
        params![
            language,
            now,
            options.refresh_after_ms,
            options.retry_not_found_after_ms,
            options.retry_failed_after_ms
        ],
        |r| r.get(0),
    )?;
    Ok(rows.collect::<Result<_, _>>()?)
}

fn load_work(conn: &Connection, id: i64) -> Result<Option<WorkInfo>, Error> {
    let Some((kind, title, year)) = conn
        .query_row(
            "SELECT kind, title, year FROM works WHERE id = ?1",
            [id],
            |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, Option<i64>>(2)?,
                ))
            },
        )
        .optional()?
    else {
        return Ok(None);
    };
    let mut titles = vec![title];
    {
        let mut stmt = conn.prepare(
            "SELECT text FROM titles WHERE work_id = ?1
             ORDER BY kind = 'main' DESC, source = 'local' DESC, id",
        )?;
        for t in stmt.query_map([id], |r| r.get::<_, String>(0))? {
            let t = t?;
            if !titles.contains(&t) {
                titles.push(t);
            }
        }
    }
    let ext_str = |provider: &str, ext_kind: &str| -> Result<Option<String>, Error> {
        Ok(conn
            .query_row(
                "SELECT ext_id FROM external_ids
                 WHERE work_id = ?1 AND provider = ?2 AND ext_kind = ?3",
                params![id, provider, ext_kind],
                |r| r.get(0),
            )
            .optional()?)
    };
    let ext = |provider: &str, ext_kind: &str| -> Result<Option<u64>, Error> {
        Ok(ext_str(provider, ext_kind)?.and_then(|v| v.parse().ok()))
    };
    let tmdb_kind = if kind == "movie" { "movie" } else { "tv" };
    let tmdb_id = ext("tmdb", tmdb_kind)?;
    let imdb_id = ext_str("imdb", "")?;
    let tvdb_id = ext_str("tvdb", if kind == "movie" { "movie" } else { "series" })?;
    let anilist_id = ext("anilist", "")?;
    let mut seasons = Vec::new();
    {
        let mut stmt = conn.prepare(
            "SELECT DISTINCT season_number FROM episodes
             WHERE work_id = ?1 AND season_number IS NOT NULL AND episode_number IS NOT NULL
             ORDER BY 1",
        )?;
        for s in stmt.query_map([id], |r| r.get::<_, i64>(0))? {
            if let Ok(s) = u32::try_from(s?) {
                seasons.push(s);
            }
        }
    }
    Ok(Some(WorkInfo {
        id,
        kind,
        year,
        titles,
        tmdb_id,
        imdb_id,
        tvdb_id,
        anilist_id,
        seasons,
    }))
}

/// The query of a work for the title index.
fn title_query(work: &WorkQuery) -> TitleQuery {
    TitleQuery {
        titles: work
            .titles
            .iter()
            .enumerate()
            .map(|(i, t)| (t.clone(), if i == 0 { 1.0 } else { 0.9 }))
            .collect(),
        year: work.year.and_then(|y| u16::try_from(y).ok()),
        season: None,
        media: if work.kind == "movie" {
            MediaHint::Movie
        } else {
            MediaHint::Episode
        },
        anime_hint: work.kind == "anime",
    }
}

/// Candidates compared by [`rank`].
const RANKED: usize = 5;

/// Two results this close are a tie the title cannot break.
const TIE: f64 = 0.01;

/// Confidence factor of a tie between different works (a remake or a
/// same-named work without a year): below [`AUTO`], so the match is
/// `auto_review`.
const AMBIGUOUS: f64 = 0.85;

/// Index of the best record for `work` and its score, if any reaches
/// [`ACCEPT`]. Ties keep the provider's order, but a tie between two works
/// lowers the score below [`AUTO`].
fn rank(work: &WorkQuery, records: Vec<TitleRecord>) -> Option<(usize, f64, String)> {
    if records.is_empty() {
        return None;
    }
    let index = TitleIndex::from_records(records);
    let scored: Vec<_> = index
        .search(&title_query(work), RANKED)
        .into_iter()
        .map(|c| {
            let mut score = f64::from(c.score);
            // A movie of another year is another movie (remakes).
            let record = index.record(c.record);
            if work.kind == "movie"
                && let (Some(a), Some(b)) = (work.year, record.year)
                && a.abs_diff(i64::from(b)) > 1
            {
                score *= 0.8;
            }
            (c, score)
        })
        .collect();
    let mut best = 0;
    for (i, (_, score)) in scored.iter().enumerate() {
        if *score > scored[best].1 {
            best = i;
        }
    }
    let (candidate, mut score) = scored.get(best).cloned()?;
    if scored
        .iter()
        .enumerate()
        .any(|(i, (_, s))| i != best && *s >= score - TIE)
    {
        score *= AMBIGUOUS;
    }
    (score >= ACCEPT).then(|| {
        (
            candidate.record,
            score,
            format!("{:?}", candidate.tier).to_lowercase(),
        )
    })
}

fn tmdb_kind(work: &WorkQuery) -> MediaKind {
    if work.kind == "movie" {
        MediaKind::Movie
    } else {
        MediaKind::Tv
    }
}

/// A TMDB search: `(kind, title, year)` → results.
pub type SearchFn<'a, E> =
    dyn FnMut(MediaKind, &str, Option<u32>) -> Result<Vec<SearchResult>, E> + 'a;

/// Searches a work by its titles and year and ranks the results with the
/// local title index: per title (at most [`MAX_QUERIES`]), `search(kind,
/// title, year)` with the year and, if that finds nothing, without it; the
/// best result scoring at least [`ACCEPT`] wins, a score of [`AUTO`] stops
/// the search. `search` is TMDB `/search/movie` or `/search/tv`: live in a
/// metadata run, recorded or synthetic in `pa-eval`.
pub fn match_search<E>(
    work: &WorkQuery,
    search: &mut SearchFn<'_, E>,
) -> Result<Option<SearchMatch>, E> {
    let kind = tmdb_kind(work);
    let year = work.year.and_then(|y| u32::try_from(y).ok());
    let mut results: Vec<SearchResult> = Vec::new();
    let mut seen = HashSet::new();
    let mut best = None;
    for query in work.titles.iter().take(MAX_QUERIES) {
        let mut years = vec![year];
        if year.is_some() {
            years.push(None);
        }
        for y in years {
            let page = search(kind, query, y)?;
            let found = !page.is_empty();
            for r in page {
                if seen.insert(r.id) {
                    results.push(r);
                }
            }
            if found {
                break;
            }
        }
        let records: Vec<TitleRecord> = results
            .iter()
            .map(|r| {
                tmdb::titles::title_record(r, None, None).unwrap_or_else(|| TitleRecord {
                    titles: vec![r.title.clone().unwrap_or_default()],
                    ..Default::default()
                })
            })
            .collect();
        if let Some((i, score, tier)) = rank(work, records) {
            best = Some(SearchMatch {
                result: results[i].clone(),
                score,
                tier,
                query: query.clone(),
            });
            if score >= AUTO {
                break;
            }
        }
    }
    Ok(best)
}

/// TMDB search for a work ([`match_search`] on the client).
fn tmdb_search(client: &TmdbClient, work: &WorkInfo) -> Result<Option<SearchMatch>, Failure> {
    match_search(&work.query(), &mut |kind, query, year| {
        match kind {
            MediaKind::Movie => client.search_movie(query, year),
            MediaKind::Tv => client.search_tv(query, year),
        }
        .map(|page| page.results)
        .map_err(tmdb_failure)
    })
}

/// The TMDB id of a work that has only an IMDb or TVDB id (an NFO file).
fn tmdb_find(client: &TmdbClient, work: &WorkInfo) -> Result<Option<u64>, Failure> {
    let kind = tmdb_kind(&work.query());
    for (id, source) in [
        (&work.imdb_id, ExternalSource::Imdb),
        (&work.tvdb_id, ExternalSource::Tvdb),
    ] {
        let Some(id) = id else { continue };
        let found = match client.find(id, source) {
            Ok(f) => f,
            Err(TmdbError::NotFound(_)) => continue,
            Err(e) => return Err(tmdb_failure(e)),
        };
        let results = match kind {
            MediaKind::Movie => found.movie_results,
            MediaKind::Tv => found.tv_results,
        };
        if let Some(r) = results.first() {
            return Ok(Some(r.id));
        }
    }
    Ok(None)
}

fn fetch_tmdb(
    client: &TmdbClient,
    work: &WorkInfo,
    explanation: &mut serde_json::Value,
) -> Result<Option<(TmdbData, Option<f64>)>, Failure> {
    let kind = tmdb_kind(&work.query());
    let found = match work.tmdb_id {
        Some(id) => Some(id),
        None => tmdb_find(client, work)?,
    };
    let (id, confidence) = match found {
        // Known ids keep the decision as it is (user, NFO).
        Some(id) => (id, None),
        None => match tmdb_search(client, work)? {
            Some(m) => {
                explanation["tmdb"] = json!({
                    "id": m.result.id,
                    "kind": kind.as_str(),
                    "query": m.query,
                    "score": (m.score * 1000.0).round() / 1000.0,
                    "tier": m.tier,
                    "year": m.result.year(),
                });
                (m.result.id, Some(m.score))
            }
            None => return Ok(None),
        },
    };
    let details = match client.details(kind, id) {
        Ok(d) => d,
        Err(TmdbError::NotFound(_)) => return Ok(None),
        Err(e) => return Err(tmdb_failure(e)),
    };
    let mut titles = Vec::new();
    match client.translations(kind, id) {
        Ok(tr) => {
            for lang in ["en", "ru"] {
                if let Some(t) = tr.find(lang).and_then(|t| t.data.title.clone()) {
                    titles.push((lang.to_owned(), t));
                }
            }
        }
        Err(TmdbError::NotFound(_)) => {}
        Err(e) => return Err(tmdb_failure(e)),
    }
    let mut seasons = Vec::new();
    if kind == MediaKind::Tv {
        for &s in &work.seasons {
            match client.season(id, s) {
                Ok(season) => seasons.push(season),
                Err(TmdbError::NotFound(_)) => {}
                Err(e) => return Err(tmdb_failure(e)),
            }
        }
    }
    Ok(Some((
        TmdbData {
            kind,
            details,
            titles,
            seasons,
        },
        confidence,
    )))
}

fn anilist_record(m: &Media) -> TitleRecord {
    TitleRecord {
        kind: if m.is_movie() {
            WorkKind::Movie
        } else {
            WorkKind::Series
        },
        anime: true,
        year: m.year().and_then(|y| u16::try_from(y).ok()),
        titles: m.all_titles().into_iter().map(str::to_owned).collect(),
        ..Default::default()
    }
}

fn fetch_anime(
    anilist: &AniListClient,
    shikimori: Option<&ShikimoriClient>,
    work: &WorkInfo,
    explanation: &mut serde_json::Value,
) -> Result<Option<(AnimeData, Option<f64>)>, Failure> {
    let (media, confidence) = match work.anilist_id {
        Some(id) => match anilist.media(id) {
            Ok(m) => (m, None),
            Err(FetchError::NotFound { .. }) => return Ok(None),
            Err(e) => return Err(fetch_failure(e)),
        },
        None => {
            let mut found: Vec<Media> = Vec::new();
            let mut best = None;
            for query in work.titles.iter().take(MAX_QUERIES) {
                for m in anilist.search(query).map_err(fetch_failure)? {
                    if !found.iter().any(|f| f.id == m.id) {
                        found.push(m);
                    }
                }
                if let Some((i, score, tier)) =
                    rank(&work.query(), found.iter().map(anilist_record).collect())
                {
                    best = Some((i, score, tier, query.clone()));
                    if score >= AUTO {
                        break;
                    }
                }
            }
            let Some((i, score, tier, query)) = best else {
                return Ok(None);
            };
            let media = found.swap_remove(i);
            explanation["anilist"] = json!({
                "id": media.id,
                "query": query,
                "score": (score * 1000.0).round() / 1000.0,
                "tier": tier,
                "year": media.year(),
            });
            (media, Some(score))
        }
    };
    let russian = match (shikimori, media.id_mal) {
        (Some(s), Some(mal)) => match s.russian(mal) {
            Ok(r) => Some(r),
            Err(FetchError::NotFound { .. }) => None,
            Err(e) => return Err(fetch_failure(e)),
        },
        _ => None,
    };
    Ok(Some((AnimeData { media, russian }, confidence)))
}

/// Everything the providers know about `work`.
fn fetch(providers: &MetadataProviders, work: &WorkInfo) -> Outcome {
    let mut fetched = Fetched {
        explanation: json!({}),
        ..Default::default()
    };
    let mut error = None;
    if let Some(client) = providers.tmdb() {
        match fetch_tmdb(client, work, &mut fetched.explanation) {
            Ok(Some((data, confidence))) => {
                fetched.tmdb = Some(data);
                fetched.confidence = confidence;
            }
            Ok(None) => {}
            Err(Failure::Fatal(e)) => return Outcome::Fatal(e),
            Err(Failure::Transient(e)) => error = Some(e),
        }
    }
    if work.kind == "anime"
        && let Some(anilist) = providers.anilist.as_deref()
    {
        match fetch_anime(
            anilist,
            providers.shikimori.as_deref(),
            work,
            &mut fetched.explanation,
        ) {
            Ok(Some((data, confidence))) => {
                fetched.anime = Some(data);
                if fetched.tmdb.is_none() {
                    fetched.confidence = confidence;
                }
            }
            Ok(None) => {}
            Err(Failure::Fatal(e)) => return Outcome::Fatal(e),
            Err(Failure::Transient(e)) => error = error.or(Some(e)),
        }
    }
    if fetched.tmdb.is_some() || fetched.anime.is_some() {
        Outcome::Matched(Box::new(fetched))
    } else if let Some(e) = error {
        Outcome::Failed(e)
    } else {
        Outcome::NotFound
    }
}

// --- Writing -----------------------------------------------------------------

fn non_empty(s: Option<&str>) -> Option<&str> {
    s.map(str::trim).filter(|s| !s.is_empty())
}

/// `(id, season, episode, absolute, episode_type)` of an episode row.
type EpisodeRow = (i64, Option<i64>, Option<i64>, Option<i64>, String);

/// Moves everything of work `from` to work `into` and deletes `from`.
/// Episodes with the same numbers become one.
fn merge_work(tx: &Transaction<'_>, from: i64, into: i64) -> Result<(), Error> {
    let episodes: Vec<EpisodeRow> = {
        let mut stmt = tx.prepare(
            "SELECT id, season_number, episode_number, absolute_number, episode_type
             FROM episodes WHERE work_id = ?1",
        )?;
        stmt.query_map([from], |r| {
            Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?, r.get(4)?))
        })?
        .collect::<Result<_, _>>()?
    };
    for (id, season, episode, absolute, kind) in episodes {
        let same: Option<i64> = tx
            .query_row(
                "SELECT id FROM episodes
                 WHERE work_id = ?1 AND season_number IS ?2 AND episode_number IS ?3
                   AND absolute_number IS ?4 AND episode_type = ?5
                 ORDER BY id LIMIT 1",
                params![into, season, episode, absolute, kind],
                |r| r.get(0),
            )
            .optional()?;
        match same {
            Some(target) => {
                tx.execute(
                    "UPDATE OR IGNORE match_episodes SET episode_id = ?2 WHERE episode_id = ?1",
                    params![id, target],
                )?;
                tx.execute("DELETE FROM episodes WHERE id = ?1", [id])?;
            }
            None => {
                tx.execute(
                    "UPDATE episodes SET work_id = ?2, season_id = NULL, canonical_ref = NULL
                     WHERE id = ?1",
                    params![id, into],
                )?;
            }
        }
    }
    for sql in [
        "UPDATE matches SET work_id = ?2 WHERE work_id = ?1",
        "UPDATE match_candidates SET work_id = ?2 WHERE work_id = ?1",
        "UPDATE OR IGNORE folder_rules SET work_id = ?2 WHERE work_id = ?1",
        "UPDATE OR IGNORE favorites SET work_id = ?2 WHERE work_id = ?1",
        "UPDATE OR IGNORE collection_items SET work_id = ?2 WHERE work_id = ?1",
        "UPDATE OR IGNORE display_preferences SET work_id = ?2 WHERE work_id = ?1",
        "UPDATE OR IGNORE version_prefs SET work_id = ?2 WHERE work_id = ?1",
        "INSERT OR IGNORE INTO titles (work_id, lang, script, text, kind, source)
         SELECT ?2, lang, script, text, CASE kind WHEN 'main' THEN 'synonym' ELSE kind END, source
         FROM titles WHERE work_id = ?1",
        "INSERT OR IGNORE INTO titles (work_id, text, kind, source)
         SELECT ?2, title, 'synonym', 'local' FROM works WHERE id = ?1",
    ] {
        tx.execute(sql, params![from, into])?;
    }
    tx.execute("DELETE FROM works WHERE id = ?1", [from])?;
    Ok(())
}

/// Sets `provider:ext_kind:ext_id` on `work`. If another work owns the id,
/// `work` is merged into it; returns the work that carries the id now.
fn claim_external(
    tx: &Transaction<'_>,
    work: i64,
    provider: &str,
    ext_kind: &str,
    ext_id: &str,
    merged: &mut usize,
) -> Result<i64, Error> {
    let owner: Option<i64> = tx
        .query_row(
            "SELECT work_id FROM external_ids WHERE provider = ?1 AND ext_kind = ?2 AND ext_id = ?3",
            params![provider, ext_kind, ext_id],
            |r| r.get(0),
        )
        .optional()?;
    match owner {
        Some(o) if o == work => Ok(work),
        Some(o) => {
            merge_work(tx, work, o)?;
            *merged += 1;
            Ok(o)
        }
        None => {
            tx.execute(
                "DELETE FROM external_ids WHERE work_id = ?1 AND provider = ?2 AND ext_kind = ?3",
                params![work, provider, ext_kind],
            )?;
            tx.execute(
                "INSERT INTO external_ids (work_id, provider, ext_kind, ext_id) VALUES (?1, ?2, ?3, ?4)",
                params![work, provider, ext_kind, ext_id],
            )?;
            Ok(work)
        }
    }
}

fn add_title(
    tx: &Transaction<'_>,
    work: i64,
    text: Option<&str>,
    kind: &str,
    lang: &str,
    script: &str,
    source: &str,
) -> Result<(), Error> {
    if let Some(t) = non_empty(text) {
        tx.execute(
            "INSERT OR IGNORE INTO titles (work_id, lang, script, text, kind, source)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6)",
            params![work, lang, script, t, kind, source],
        )?;
    }
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn set_artwork(
    tx: &Transaction<'_>,
    work: Option<i64>,
    episode: Option<i64>,
    kind: &str,
    provider: &str,
    url: Option<String>,
    lang: &str,
    priority: i64,
) -> Result<(), Error> {
    let Some(url) = url.filter(|u| !u.is_empty()) else {
        return Ok(());
    };
    tx.execute(
        "DELETE FROM artwork WHERE work_id IS ?1 AND episode_id IS ?2 AND kind = ?3 AND provider = ?4",
        params![work, episode, kind, provider],
    )?;
    tx.execute(
        "INSERT INTO artwork (work_id, episode_id, kind, provider, url, lang, priority)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params![work, episode, kind, provider, url, lang, priority],
    )?;
    Ok(())
}

fn valid_date(d: Option<&str>) -> Option<&str> {
    d.filter(|d| {
        let b = d.as_bytes();
        b.len() == 10
            && b[4] == b'-'
            && b[7] == b'-'
            && b.iter()
                .enumerate()
                .all(|(i, c)| i == 4 || i == 7 || c.is_ascii_digit())
    })
}

/// AniList descriptions carry `<br>` and `<i>` markup.
fn strip_html(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut in_tag = false;
    let mut tag = String::new();
    for c in s.chars() {
        match (in_tag, c) {
            (false, '<') => {
                in_tag = true;
                tag.clear();
            }
            (true, '>') => {
                in_tag = false;
                if tag.trim_start_matches('/').starts_with("br") {
                    out.push('\n');
                }
            }
            (true, c) => tag.push(c.to_ascii_lowercase()),
            (false, c) => out.push(c),
        }
    }
    let mut text = out.replace("\n\n\n", "\n\n");
    while text.contains("\n\n\n") {
        text = text.replace("\n\n\n", "\n\n");
    }
    text.trim().to_owned()
}

fn lang_of(language: &str) -> &str {
    language.split('-').next().unwrap_or("")
}

fn record_state(
    tx: &Transaction<'_>,
    work: i64,
    outcome: &str,
    language: &str,
    error: Option<&str>,
    now: i64,
) -> Result<(), Error> {
    tx.execute(
        "INSERT INTO work_metadata (work_id, outcome, language, fetched_at_ms, error)
         VALUES (?1, ?2, ?3, ?4, ?5)
         ON CONFLICT (work_id) DO UPDATE SET outcome = excluded.outcome,
             language = excluded.language, fetched_at_ms = excluded.fetched_at_ms,
             error = excluded.error",
        params![work, outcome, language, now, error],
    )?;
    Ok(())
}

/// Writes the metadata of `work` (or of the work it is merged into).
fn apply(
    tx: &Transaction<'_>,
    work: &WorkInfo,
    fetched: &Fetched,
    language: &str,
    now: i64,
    merged: &mut usize,
) -> Result<(), Error> {
    // The work may have been merged away or removed since it was read.
    let exists: Option<i64> = tx
        .query_row("SELECT id FROM works WHERE id = ?1", [work.id], |r| {
            r.get(0)
        })
        .optional()?;
    if exists.is_none() {
        return Ok(());
    }
    let lang = lang_of(language);
    let mut target = work.id;
    if let Some(t) = &fetched.tmdb {
        target = claim_external(
            tx,
            target,
            "tmdb",
            t.kind.as_str(),
            &t.details.id.to_string(),
            merged,
        )?;
        if let Some(imdb) = non_empty(t.details.imdb_id.as_deref())
            && tx
                .query_row(
                    "SELECT 1 FROM external_ids WHERE provider = 'imdb' AND ext_kind = '' AND ext_id = ?1",
                    [imdb],
                    |_| Ok(()),
                )
                .optional()?
                .is_none()
        {
            tx.execute(
                "INSERT OR IGNORE INTO external_ids (work_id, provider, ext_kind, ext_id)
                 VALUES (?1, 'imdb', '', ?2)",
                params![target, imdb],
            )?;
        }
    }
    if let Some(a) = &fetched.anime {
        target = claim_external(tx, target, "anilist", "", &a.media.id.to_string(), merged)?;
        for (provider, id) in [
            ("mal", a.media.id_mal),
            ("shikimori", a.russian.as_ref().map(|r| r.id)),
        ] {
            if let Some(id) = id {
                tx.execute(
                    "INSERT OR IGNORE INTO external_ids (work_id, provider, ext_kind, ext_id)
                     VALUES (?1, ?2, '', ?3)",
                    params![target, provider, id.to_string()],
                )?;
            }
        }
    }

    // Titles: provider rows are rewritten; local and user titles stay.
    tx.execute(
        "DELETE FROM titles WHERE work_id = ?1 AND source IN ('tmdb', 'anilist', 'shikimori')",
        [target],
    )?;
    let tmdb = fetched.tmdb.as_ref().map(|t| &t.details);
    let anime = fetched.anime.as_ref();
    let russian = anime.and_then(|a| a.russian.as_ref());
    let main_title = tmdb
        .and_then(|d| non_empty(d.title.as_deref()))
        .or_else(|| {
            if lang == "ru" {
                russian.and_then(|r| non_empty(r.title.as_deref()))
            } else {
                None
            }
        })
        .or_else(|| {
            anime.and_then(|a| {
                non_empty(a.media.title.english.as_deref())
                    .or(non_empty(a.media.title.romaji.as_deref()))
            })
        })
        .map(str::to_owned);
    let original_title = tmdb
        .and_then(|d| non_empty(d.original_title.as_deref()))
        .or_else(|| anime.and_then(|a| non_empty(a.media.title.native.as_deref())))
        .map(str::to_owned);
    if let Some(title) = &main_title {
        tx.execute(
            "UPDATE titles SET kind = 'synonym' WHERE work_id = ?1 AND kind = 'main'",
            [target],
        )?;
        let source = if tmdb.is_some() { "tmdb" } else { "anilist" };
        add_title(tx, target, Some(title), "main", lang, "", source)?;
    }
    if let Some(d) = tmdb {
        let orig_lang = d.original_language.as_deref().unwrap_or("");
        add_title(
            tx,
            target,
            d.original_title.as_deref(),
            "official",
            orig_lang,
            "",
            "tmdb",
        )?;
    }
    if let Some(t) = &fetched.tmdb {
        for (l, title) in &t.titles {
            add_title(tx, target, Some(title), "official", l, "", "tmdb")?;
        }
    }
    if let Some(a) = anime {
        let t = &a.media.title;
        add_title(
            tx,
            target,
            t.romaji.as_deref(),
            "romaji",
            "ja",
            "Latn",
            "anilist",
        )?;
        add_title(
            tx,
            target,
            t.english.as_deref(),
            "official",
            "en",
            "",
            "anilist",
        )?;
        add_title(
            tx,
            target,
            t.native.as_deref(),
            "official",
            "ja",
            "",
            "anilist",
        )?;
        for s in &a.media.synonyms {
            add_title(tx, target, Some(s), "synonym", "", "", "anilist")?;
        }
        if let Some(r) = &a.russian {
            add_title(
                tx,
                target,
                r.title.as_deref(),
                "official",
                "ru",
                "",
                "shikimori",
            )?;
            add_title(
                tx,
                target,
                r.license_name.as_deref(),
                "official",
                "ru",
                "",
                "shikimori",
            )?;
        }
    }

    // The work row.
    let overview = tmdb
        .and_then(|d| non_empty(d.overview.as_deref()))
        .map(str::to_owned)
        .or_else(|| {
            if lang == "ru" {
                russian.and_then(|r| non_empty(r.description.as_deref()).map(str::to_owned))
            } else {
                None
            }
        })
        .or_else(|| {
            anime
                .and_then(|a| a.media.description.as_deref())
                .map(strip_html)
                .filter(|s| !s.is_empty())
        });
    let year = tmdb
        .and_then(Details::year)
        .or_else(|| anime.and_then(|a| a.media.year()))
        .map(i64::from)
        .filter(|y| (1800..=3000).contains(y));
    let runtime_min = tmdb
        .and_then(|d| d.runtime.or_else(|| d.episode_run_time.first().copied()))
        .or_else(|| anime.and_then(|a| a.media.duration))
        .filter(|m| *m > 0);
    let status = tmdb
        .and_then(|d| d.status.clone())
        .or_else(|| anime.and_then(|a| a.media.status.clone()));
    let original_lang =
        tmdb.and_then(|d| non_empty(d.original_language.as_deref()).map(str::to_owned));
    tx.execute(
        "UPDATE works SET title = ifnull(?2, title), original_title = ifnull(?3, original_title),
             original_lang = ifnull(?4, original_lang), year = ifnull(?5, year),
             overview = ifnull(?6, overview), runtime_ms = ifnull(?7, runtime_ms),
             status = ifnull(?8, status), updated_at_ms = ?9
         WHERE id = ?1",
        params![
            target,
            main_title,
            original_title,
            original_lang,
            year,
            overview,
            runtime_min.map(|m| i64::from(m) * 60_000),
            status,
            now
        ],
    )?;
    if let Some(a) = anime
        && let Some(format) = a.media.format.as_deref()
    {
        let anime_type = match format {
            "TV" | "TV_SHORT" => Some("tv"),
            "MOVIE" => Some("movie"),
            "OVA" => Some("ova"),
            "ONA" => Some("ona"),
            "SPECIAL" => Some("special"),
            "MUSIC" => Some("music_video"),
            _ => None,
        };
        tx.execute(
            "UPDATE works SET anime_type = ?2 WHERE id = ?1 AND kind = 'anime'",
            params![target, anime_type],
        )?;
    }

    // Genres (LIB-009): TMDB names are in the metadata language; AniList
    // names (English) only when TMDB has none.
    let tmdb_genres: Vec<&str> = tmdb
        .map(|d| d.genres.iter().map(|g| g.name.as_str()).collect())
        .unwrap_or_default();
    let anilist_genres: Vec<&str> = match anime {
        Some(a) if tmdb_genres.is_empty() => a.media.genres.iter().map(String::as_str).collect(),
        _ => Vec::new(),
    };
    pa_library::views::set_work_genres(tx, target, "tmdb", &tmdb_genres)?;
    pa_library::views::set_work_genres(tx, target, "anilist", &anilist_genres)?;

    // TMDB collection of a movie (LIB-010).
    if let Some(t) = &fetched.tmdb
        && t.kind.as_str() == "movie"
    {
        let collection = t
            .details
            .belongs_to_collection
            .as_ref()
            .map(|c| (c.id, c.name.as_deref().unwrap_or("")));
        crate::lists::link_tmdb_collection(tx, target, collection, now)?;
    }

    // Artwork.
    tx.execute(
        "DELETE FROM artwork WHERE work_id = ?1 AND provider IN ('tmdb', 'anilist')",
        [target],
    )?;
    if let Some(d) = tmdb {
        let poster = d
            .poster_path
            .as_deref()
            .map(|p| tmdb::image_url(POSTER_SIZE, p));
        let backdrop = d
            .backdrop_path
            .as_deref()
            .map(|p| tmdb::image_url(BACKDROP_SIZE, p));
        set_artwork(tx, Some(target), None, "poster", "tmdb", poster, lang, 0)?;
        set_artwork(tx, Some(target), None, "backdrop", "tmdb", backdrop, "", 0)?;
    }
    if let Some(a) = anime {
        let cover = a.media.cover_image.as_ref().and_then(|c| c.large.clone());
        set_artwork(tx, Some(target), None, "poster", "anilist", cover, "", 10)?;
    }

    // Episodes of the TMDB seasons.
    if let Some(t) = &fetched.tmdb {
        for season in &t.seasons {
            for ep in &season.episodes {
                let rows: Vec<i64> = {
                    let mut stmt = tx.prepare(
                        "SELECT id FROM episodes
                         WHERE work_id = ?1 AND season_number = ?2 AND episode_number = ?3
                           AND episode_type = 'regular'",
                    )?;
                    stmt.query_map(params![target, ep.season_number, ep.episode_number], |r| {
                        r.get(0)
                    })?
                    .collect::<Result<_, _>>()?
                };
                for id in rows {
                    tx.execute(
                        "UPDATE episodes SET title = ifnull(?2, title), overview = ifnull(?3, overview),
                             air_date = ifnull(?4, air_date), runtime_ms = ifnull(?5, runtime_ms)
                         WHERE id = ?1",
                        params![
                            id,
                            non_empty(ep.name.as_deref()),
                            non_empty(ep.overview.as_deref()),
                            valid_date(ep.air_date.as_deref()),
                            ep.runtime.filter(|m| *m > 0).map(|m| i64::from(m) * 60_000),
                        ],
                    )?;
                    let canonical = ep.id.to_string();
                    tx.execute(
                        "UPDATE episodes SET canonical_ref = ?2 WHERE id = ?1
                           AND NOT EXISTS (SELECT 1 FROM episodes e2
                                           WHERE e2.work_id = ?3 AND e2.canonical_ref = ?2)",
                        params![id, canonical, target],
                    )?;
                    let still = ep
                        .still_path
                        .as_deref()
                        .map(|p| tmdb::image_url(STILL_SIZE, p));
                    set_artwork(tx, None, Some(id), "still", "tmdb", still, "", 0)?;
                }
            }
        }
    }

    // Parser decisions become scorer decisions; user decisions stay.
    if let Some(c) = fetched.confidence {
        let c = c.clamp(0.0, 1.0);
        let state = if c >= AUTO { "auto" } else { "auto_review" };
        tx.execute(
            "UPDATE matches SET decided_by = 'scorer', model_version = ?2, confidence = ?3,
                 state = ?4, explanation_json = ?5, updated_at_ms = ?6
             WHERE work_id = ?1 AND state IN ('auto', 'auto_review')",
            params![
                target,
                MODEL_VERSION,
                c,
                state,
                fetched.explanation.to_string(),
                now
            ],
        )?;
    }
    record_state(tx, target, "matched", language, None, now)?;
    Ok(())
}

impl Catalog {
    /// Fetches provider metadata for works that need it (see the [module
    /// docs](self)). Runs are serialized with scans. `on_progress(done,
    /// total)` is called after every work.
    pub fn fetch_metadata(
        &self,
        providers: &MetadataProviders,
        options: &MetadataOptions,
        cancel: &CancelToken,
        on_progress: &mut dyn FnMut(usize, usize),
    ) -> Result<MetadataReport, Error> {
        let _serial = self.scan_lock.lock().unwrap_or_else(|e| e.into_inner());
        let language = providers.language();
        let mut report = MetadataReport::default();
        if providers.tmdb().is_none() && providers.anilist.is_none() {
            return Ok(report);
        }
        let works: Vec<WorkInfo> = {
            let conn = self.lock();
            let ids = pending(&conn, &language, options, crate::now_ms())?;
            let mut works = Vec::with_capacity(ids.len());
            for id in ids {
                if let Some(w) = load_work(&conn, id)? {
                    // Works without TMDB support (non-anime without a TMDB
                    // key) are not asked for.
                    if providers.tmdb().is_some() || w.kind == "anime" {
                        works.push(w);
                    }
                }
            }
            works
        };
        report.works = works.len();
        let total = works.len();
        on_progress(0, total);
        if total == 0 {
            return Ok(report);
        }

        let next = AtomicUsize::new(0);
        let stop = AtomicBool::new(false);
        let (tx, rx) = mpsc::channel::<(usize, Outcome)>();
        std::thread::scope(|scope| -> Result<(), Error> {
            for _ in 0..WORKERS.min(total) {
                let tx = tx.clone();
                let (next, stop, works) = (&next, &stop, &works);
                scope.spawn(move || {
                    loop {
                        if stop.load(Ordering::Relaxed) || cancel.is_cancelled() {
                            break;
                        }
                        let i = next.fetch_add(1, Ordering::Relaxed);
                        let Some(work) = works.get(i) else { break };
                        if tx.send((i, fetch(providers, work))).is_err() {
                            break;
                        }
                    }
                });
            }
            drop(tx);
            let mut done = 0;
            let mut failures_in_a_row = 0;
            let result = (|| -> Result<(), Error> {
                for (i, outcome) in &rx {
                    if stop.load(Ordering::Relaxed) {
                        continue;
                    }
                    let work = &works[i];
                    let now = crate::now_ms();
                    let mut conn = self.lock();
                    let t = conn.transaction()?;
                    let exists = t
                        .query_row("SELECT 1 FROM works WHERE id = ?1", [work.id], |_| Ok(()))
                        .optional()?
                        .is_some();
                    match outcome {
                        Outcome::Matched(fetched) => {
                            failures_in_a_row = 0;
                            report.matched += 1;
                            apply(&t, work, &fetched, &language, now, &mut report.merged)?;
                        }
                        Outcome::NotFound => {
                            failures_in_a_row = 0;
                            report.not_found += 1;
                            if exists {
                                record_state(&t, work.id, "not_found", &language, None, now)?;
                            }
                        }
                        Outcome::Failed(e) => {
                            report.failed += 1;
                            failures_in_a_row += 1;
                            if exists {
                                record_state(&t, work.id, "failed", &language, Some(&e), now)?;
                            }
                            if failures_in_a_row >= MAX_FAILURES_IN_A_ROW {
                                stop.store(true, Ordering::Relaxed);
                                report.stopped = true;
                                report.error = Some(e);
                            }
                        }
                        Outcome::Fatal(e) => {
                            stop.store(true, Ordering::Relaxed);
                            report.stopped = true;
                            report.error = Some(e);
                        }
                    }
                    t.commit()?;
                    drop(conn);
                    done += 1;
                    on_progress(done, total);
                }
                Ok(())
            })();
            if result.is_err() {
                stop.store(true, Ordering::Relaxed);
                // Let the workers see the stop and the channel drain.
                for _ in rx.try_iter() {}
            }
            result
        })?;
        if cancel.is_cancelled() {
            report.stopped = true;
        }
        pa_library::search::sync(&self.lock())?;
        Ok(report)
    }

    /// Forgets the metadata fetch state so the next
    /// [`fetch_metadata`](Self::fetch_metadata) asks again: of every work
    /// (`all`) or of works no provider matched (after a new API key).
    pub fn reset_metadata(&self, all: bool) -> Result<(), Error> {
        let conn = self.lock();
        if all {
            conn.execute("DELETE FROM work_metadata", [])?;
        } else {
            conn.execute("DELETE FROM work_metadata WHERE outcome <> 'matched'", [])?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_anilist_markup() {
        assert_eq!(
            strip_html("Line one.<br><br>\n<i>Line</i> two."),
            "Line one.\n\nLine two."
        );
    }

    #[test]
    fn dates_are_checked() {
        assert_eq!(valid_date(Some("2021-04-02")), Some("2021-04-02"));
        assert_eq!(valid_date(Some("2021")), None);
        assert_eq!(valid_date(Some("2021-4-2x")), None);
    }

    #[test]
    fn ranking_prefers_title_and_year() {
        let work = WorkQuery {
            kind: "movie".into(),
            year: Some(2021),
            titles: vec!["Dune".into()],
        };
        let rec = |title: &str, year: u16| TitleRecord {
            kind: WorkKind::Movie,
            year: Some(year),
            titles: vec![title.into()],
            ..Default::default()
        };
        let (i, score, _) = rank(&work, vec![rec("Dune", 1984), rec("Dune", 2021)]).unwrap();
        assert_eq!(i, 1);
        assert!(score >= AUTO, "{score}");
        assert!(rank(&work, vec![rec("Dunkirk", 2017)]).is_none());
        // Without a year two same-named works tie: not auto.
        let no_year = WorkQuery {
            year: None,
            ..work.clone()
        };
        let (i, score, _) = rank(&no_year, vec![rec("Dune", 2021), rec("Dune", 1984)]).unwrap();
        assert_eq!(i, 0, "the provider's order breaks the tie");
        assert!((ACCEPT..AUTO).contains(&score), "{score}");
        // Only a remake of another year: penalised below auto.
        let (_, score, _) = rank(&work, vec![rec("Dune", 1984)]).unwrap_or((0, 0.0, String::new()));
        assert!(score < AUTO, "{score}");
    }

    #[test]
    fn match_search_tries_the_year_then_without_it() {
        let work = WorkQuery {
            kind: "movie".into(),
            year: Some(2021),
            titles: vec!["Dune".into()],
        };
        let result = |id: u64, year: &str| SearchResult {
            id,
            media_type: Some("movie".into()),
            title: Some("Dune".into()),
            original_title: None,
            original_language: None,
            release_date: Some(format!("{year}-01-01")),
            origin_country: vec![],
            genre_ids: vec![],
            overview: None,
            poster_path: None,
            backdrop_path: None,
            popularity: 0.0,
            adult: false,
        };
        let mut calls = Vec::new();
        let m = match_search::<()>(&work, &mut |kind, q, y| {
            calls.push((kind, q.to_owned(), y));
            Ok(match y {
                Some(_) => vec![],
                None => vec![result(841, "1984"), result(438631, "2021")],
            })
        })
        .unwrap()
        .unwrap();
        assert_eq!(m.result.id, 438631);
        assert_eq!(m.query, "Dune");
        assert_eq!(
            calls,
            [
                (MediaKind::Movie, "Dune".to_owned(), Some(2021)),
                (MediaKind::Movie, "Dune".to_owned(), None)
            ]
        );
        assert_eq!(
            match_search::<()>(&work, &mut |_, _, _| Ok(vec![])).unwrap(),
            None
        );
    }
}
