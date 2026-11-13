// SPDX-License-Identifier: MPL-2.0
//! Shikimori REST client (META-010): Russian titles and synopses of anime
//! by MyAnimeList id, search, and the relation list.
//!
//! Shikimori anime ids are MyAnimeList ids, so a MAL id from the mapping
//! layer (`pa-anime-mappings`, Fribb list) leads straight to
//! [`ShikimoriClient::russian`] without a search. Public data needs no
//! token; requests are plain `GET`s through the shared pipeline
//! ([`fetch`](crate::fetch)): cache → rate limit → retries → stale fallback.
//!
//! **Rules** (docs/legal/providers.md, Shikimori):
//! - limits **5 requests per second and 90 per minute** — the default
//!   bucket ([`RateLimit::per_minute`]`(90, 3)`) admits at most 90 in any
//!   minute and 4 in any second;
//! - the `User-Agent` must carry the name of the project's registered OAuth
//!   application ([`ShikimoriConfig::user_agent`], sent with every request)
//!   and must not imitate a browser — otherwise the IP is banned;
//! - API only, never scraping the site.
//!
//! Terms say nothing about caching; the cache follows the same short TTLs as
//! the other providers ([`CacheTtl`], at most [`MAX_CACHE_AGE`]), and
//! [`ShikimoriClient::purge_all`] drops everything. Hentai (`rating = rx`) is
//! hidden unless [`ShikimoriConfig::include_adult`] is set; searches ask for
//! `censored=true`.
//!
//! The client is `Send + Sync`; methods take `&self` and block. It
//! implements the core's [`Provider`](pa_recognition::provider::Provider) —
//! see [`provider`].

pub mod model;
pub mod provider;

use std::sync::Arc;
use std::time::Duration;

use serde::de::DeserializeOwned;

use crate::cache::{MAX_CACHE_AGE, ProviderCache};
use crate::clock::Clock;
use crate::fetch::{FetchError, Fetcher, Hooks};
use crate::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::rate::{RateLimit, RateLimiter, RetryPolicy};

pub use model::*;

/// Provider id in cache rows and neutral requests.
pub const PROVIDER: &str = "shikimori";

/// Production API base (the site moved from `shikimori.one` to
/// `shikimori.io`; the old domain redirects).
pub const DEFAULT_BASE_URL: &str = "https://shikimori.io/api";

/// How long responses stay fresh. Every value is capped at
/// [`MAX_CACHE_AGE`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CacheTtl {
    /// Searches.
    pub search: Duration,
    /// Released titles and relation lists.
    pub released: Duration,
    /// Airing or announced titles.
    pub ongoing: Duration,
}

impl Default for CacheTtl {
    /// Research 03 §4.7: metadata 7–30 days, ongoing ~1 day.
    fn default() -> Self {
        const DAY: u64 = 24 * 3600;
        Self {
            search: Duration::from_secs(DAY),
            released: Duration::from_secs(14 * DAY),
            ongoing: Duration::from_secs(DAY),
        }
    }
}

/// Client settings.
#[derive(Debug, Clone)]
pub struct ShikimoriConfig {
    /// API base URL.
    pub base_url: String,
    /// `User-Agent` of every request; must contain the registered OAuth
    /// application name (`Kinolune`).
    pub user_agent: String,
    /// Client-side rate limit.
    pub rate_limit: RateLimit,
    /// Include hentai (off; store builds must keep it off).
    pub include_adult: bool,
    /// Results per search (Shikimori allows up to 50).
    pub search_limit: u32,
    /// Retry policy for 429 and 5xx.
    pub retry: RetryPolicy,
    /// Freshness of cached responses.
    pub ttl: CacheTtl,
}

impl Default for ShikimoriConfig {
    fn default() -> Self {
        Self {
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: crate::USER_AGENT.to_owned(),
            rate_limit: RateLimit::per_minute(90, 3),
            include_adult: false,
            search_limit: 10,
            retry: RetryPolicy::default(),
            ttl: CacheTtl::default(),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum TtlClass {
    Search,
    Anime,
    Related,
}

/// Russian localisation of one anime ([`ShikimoriClient::russian`]).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RussianInfo {
    /// Shikimori id (= MAL id).
    pub id: u64,
    /// Russian title.
    pub title: Option<String>,
    /// Title of the official Russian licence, if any.
    pub license_name: Option<String>,
    /// Russian synopsis, markup removed.
    pub description: Option<String>,
    /// Romanised title.
    pub original_title: String,
    /// Year of the first air date.
    pub year: Option<u32>,
}

/// The Shikimori client. See the [module docs](self).
pub struct ShikimoriClient {
    config: ShikimoriConfig,
    fetcher: Fetcher,
}

impl std::fmt::Debug for ShikimoriClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ShikimoriClient")
            .field("config", &self.config)
            .finish_non_exhaustive()
    }
}

