// SPDX-License-Identifier: MPL-2.0
//! Request pipeline shared by the keyless providers (AniList, Shikimori):
//! cache → rate limit → retries → stale fallback, the same order and rules
//! as the TMDB client (see [`tmdb`](crate::tmdb) module docs), minus keys.
//!
//! 1. A **fresh** cache entry is returned without network.
//! 2. Every attempt takes a token from the provider's [`RateLimiter`].
//! 3. HTTP 429 and 500/502/503/504 are retried per [`RetryPolicy`]; a 429
//!    pauses the whole client. Other 4xx are not retried.
//! 4. On a network error, an exhausted retry budget or a 5xx, a **stale**
//!    entry younger than [`MAX_CACHE_AGE`] is served instead.
//!
//! Provider quirks plug in through an internal hooks trait: reading quota headers,
//! extracting error messages, rejecting 2xx bodies that carry an error.

use std::sync::Arc;
use std::time::Duration;

use pa_recognition::provider::ProviderError;

use crate::cache::{CacheEntry, MAX_CACHE_AGE, ProviderCache};
use crate::clock::Clock;
use crate::http::{HttpRequest, HttpResponse, HttpTransport};
use crate::rate::{RateLimiter, RetryPolicy, parse_retry_after};

/// Failures of the keyless provider clients (AniList, Shikimori).
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum FetchError {
    /// The entity does not exist, or is hidden by the adult-content filter.
    #[error("not found at {provider}: {what}")]
    NotFound {
        /// Provider id.
        provider: &'static str,
        /// What was asked for.
        what: String,
    },
    /// Still HTTP 429 after all retries.
    #[error("{provider} rate limit: HTTP 429 after {attempts} attempts")]
    RateLimited {
        /// Provider id.
        provider: &'static str,
        /// Requests made.
        attempts: u32,
        /// Last `Retry-After`.
        retry_after: Option<Duration>,
    },
    /// Other HTTP error.
    #[error("{provider} HTTP {status}: {message}")]
    Http {
        /// Provider id.
        provider: &'static str,
        /// Status code.
        status: u16,
        /// Provider's message or a short description.
        message: String,
    },
    /// Network failure (no response at all).
    #[error("network error: {0}")]
    Transport(String),
    /// The response did not have the expected shape.
    #[error("unexpected response: {0}")]
    Decode(String),
}

impl From<FetchError> for ProviderError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::Transport(m) => Self::Unavailable(m),
            FetchError::NotFound { what, .. } => Self::NotFound(what),
            FetchError::RateLimited { retry_after, .. } => Self::RateLimited {
                retry_after_secs: retry_after.map(|d| d.as_secs()),
            },
            e @ (FetchError::Http { .. } | FetchError::Decode(_)) => Self::Failed(e.to_string()),
        }
    }
}

/// Provider-specific behaviour around a request.
pub(crate) trait Hooks {
    /// Sees every response, before status handling (quota headers).
    fn observe(&self, _limiter: &RateLimiter, _clock: &dyn Clock, _response: &HttpResponse) {}

    /// Human-readable message of an error response.
    fn error_message(&self, response: &HttpResponse) -> Option<String>;

    /// Rejects a 2xx body that is an error in disguise; such bodies are
    /// not cached.
    fn check_ok(&self, _body: &str) -> Result<(), FetchError> {
        Ok(())
    }

    /// Freshness of a successful response body.
    fn ttl(&self, body: &str) -> Duration;
}

/// The pipeline. One per client; `Send + Sync`.
pub(crate) struct Fetcher {
    pub provider: &'static str,
    pub transport: Arc<dyn HttpTransport>,
    pub cache: Arc<dyn ProviderCache>,
    pub clock: Arc<dyn Clock>,
    pub limiter: RateLimiter,
    pub retry: RetryPolicy,
}

