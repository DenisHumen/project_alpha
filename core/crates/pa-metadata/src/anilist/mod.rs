// SPDX-License-Identifier: MPL-2.0
//! AniList GraphQL client (META-010): titles in romaji, English and native
//! script, synonyms, MAL ids and the relation graph (sequels, prequels,
//! side stories) used to walk anime seasons.
//!
//! Requests are GraphQL **queries** sent as `POST` with a JSON body through
//! the same [`HttpTransport`] as TMDB ([`HttpTransport::post_json`]); no
//! key and no account are needed for public data. The pipeline is the
//! shared one ([`fetch`](crate::fetch)): cache → rate limit → retries →
//! stale fallback.
//!
//! **Rate limit.** AniList documents 90 requests per minute plus a burst
//! limiter, and currently runs in a degraded mode of **30 per minute**; over
//! the limit it answers 429 with a one-minute timeout and may block an IP
//! manually (docs/legal/providers.md, AniList §5). The client starts at
//! [`AniListConfig::per_minute`] (30) and adapts to the `X-RateLimit-Limit`
//! header of every response, up to [`AniListConfig::max_per_minute`] (90)
//! and down to whatever AniList announces. `X-RateLimit-Remaining: 0` pauses
//! the client until `X-RateLimit-Reset`; a 429 pauses it for `Retry-After`
//! (up to 65 s, not capped at TMDB's 30 s).
//!
//! **Terms** (docs/legal/providers.md, AniList): free below $150/month of
//! revenue; no use of the API as a backup or data store and no hoarding or
//! mass collection. Hence: requests only on demand for the user's own files
//! (no crawling, no prefetch of the catalogue), a short-lived cache
//! ([`CacheTtl`], at most [`MAX_CACHE_AGE`]), [`AniListClient::purge_all`]
//! to drop everything, no AniList data in git or bundles (fixtures are
//! synthetic). Attribution is not required; the app name must not contain
//! "AniList" except as "for AniList". 18+ titles are filtered unless
//! [`AniListConfig::include_adult`] is set (store builds keep it off).
//!
//! The client is `Send + Sync`; methods take `&self` and block. It
//! implements the core's [`Provider`](pa_recognition::provider::Provider) —
//! see [`provider`].

pub mod model;
pub mod provider;

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Duration;

use serde::de::DeserializeOwned;
use serde_json::{Map, Value, json};

use crate::cache::{MAX_CACHE_AGE, ProviderCache};
use crate::clock::Clock;
use crate::fetch::{FetchError, Fetcher, Hooks};
use crate::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::rate::{RateLimit, RateLimiter, RetryPolicy};

pub use model::*;

/// Provider id in cache rows and neutral requests.
pub const PROVIDER: &str = "anilist";

/// Production GraphQL endpoint.
pub const DEFAULT_BASE_URL: &str = "https://graphql.anilist.co";

/// Fields of a `Media` the client asks for.
const MEDIA_FIELDS: &str = "id idMal format status episodes duration season seasonYear isAdult \
title { romaji english native } synonyms startDate { year month day } \
endDate { year month day } description(asHtml: false) genres averageScore popularity siteUrl \
coverImage { large } nextAiringEpisode { episode airingAt }";

/// Fields of a related work.
const RELATION_FIELDS: &str = "relations { edges { relationType(version: 2) node { id idMal type \
format status seasonYear isAdult title { romaji english native } startDate { year month day } } } }";

fn search_query() -> String {
    format!(
        "query Search($search: String, $perPage: Int, $isAdult: Boolean) {{ \
Page(page: 1, perPage: $perPage) {{ media(search: $search, type: ANIME, isAdult: $isAdult, \
sort: SEARCH_MATCH) {{ {MEDIA_FIELDS} }} }} }}"
    )
}

fn media_query() -> String {
    format!(
        "query Media($id: Int, $idMal: Int) {{ Media(id: $id, idMal: $idMal, type: ANIME) {{ \
{MEDIA_FIELDS} {RELATION_FIELDS} }} }}"
    )
}

/// How long responses stay fresh. Every value is capped at
/// [`MAX_CACHE_AGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheTtl {
    /// Searches.
    pub search: Duration,
    /// Finished or cancelled titles.
    pub finished: Duration,
    /// Airing, announced or paused titles.
    pub ongoing: Duration,
}

impl Default for CacheTtl {
    /// Research 03 §4.7: metadata 7–30 days, ongoing ~1 day.
    fn default() -> Self {
        const DAY: u64 = 24 * 3600;
        Self {
            search: Duration::from_secs(DAY),
            finished: Duration::from_secs(14 * DAY),
            ongoing: Duration::from_secs(DAY),
        }
    }
}

