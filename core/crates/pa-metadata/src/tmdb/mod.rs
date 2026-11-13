// SPDX-License-Identifier: MPL-2.0
//! TMDB v3 client (META-009).
//!
//! Everything goes through one path, [`TmdbClient::get_json`]:
//!
//! 1. **Cache** ([`ProviderCache`]): a fresh entry is returned without
//!    network. Keys are credential-free (`tmdb:/movie/1?language=ru-RU`).
//! 2. **Key** ([`Keys`]): the user's key, else the project key. With no key
//!    the client serves a stale cache entry or fails with
//!    [`TmdbError::NoKey`]; the app keeps working on recognised names.
//! 3. **Rate limit** ([`RateLimiter`]): token bucket, by default 20 req/s
//!    with a burst of 20 — half of TMDB's published ~40 req/s.
//! 4. **Retries** ([`RetryPolicy`]): HTTP 429 → wait `Retry-After` (or
//!    exponential backoff), pause the whole client, retry; 5xx → the same.
//!    401 / 404 / other 4xx are not retried.
//! 5. On a network error or an exhausted retry budget a **stale** cache
//!    entry (younger than [`MAX_CACHE_AGE`], six months) is served instead.
//!
//! Terms (docs/legal/providers.md, TMDB): attribution
//! ([`attribution::TMDB`](crate::attribution::TMDB)), no caching beyond six
//! months ([`TmdbClient::purge_expired`] at startup), all data deleted when
//! the licence ends ([`TmdbClient::purge_all`]).
//!
//! The client is `Send + Sync`; methods take `&self` and block, so async
//! callers run them on a blocking pool. It also implements the core's
//! [`Provider`](pa_recognition::provider::Provider) — see [`provider`].

pub mod model;
pub mod provider;
pub mod titles;

use std::sync::{Arc, RwLock};
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::cache::{CacheEntry, MAX_CACHE_AGE, ProviderCache};
use crate::clock::Clock;
use crate::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::key::{KeySource, Keys};
use crate::rate::{RateLimit, RateLimiter, RetryPolicy, parse_retry_after};

pub use model::*;

/// Provider id in cache rows and neutral requests.
pub const PROVIDER: &str = "tmdb";

/// Production API base.
pub const DEFAULT_BASE_URL: &str = "https://api.themoviedb.org/3";

/// Image CDN base.
pub const IMAGE_BASE_URL: &str = "https://image.tmdb.org/t/p";

/// URL of an image (`size`: `w92`…`w780`, `original`).
pub fn image_url(size: &str, file_path: &str) -> String {
    format!("{IMAGE_BASE_URL}/{size}{file_path}")
}

/// How long responses stay fresh. Every value is capped at
/// [`MAX_CACHE_AGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheTtl {
    /// Searches and `/find`.
    pub search: Duration,
    /// Movies, ended series, collections, translations, titles, ids, images.
    pub stable: Duration,
    /// Series that still air (`Returning Series`, `In Production`, …).
    pub ongoing: Duration,
    /// Seasons and episode groups.
    pub tv_parts: Duration,
}

impl Default for CacheTtl {
    /// Research 03 §4.7: metadata 7–30 days, ongoing ~1 day.
    fn default() -> Self {
        const DAY: u64 = 24 * 3600;
        Self {
            search: Duration::from_secs(DAY),
            stable: Duration::from_secs(14 * DAY),
            ongoing: Duration::from_secs(DAY),
            tv_parts: Duration::from_secs(3 * DAY),
        }
    }
}

/// Client settings.
#[derive(Debug, Clone)]
pub struct TmdbConfig {
    /// API base URL.
    pub base_url: String,
    /// Language of titles and overviews (`ru-RU`, `en-US`).
    pub language: String,
    /// Include adult titles in searches (off; store builds must keep it off).
    pub include_adult: bool,
    /// Client-side rate limit.
    pub rate_limit: RateLimit,
    /// Retry policy for 429 and 5xx.
    pub retry: RetryPolicy,
    /// Freshness of cached responses.
    pub ttl: CacheTtl,
}