impl ShikimoriClient {
    /// A client with injected transport, cache and clock.
    pub fn new(
        config: ShikimoriConfig,
        transport: Arc<dyn HttpTransport>,
        cache: Arc<dyn ProviderCache>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        Self {
            fetcher: Fetcher {
                provider: PROVIDER,
                transport,
                cache,
                clock,
                limiter: RateLimiter::new(config.rate_limit),
                retry: config.retry,
            },
            config,
        }
    }

    /// A live client: HTTPS via `ureq`, the system clock and the given cache.
    #[cfg(feature = "http")]
    pub fn live(config: ShikimoriConfig, cache: Arc<dyn ProviderCache>) -> Self {
        let transport = crate::http::UreqTransport::new(&config.user_agent);
        Self::new(
            config,
            Arc::new(transport),
            cache,
            Arc::new(crate::clock::SystemClock::default()),
        )
    }

    /// Settings in use.
    pub fn config(&self) -> &ShikimoriConfig {
        &self.config
    }

    /// Deletes cached responses older than six months (call at startup).
    pub fn purge_expired(&self) -> usize {
        self.fetcher.purge_expired()
    }

    /// Deletes all cached Shikimori data.
    pub fn purge_all(&self) -> usize {
        self.fetcher.purge_all()
    }

    // --- Endpoints -------------------------------------------------------

    /// `/animes?search=`: anime by any title, Russian included.
    pub fn search(&self, query: &str) -> Result<Vec<AnimeShort>, FetchError> {
        let mut q = vec![
            ("search".to_owned(), query.trim().to_owned()),
            (
                "limit".to_owned(),
                self.config.search_limit.clamp(1, 50).to_string(),
            ),
        ];
        if !self.config.include_adult {
            q.push(("censored".to_owned(), "true".to_owned()));
        }
        self.get_json("/animes", q, TtlClass::Search)
    }

    /// `/animes/{id}`; `id` is the Shikimori id, which is the MAL id.
    pub fn anime(&self, id: u64) -> Result<Anime, FetchError> {
        let anime: Anime = self.get_json(&format!("/animes/{id}"), Vec::new(), TtlClass::Anime)?;
        if anime.adult() && !self.config.include_adult {
            return Err(FetchError::NotFound {
                provider: PROVIDER,
                what: format!("anime {id}"),
            });
        }
        Ok(anime)
    }

    /// `/animes/{id}/related` (anime entries only; manga is dropped).
    pub fn related(&self, id: u64) -> Result<Vec<Related>, FetchError> {
        let mut related: Vec<Related> = self.get_json(
            &format!("/animes/{id}/related"),
            Vec::new(),
            TtlClass::Related,
        )?;
        related.retain(|r| r.anime.is_some());
        Ok(related)
    }

    /// Russian title and synopsis of the anime with MyAnimeList id `mal_id`.
    pub fn russian(&self, mal_id: u64) -> Result<RussianInfo, FetchError> {
        let anime = self.anime(mal_id)?;
        if anime.myanimelist_id.is_some_and(|m| m != mal_id) {
            return Err(FetchError::NotFound {
                provider: PROVIDER,
                what: format!("anime with MAL id {mal_id}"),
            });
        }
        Ok(RussianInfo {
            id: anime.base.id,
            title: anime.base.russian_title().map(str::to_owned),
            license_name: anime
                .license_name_ru
                .as_deref()
                .map(str::trim)
                .filter(|s| !s.is_empty())
                .map(str::to_owned),
            description: anime.plain_description(),
            year: anime.base.year(),
            original_title: anime.base.name,
        })
    }

    // --- Plumbing --------------------------------------------------------

    fn get_json<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Vec<(String, String)>,
        class: TtlClass,
    ) -> Result<T, FetchError> {
        let request = HttpRequest {
            base: self.config.base_url.trim_end_matches('/').to_owned(),
            path: path.to_owned(),
            query,
            headers: vec![
                ("accept".into(), "application/json".into()),
                ("user-agent".into(), self.config.user_agent.clone()),
            ],
        };
        let body = self.fetcher.fetch(
            &request,
            None,
            &ShikimoriHooks {
                ttl: self.config.ttl,
                class,
            },
        )?;
        serde_json::from_str(&body)
            .map_err(|e| FetchError::Decode(format!("Shikimori {path}: {e}")))
    }
}

struct ShikimoriHooks {
    ttl: CacheTtl,
    class: TtlClass,
}

impl Hooks for ShikimoriHooks {
    fn error_message(&self, response: &HttpResponse) -> Option<String> {
        let v: serde_json::Value = serde_json::from_str(&response.body).ok()?;
        ["message", "error"]
            .iter()
            .find_map(|k| v.get(k)?.as_str().map(str::to_owned))
    }

    fn ttl(&self, body: &str) -> Duration {
        let ttl = match self.class {
            TtlClass::Search => self.ttl.search,
            TtlClass::Related => self.ttl.released,
            TtlClass::Anime => {
                let ongoing = serde_json::from_str::<AnimeShort>(body)
                    .ok()
                    .is_none_or(|a| a.is_ongoing());
                if ongoing {
                    self.ttl.ongoing
                } else {
                    self.ttl.released
                }
            }
        };
        ttl.min(MAX_CACHE_AGE)
    }
}