/// Client settings.
#[derive(Debug, Clone)]
pub struct AniListConfig {
    /// GraphQL endpoint.
    pub base_url: String,
    /// Starting limit, requests per minute (AniList's degraded mode).
    pub per_minute: u32,
    /// Ceiling for the adaptive limit (AniList's normal mode).
    pub max_per_minute: u32,
    /// Requests that may go out back to back.
    pub burst: u32,
    /// Follow `X-RateLimit-Limit` of responses.
    pub adaptive: bool,
    /// Include 18+ titles (off; store builds must keep it off).
    pub include_adult: bool,
    /// Results per search.
    pub search_limit: u32,
    /// Retry policy for 429 and 5xx.
    pub retry: RetryPolicy,
    /// Freshness of cached responses.
    pub ttl: CacheTtl,
}

impl Default for AniListConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            per_minute: 30,
            max_per_minute: 90,
            burst: 2,
            adaptive: true,
            include_adult: false,
            search_limit: 10,
            retry: RetryPolicy {
                max_retries: 3,
                base_delay: Duration::from_secs(2),
                // AniList's 429 timeout is one minute.
                max_delay: Duration::from_secs(65),
            },
            ttl: CacheTtl::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TtlClass {
    Search,
    Media,
}

/// The AniList client. See the [module docs](self).
pub struct AniListClient {
    config: AniListConfig,
    fetcher: Fetcher,
    per_minute: AtomicU32,
}

impl std::fmt::Debug for AniListClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("AniListClient")
            .field("config", &self.config)
            .field("per_minute", &self.rate_per_minute())
            .finish_non_exhaustive()
    }
}

impl AniListClient {
    /// A client with injected transport, cache and clock.
    pub fn new(
        config: AniListConfig,
        transport: Arc<dyn HttpTransport>,
        cache: Arc<dyn ProviderCache>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let per_minute = config.per_minute.max(1);
        let limiter = RateLimiter::new(RateLimit::per_minute(per_minute, config.burst));
        Self {
            fetcher: Fetcher {
                provider: PROVIDER,
                transport,
                cache,
                clock,
                limiter,
                retry: config.retry,
            },
            config,
            per_minute: AtomicU32::new(per_minute),
        }
    }

    /// A live client: HTTPS via `ureq`, the system clock and the given cache.
    #[cfg(feature = "http")]
    pub fn live(config: AniListConfig, cache: Arc<dyn ProviderCache>) -> Self {
        Self::new(
            config,
            Arc::new(crate::http::UreqTransport::new(crate::USER_AGENT)),
            cache,
            Arc::new(crate::clock::SystemClock::default()),
        )
    }

    /// Settings in use.
    pub fn config(&self) -> &AniListConfig {
        &self.config
    }

    /// The rate limit in force, requests per minute (changes with
    /// `X-RateLimit-Limit`).
    pub fn rate_per_minute(&self) -> u32 {
        self.per_minute.load(Ordering::Relaxed)
    }

    /// Deletes cached responses older than six months (call at startup).
    pub fn purge_expired(&self) -> usize {
        self.fetcher.purge_expired()
    }

    /// Deletes all cached AniList data.
    pub fn purge_all(&self) -> usize {
        self.fetcher.purge_all()
    }

    // --- Queries ---------------------------------------------------------

    /// Anime matching `query` (titles and synonyms, best match first).
    /// 18+ titles are dropped unless allowed.
    pub fn search(&self, query: &str) -> Result<Vec<Media>, FetchError> {
        let mut vars = Map::new();
        vars.insert("search".into(), json!(query.trim()));
        vars.insert("perPage".into(), json!(self.config.search_limit.clamp(1, 50)));
        if !self.config.include_adult {
            vars.insert("isAdult".into(), json!(false));
        }
        let data: PageData = self.query(&search_query(), vars, TtlClass::Search)?;
        let mut media = data.page.map(|p| p.media).unwrap_or_default();
        if !self.config.include_adult {
            media.retain(|m| !m.adult());
        }
        Ok(media)
    }

    /// One anime by AniList id, with its relations.
    pub fn media(&self, id: u64) -> Result<Media, FetchError> {
        let mut vars = Map::new();
        vars.insert("id".into(), json!(id));
        self.media_with(vars, &format!("anime {id}"))
    }

    /// One anime by MyAnimeList id, with its relations.
    pub fn media_by_mal(&self, mal_id: u64) -> Result<Media, FetchError> {
        let mut vars = Map::new();
        vars.insert("idMal".into(), json!(mal_id));
        self.media_with(vars, &format!("anime with MAL id {mal_id}"))
    }