impl Fetcher {
    /// `GET` (`body = None`) or JSON `POST` through cache, limiter and
    /// retries. Returns the response body.
    pub fn fetch(
        &self,
        request: &HttpRequest,
        body: Option<&str>,
        hooks: &dyn Hooks,
    ) -> Result<String, FetchError> {
        let canonical = match body {
            Some(b) => request.canonical_post(b),
            None => request.canonical(),
        };
        let cache_key = format!("{}:{canonical}", self.provider);
        let now = self.clock.unix_secs();
        let cached = self.cache.get(&cache_key).ok().flatten();
        if let Some(entry) = &cached
            && entry.is_fresh(now)
        {
            return Ok(entry.body.clone());
        }
        let stale = cached.filter(|e| e.is_usable(now)).map(|e| e.body);

        let result = self
            .send_with_retries(request, body, hooks)
            .and_then(|response| {
                hooks.check_ok(&response.body)?;
                Ok(response)
            });
        match result {
            Ok(response) => {
                let ttl = hooks.ttl(&response.body).min(MAX_CACHE_AGE);
                let fetched_at = self.clock.unix_secs();
                let entry = CacheEntry {
                    body: response.body,
                    fetched_at,
                    expires_at: fetched_at.saturating_add(ttl.as_secs()),
                };
                // A cache write failure must not fail the request.
                let _ = self.cache.put(self.provider, &cache_key, &entry);
                Ok(entry.body)
            }
            Err(e @ (FetchError::Transport(_) | FetchError::RateLimited { .. })) => stale.ok_or(e),
            Err(e @ FetchError::Http { status, .. }) if status >= 500 => stale.ok_or(e),
            Err(e) => Err(e),
        }
    }

    fn send_with_retries(
        &self,
        request: &HttpRequest,
        body: Option<&str>,
        hooks: &dyn Hooks,
    ) -> Result<HttpResponse, FetchError> {
        let policy = self.retry;
        let mut retry = 0;
        loop {
            self.limiter.acquire(self.clock.as_ref());
            let response = match body {
                Some(b) => self.transport.post_json(request, b),
                None => self.transport.get(request),
            }
            .map_err(|e| FetchError::Transport(e.0))?;
            hooks.observe(&self.limiter, self.clock.as_ref(), &response);
            let status = response.status;
            if (200..300).contains(&status) {
                return Ok(response);
            }
            let retryable = status == 429 || matches!(status, 500 | 502 | 503 | 504);
            if !retryable {
                return Err(self.http_error(request, &response, hooks));
            }
            let retry_after = parse_retry_after(response.header("retry-after"));
            if retry >= policy.max_retries {
                return Err(if status == 429 {
                    FetchError::RateLimited {
                        provider: self.provider,
                        attempts: retry + 1,
                        retry_after,
                    }
                } else {
                    self.http_error(request, &response, hooks)
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

    fn http_error(
        &self,
        request: &HttpRequest,
        response: &HttpResponse,
        hooks: &dyn Hooks,
    ) -> FetchError {
        let message = hooks
            .error_message(response)
            .unwrap_or_else(|| format!("HTTP {}", response.status));
        match response.status {
            404 => FetchError::NotFound {
                provider: self.provider,
                what: if request.path.len() > 1 {
                    request.path.clone()
                } else {
                    message
                },
            },
            status => FetchError::Http {
                provider: self.provider,
                status,
                message,
            },
        }
    }

    /// Deletes this provider's responses older than [`MAX_CACHE_AGE`].
    pub fn purge_expired(&self) -> usize {
        let cutoff = self
            .clock
            .unix_secs()
            .saturating_sub(MAX_CACHE_AGE.as_secs());
        self.cache.purge_older_than(cutoff).unwrap_or(0)
    }

    /// Deletes all cached responses of this provider.
    pub fn purge_all(&self) -> usize {
        self.cache.purge_provider(self.provider).unwrap_or(0)
    }
}

/// Neutral-request parameter access shared by the provider adapters.
pub(crate) struct Params<'a>(pub &'a pa_recognition::provider::ProviderRequest);

impl Params<'_> {
    pub fn opt(&self, key: &str) -> Option<&str> {
        self.0.params.get(key).map(String::as_str)
    }

    pub fn get(&self, key: &str) -> Result<&str, ProviderError> {
        self.opt(key)
            .ok_or_else(|| ProviderError::BadRequest(format!("missing param {key:?}")))
    }

    pub fn opt_number<T: std::str::FromStr>(&self, key: &str) -> Result<Option<T>, ProviderError> {
        self.opt(key)
            .map(|v| {
                v.parse().map_err(|_| {
                    ProviderError::BadRequest(format!("param {key:?} is not a number"))
                })
            })
            .transpose()
    }

    pub fn number<T: std::str::FromStr>(&self, key: &str) -> Result<T, ProviderError> {
        self.opt_number(key)?
            .ok_or_else(|| ProviderError::BadRequest(format!("missing param {key:?}")))
    }
}

/// Year of an ISO date or a bare year (`2021-04-02`, `2021`).
pub(crate) fn year_of(date: &str) -> Option<u32> {
    date.get(..4)?.parse().ok().filter(|y| *y > 1800)
}