impl Default for TmdbConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            language: "ru-RU".to_owned(),
            include_adult: false,
            rate_limit: RateLimit {
                burst: 20,
                per_second: 20.0,
            },
            retry: RetryPolicy::default(),
            ttl: CacheTtl::default(),
        }
    }
}

/// TMDB failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum TmdbError {
    /// Neither a user key nor a project key is configured.
    #[error("no TMDB API key configured")]
    NoKey,
    /// TMDB rejected the key (HTTP 401).
    #[error("TMDB rejected the API key")]
    Unauthorized,
    /// The resource does not exist (HTTP 404).
    #[error("not found on TMDB: {0}")]
    NotFound(String),
    /// Still HTTP 429 after all retries.
    #[error("TMDB rate limit: HTTP 429 after {attempts} attempts")]
    RateLimited {
        /// Requests made.
        attempts: u32,
        /// Last `Retry-After`.
        retry_after: Option<Duration>,
    },
    /// Other HTTP error.
    #[error("TMDB HTTP {status}: {message}")]
    Http {
        /// Status code.
        status: u16,
        /// TMDB `status_message` or a short description.
        message: String,
    },
    /// Network failure (no response at all).
    #[error("network error: {0}")]
    Transport(String),
    /// The response did not have the expected shape.
    #[error("unexpected TMDB response: {0}")]
    Decode(String),
}

/// Which TTL applies to a response.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum TtlClass {
    Search,
    Stable,
    /// TV details: ongoing or stable depending on `status`.
    TvDetails,
    TvParts,
}

/// The TMDB client. See the [module docs](self).
pub struct TmdbClient {
    config: TmdbConfig,
    keys: RwLock<Keys>,
    transport: Arc<dyn HttpTransport>,
    cache: Arc<dyn ProviderCache>,
    clock: Arc<dyn Clock>,
    limiter: RateLimiter,
}

impl std::fmt::Debug for TmdbClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TmdbClient")
            .field("config", &self.config)
            .field("keys", &self.keys)
            .finish_non_exhaustive()
    }
}

impl TmdbClient {
    /// A client with injected transport, cache and clock.
    pub fn new(
        config: TmdbConfig,
        keys: Keys,
        transport: Arc<dyn HttpTransport>,
        cache: Arc<dyn ProviderCache>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let limiter = RateLimiter::new(config.rate_limit);
        Self {
            config,
            keys: RwLock::new(keys),
            transport,
            cache,
            clock,
            limiter,
        }
    }

    /// A live client: HTTPS via `ureq`, the system clock, the build's project
    /// key (if any) and the given cache.
    #[cfg(feature = "http")]
    pub fn live(config: TmdbConfig, user_key: Option<&str>, cache: Arc<dyn ProviderCache>) -> Self {
        Self::new(
            config,
            Keys::tmdb_from_build().with_user_key(user_key),
            Arc::new(crate::http::UreqTransport::new(crate::USER_AGENT)),
            cache,
            Arc::new(crate::clock::SystemClock::default()),
        )
    }

    /// Settings in use.
    pub fn config(&self) -> &TmdbConfig {
        &self.config
    }

    /// Sets or clears the user's own key ("свой ключ"); takes effect for the
    /// next request. Cached data stays valid: it does not depend on the key.
    pub fn set_user_key(&self, key: Option<&str>) {
        let mut keys = self.keys.write().unwrap_or_else(|e| e.into_inner());
        *keys = keys.clone().with_user_key(key);
    }

    /// Where the active key comes from; `None` when there is no key.
    pub fn key_source(&self) -> Option<KeySource> {
        let keys = self.keys.read().unwrap_or_else(|e| e.into_inner());
        keys.active().map(|(_, s)| s)
    }

    /// Deletes cached TMDB responses older than six months (call at
    /// startup and periodically). Returns how many were removed.
    pub fn purge_expired(&self) -> usize {
        let cutoff = self
            .clock
            .unix_secs()
            .saturating_sub(MAX_CACHE_AGE.as_secs());
        self.cache.purge_older_than(cutoff).unwrap_or(0)
    }