    fn media_with(&self, vars: Map<String, Value>, what: &str) -> Result<Media, FetchError> {
        let data: MediaData = self.query(&media_query(), vars, TtlClass::Media)?;
        let not_found = || FetchError::NotFound {
            provider: PROVIDER,
            what: what.to_owned(),
        };
        let mut media = data.media.ok_or_else(not_found)?;
        if media.adult() && !self.config.include_adult {
            return Err(not_found());
        }
        if !self.config.include_adult
            && let Some(rel) = &mut media.relations
        {
            rel.edges.retain(|e| !e.node.is_adult.unwrap_or(false));
        }
        Ok(media)
    }

    // --- Plumbing --------------------------------------------------------

    fn query<T: DeserializeOwned>(
        &self,
        query: &str,
        variables: Map<String, Value>,
        class: TtlClass,
    ) -> Result<T, FetchError> {
        let body = self.query_body(query, variables, class)?;
        let response: GraphqlResponse<T> = serde_json::from_str(&body)
            .map_err(|e| FetchError::Decode(format!("AniList: {e}")))?;
        response
            .data
            .ok_or_else(|| FetchError::Decode("AniList: no data".into()))
    }

    fn query_body(
        &self,
        query: &str,
        variables: Map<String, Value>,
        class: TtlClass,
    ) -> Result<String, FetchError> {
        let request = HttpRequest {
            base: self.config.base_url.trim_end_matches('/').to_owned(),
            path: "/".to_owned(),
            query: Vec::new(),
            headers: vec![("accept".into(), "application/json".into())],
        };
        let body = json!({ "query": query, "variables": variables }).to_string();
        self.fetcher.fetch(
            &request,
            Some(&body),
            &AniListHooks {
                client: self,
                class,
            },
        )
    }

    /// Follows `X-RateLimit-*` headers.
    fn observe_quota(&self, limiter: &RateLimiter, clock: &dyn Clock, response: &HttpResponse) {
        let header = |name: &str| {
            response
                .header(name)
                .and_then(|v| v.trim().parse::<u64>().ok())
        };
        if self.config.adaptive
            && let Some(limit) = header("x-ratelimit-limit")
        {
            let target = u32::try_from(limit)
                .unwrap_or(u32::MAX)
                .clamp(1, self.config.max_per_minute.max(1));
            if self.per_minute.swap(target, Ordering::Relaxed) != target {
                limiter.set_limit(RateLimit::per_minute(target, self.config.burst));
            }
        }
        // Quota used up (other apps on the same IP): wait for the reset.
        // A 429 is handled by the retry loop with `Retry-After`.
        if response.status != 429
            && header("x-ratelimit-remaining") == Some(0)
            && let Some(reset) = header("x-ratelimit-reset")
        {
            let wait = reset.saturating_sub(clock.unix_secs()).min(65);
            if wait > 0 {
                limiter.pause_for(clock, Duration::from_secs(wait));
            }
        }
    }
}

struct AniListHooks<'a> {
    client: &'a AniListClient,
    class: TtlClass,
}

impl Hooks for AniListHooks<'_> {
    fn observe(&self, limiter: &RateLimiter, clock: &dyn Clock, response: &HttpResponse) {
        self.client.observe_quota(limiter, clock, response);
    }

    fn error_message(&self, response: &HttpResponse) -> Option<String> {
        let parsed: GraphqlResponse<Value> = serde_json::from_str(&response.body).ok()?;
        let first = parsed.errors.into_iter().next()?;
        Some(first.message).filter(|m| !m.is_empty())
    }

    fn check_ok(&self, body: &str) -> Result<(), FetchError> {
        let parsed: GraphqlResponse<Value> = serde_json::from_str(body)
            .map_err(|e| FetchError::Decode(format!("AniList: {e}")))?;
        match parsed.errors.into_iter().next() {
            None => Ok(()),
            Some(e) if e.status == Some(404) => Err(FetchError::NotFound {
                provider: PROVIDER,
                what: e.message,
            }),
            Some(e) => Err(FetchError::Http {
                provider: PROVIDER,
                status: e.status.unwrap_or(200),
                message: e.message,
            }),
        }
    }

    fn ttl(&self, body: &str) -> Duration {
        let ttl = &self.client.config.ttl;
        let ttl = match self.class {
            TtlClass::Search => ttl.search,
            TtlClass::Media => {
                let ongoing = serde_json::from_str::<GraphqlResponse<MediaData>>(body)
                    .ok()
                    .and_then(|r| r.data)
                    .and_then(|d| d.media)
                    .is_none_or(|m| m.is_ongoing());
                if ongoing { ttl.ongoing } else { ttl.finished }
            }
        };
        ttl.min(MAX_CACHE_AGE)
    }
}
