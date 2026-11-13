// SPDX-License-Identifier: MPL-2.0
//! The boundary between the sans-IO recognition core and metadata providers.
//!
//! The core never does network I/O (research 03 §7.2). Pipeline stages that
//! need provider data (candidate generation, numbering) ask for it through
//! the [`Provider`] trait with a provider-neutral [`ProviderRequest`] and get
//! JSON back. Implementations live outside the core: the live clients in
//! `pa-metadata` (TMDB in META-009, AniList/Shikimori/AniDB/... later), and
//! the record/replay tapes of the eval runner `pa-eval`. Because requests are
//! plain data with a canonical form, any live provider can be recorded into a
//! tape and replayed offline, byte for byte.
//!
//! Request vocabulary (`provider` / `endpoint` / `params`) is defined by each
//! implementation; the shared ones are documented on the implementing type
//! (for TMDB: `pa_metadata::tmdb::TmdbClient`).

use std::collections::BTreeMap;
use std::fmt;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// A provider call in a provider-neutral shape.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct ProviderRequest {
    /// Provider name: `tmdb`, `anidb`, `tvdb`, `anilist`, ...
    pub provider: String,
    /// Operation: `search`, `details`, `episodes`, ...
    pub endpoint: String,
    /// Parameters; a `BTreeMap` keeps the canonical form stable.
    #[serde(default)]
    pub params: BTreeMap<String, String>,
}

impl ProviderRequest {
    /// A request without parameters.
    pub fn new(provider: &str, endpoint: &str) -> Self {
        Self {
            provider: provider.to_owned(),
            endpoint: endpoint.to_owned(),
            params: BTreeMap::new(),
        }
    }

    /// Adds (or replaces) one parameter.
    pub fn param(mut self, key: &str, value: impl Into<String>) -> Self {
        self.params.insert(key.to_owned(), value.into());
        self
    }

    /// Canonical JSON of the request: the key of record/replay tapes.
    pub fn key(&self) -> String {
        serde_json::to_string(self).expect("request serializes")
    }
}

/// Why a provider call produced no data.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum ProviderError {
    /// Replay or offline mode: there is no recorded response for the request.
    TapeMiss(String),
    /// The provider is not usable right now: no API key configured, the
    /// provider is disabled by the user, or there is no network. The core
    /// treats this as "no candidates" and keeps the local recognition result.
    Unavailable(String),
    /// The provider kept answering "too many requests" after the client's own
    /// backoff; `retry_after_secs` is the provider's hint, if it sent one.
    RateLimited {
        /// Seconds the provider asked to wait, if known.
        retry_after_secs: Option<u64>,
    },
    /// The requested entity does not exist at the provider.
    NotFound(String),
    /// The request is not understood by this provider (unknown endpoint or
    /// missing/invalid parameter) — a programming error in the caller.
    BadRequest(String),
    /// Any other failure reported by a live provider.
    Failed(String),
}

impl fmt::Display for ProviderError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::TapeMiss(key) => write!(f, "request not on tape: {key}"),
            Self::Unavailable(why) => write!(f, "provider unavailable: {why}"),
            Self::RateLimited {
                retry_after_secs: Some(s),
            } => write!(f, "provider rate limit hit, retry after {s} s"),
            Self::RateLimited { .. } => write!(f, "provider rate limit hit"),
            Self::NotFound(what) => write!(f, "not found at provider: {what}"),
            Self::BadRequest(msg) => write!(f, "bad provider request: {msg}"),
            Self::Failed(msg) => write!(f, "provider failed: {msg}"),
        }
    }
}

impl std::error::Error for ProviderError {}

/// Metadata provider as seen by the recognition pipeline.
///
/// Implementations must be deterministic for a given request and provider
/// state (cache, tape): recognition results are reproduced offline from
/// recorded responses.
pub trait Provider {
    /// Answers one request.
    fn call(&mut self, request: &ProviderRequest) -> Result<Value, ProviderError>;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn key_is_canonical() {
        let a = ProviderRequest::new("tmdb", "search")
            .param("year", "2016")
            .param("query", "Lane");
        let b = ProviderRequest::new("tmdb", "search")
            .param("query", "Lane")
            .param("year", "2016");
        assert_eq!(a.key(), b.key());
        assert_eq!(
            a.key(),
            r#"{"provider":"tmdb","endpoint":"search","params":{"query":"Lane","year":"2016"}}"#
        );
    }

    #[test]
    fn errors_display() {
        let e = ProviderError::RateLimited {
            retry_after_secs: Some(3),
        };
        assert_eq!(e.to_string(), "provider rate limit hit, retry after 3 s");
    }
}