    /// Deletes all cached TMDB data (licence terminated, user opt-out).
    pub fn purge_all(&self) -> usize {
        self.cache.purge_provider(PROVIDER).unwrap_or(0)
    }

    // --- Endpoints -------------------------------------------------------

    /// `/search/movie`.
    pub fn search_movie(&self, query: &str, year: Option<u32>) -> Result<SearchPage, TmdbError> {
        let mut q = self.search_params(query);
        if let Some(y) = year {
            q.push(("year".into(), y.to_string()));
        }
        let mut page: SearchPage = self.get_json("/search/movie", q, TtlClass::Search)?;
        tag_kind(&mut page.results, MediaKind::Movie);
        Ok(page)
    }

    /// `/search/tv`; `year` filters by the first air date.
    pub fn search_tv(&self, query: &str, year: Option<u32>) -> Result<SearchPage, TmdbError> {
        let mut q = self.search_params(query);
        if let Some(y) = year {
            q.push(("first_air_date_year".into(), y.to_string()));
        }
        let mut page: SearchPage = self.get_json("/search/tv", q, TtlClass::Search)?;
        tag_kind(&mut page.results, MediaKind::Tv);
        Ok(page)
    }

    /// `/search/multi`: movies and series (people are dropped).
    pub fn search_multi(&self, query: &str) -> Result<SearchPage, TmdbError> {
        let mut page: SearchPage =
            self.get_json("/search/multi", self.search_params(query), TtlClass::Search)?;
        page.results.retain(|r| r.kind().is_some());
        Ok(page)
    }

    /// `/movie/{id}` or `/tv/{id}` in the configured language.
    pub fn details(&self, kind: MediaKind, id: u64) -> Result<Details, TmdbError> {
        let class = match kind {
            MediaKind::Movie => TtlClass::Stable,
            MediaKind::Tv => TtlClass::TvDetails,
        };
        self.get_json(&format!("/{}/{id}", kind.as_str()), self.lang(), class)
    }

    /// `/{kind}/{id}/translations`: titles and overviews in every language
    /// (e.g. `ru-RU` via [`Translations::find`]).
    pub fn translations(&self, kind: MediaKind, id: u64) -> Result<Translations, TmdbError> {
        let path = format!("/{}/{id}/translations", kind.as_str());
        self.get_json(&path, Vec::new(), TtlClass::Stable)
    }

    /// `/{kind}/{id}/alternative_titles` (all countries).
    pub fn alternative_titles(
        &self,
        kind: MediaKind,
        id: u64,
    ) -> Result<AlternativeTitles, TmdbError> {
        let path = format!("/{}/{id}/alternative_titles", kind.as_str());
        self.get_json(&path, Vec::new(), TtlClass::Stable)
    }

    /// `/{kind}/{id}/external_ids` (IMDb, TVDB, Wikidata).
    pub fn external_ids(&self, kind: MediaKind, id: u64) -> Result<ExternalIds, TmdbError> {
        let path = format!("/{}/{id}/external_ids", kind.as_str());
        self.get_json(&path, Vec::new(), TtlClass::Stable)
    }

    /// `/{kind}/{id}/images`: images in the configured language, English
    /// and textless ones.
    pub fn images(&self, kind: MediaKind, id: u64) -> Result<Images, TmdbError> {
        let lang = self.config.language.split('-').next().unwrap_or("en");
        let mut langs = vec![lang, "en", "null"];
        langs.dedup();
        let q = vec![("include_image_language".into(), langs.join(","))];
        self.get_json(
            &format!("/{}/{id}/images", kind.as_str()),
            q,
            TtlClass::Stable,
        )
    }

    /// `/collection/{id}`.
    pub fn collection(&self, id: u64) -> Result<Collection, TmdbError> {
        let mut c: Collection =
            self.get_json(&format!("/collection/{id}"), self.lang(), TtlClass::Stable)?;
        tag_kind(&mut c.parts, MediaKind::Movie);
        Ok(c)
    }

    /// `/tv/{id}/season/{n}`.
    pub fn season(&self, tv_id: u64, season: u32) -> Result<Season, TmdbError> {
        let path = format!("/tv/{tv_id}/season/{season}");
        self.get_json(&path, self.lang(), TtlClass::TvParts)
    }

    /// `/tv/{id}/episode_groups`: alternative orderings (absolute, DVD, …).
    pub fn episode_groups(&self, tv_id: u64) -> Result<EpisodeGroupList, TmdbError> {
        let path = format!("/tv/{tv_id}/episode_groups");
        self.get_json(&path, Vec::new(), TtlClass::TvParts)
    }

    /// `/tv/episode_group/{id}`: one ordering with its episodes.
    pub fn episode_group(&self, group_id: &str) -> Result<EpisodeGroup, TmdbError> {
        if group_id.is_empty() || !group_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(TmdbError::NotFound(format!("episode group {group_id:?}")));
        }
        let path = format!("/tv/episode_group/{group_id}");
        self.get_json(&path, self.lang(), TtlClass::TvParts)
    }

    /// `/find/{id}`: TMDB entries for an IMDb / TVDB / Wikidata id.
    pub fn find(
        &self,
        external_id: &str,
        source: ExternalSource,
    ) -> Result<FindResults, TmdbError> {
        if external_id.is_empty() || !external_id.chars().all(|c| c.is_ascii_alphanumeric()) {
            return Err(TmdbError::NotFound(format!("external id {external_id:?}")));
        }
        let mut q = self.lang();
        q.push(("external_source".into(), source.as_str().into()));
        let mut found: FindResults =
            self.get_json(&format!("/find/{external_id}"), q, TtlClass::Search)?;
        tag_kind(&mut found.movie_results, MediaKind::Movie);
        tag_kind(&mut found.tv_results, MediaKind::Tv);
        Ok(found)
    }

    // --- Plumbing --------------------------------------------------------

    fn lang(&self) -> Vec<(String, String)> {
        vec![("language".into(), self.config.language.clone())]
    }

    fn search_params(&self, query: &str) -> Vec<(String, String)> {
        let mut q = self.lang();
        q.push(("query".into(), query.trim().to_owned()));
        q.push((
            "include_adult".into(),
            self.config.include_adult.to_string(),
        ));
        q
    }

    /// GET `path` with `query`, through cache, key, rate limit and retries,
    /// and decode the JSON body.
    pub(crate) fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Vec<(String, String)>,
        class: TtlClass,
    ) -> Result<T, TmdbError> {
        let body = self.get_body(path, query, class)?;
        serde_json::from_str(&body).map_err(|e| TmdbError::Decode(format!("{path}: {e}")))
    }

    pub(crate) fn get_body(
        &self,
        path: &str,
        query: Vec<(String, String)>,
        class: TtlClass,
    ) -> Result<String, TmdbError> {
        let mut request = HttpRequest {
            base: self.config.base_url.trim_end_matches('/').to_owned(),
            path: path.to_owned(),
            query,
            headers: vec![("accept".into(), "application/json".into())],
        };
        let cache_key = format!("{PROVIDER}:{}", request.canonical());
        let now = self.clock.unix_secs();
        let cached = self.cache.get(&cache_key).ok().flatten();
        if let Some(entry) = &cached
            && entry.is_fresh(now)
        {
            return Ok(entry.body.clone());
        }
        let stale = cached.filter(|e| e.is_usable(now)).map(|e| e.body);

        let secret = {
            let keys = self.keys.read().unwrap_or_else(|e| e.into_inner());
            match keys.active() {
                Some((key, _)) => key.clone(),
                None => return stale.ok_or(TmdbError::NoKey),
            }
        };
        if secret.is_bearer_token() {
            request.headers.push((
                "authorization".into(),
                format!("Bearer {}", secret.expose()),
            ));
        } else {
            request
                .query
                .push(("api_key".into(), secret.expose().to_owned()));
        }

        match self.send_with_retries(&request) {
            Ok(response) => {
                let ttl = self.ttl_for(class, &response.body).min(MAX_CACHE_AGE);
                let fetched_at = self.clock.unix_secs();
                let entry = CacheEntry {
                    body: response.body,
                    fetched_at,
                    expires_at: fetched_at.saturating_add(ttl.as_secs()),
                };
                // A cache write failure must not fail the request.
                let _ = self.cache.put(PROVIDER, &cache_key, &entry);
                Ok(entry.body)
            }
            Err(e @ (TmdbError::Transport(_) | TmdbError::RateLimited { .. })) => {
                stale.ok_or_else(|| redact(e, secret.expose()))
            }
            Err(TmdbError::Http { status, message }) if status >= 500 => {
                stale.ok_or(TmdbError::Http { status, message })
            }
            Err(e) => Err(redact(e, secret.expose())),
        }
    }

    fn send_with_retries(&self, request: &HttpRequest) -> Result<HttpResponse, TmdbError> {
        let policy = self.config.retry;
        let mut retry = 0;
        loop {
            self.limiter.acquire(self.clock.as_ref());
            let response = self
                .transport
                .get(request)
                .map_err(|e| TmdbError::Transport(e.0))?;
            let status = response.status;
            if (200..300).contains(&status) {
                return Ok(response);
            }
            let retryable = status == 429 || matches!(status, 500 | 502 | 503 | 504);
            if !retryable {
                return Err(http_error(request, &response));
            }
            let retry_after = parse_retry_after(response.header("retry-after"));
            if retry >= policy.max_retries {
                return Err(if status == 429 {
                    TmdbError::RateLimited {
                        attempts: retry + 1,
                        retry_after,
                    }
                } else {
                    http_error(request, &response)
                });
            }
            let delay = policy.delay(retry, retry_after);
            if status == 429 {
                // Everyone waits, not only this caller.
                self.limiter.pause_for(self.clock.as_ref(), delay);
            } else {
                self.clock.sleep(delay);
            }
            retry += 1;
        }
    }

    fn ttl_for(&self, class: TtlClass, body: &str) -> Duration {
        let ttl = &self.config.ttl;
        match class {
            TtlClass::Search => ttl.search,
            TtlClass::Stable => ttl.stable,
            TtlClass::TvParts => ttl.tv_parts,
            TtlClass::TvDetails => {
                let ongoing = serde_json::from_str::<Details>(body)
                    .map(|d| d.is_ongoing())
                    .unwrap_or(true);
                if ongoing { ttl.ongoing } else { ttl.stable }
            }
        }
    }
}

fn tag_kind(results: &mut [SearchResult], kind: MediaKind) {
    for r in results {
        r.media_type.get_or_insert_with(|| kind.as_str().to_owned());
    }
}

fn http_error(request: &HttpRequest, response: &HttpResponse) -> TmdbError {
    #[derive(serde::Deserialize)]
    struct Status {
        status_message: Option<String>,
    }
    let message = serde_json::from_str::<Status>(&response.body)
        .ok()
        .and_then(|s| s.status_message)
        .unwrap_or_else(|| format!("HTTP {}", response.status));
    match response.status {
        401 => TmdbError::Unauthorized,
        404 => TmdbError::NotFound(request.path.clone()),
        status => TmdbError::Http { status, message },
    }
}

/// Removes the key from error text (a transport may quote the URL).
fn redact(e: TmdbError, secret: &str) -> TmdbError {
    let clean = |s: String| s.replace(secret, "<redacted>");
    match e {
        TmdbError::Transport(m) => TmdbError::Transport(clean(m)),
        TmdbError::Http { status, message } => TmdbError::Http {
            status,
            message: clean(message),
        },
        TmdbError::Decode(m) => TmdbError::Decode(clean(m)),
        other => other,
    }
}
