// SPDX-License-Identifier: MPL-2.0
//! Metadata provider clients of Kinolune — the network layer *outside* the
//! sans-IO recognition core (research 03 §7.2, META-009).
//!
//! The core (`pa-recognition`) asks for provider data through
//! [`pa_recognition::provider::Provider`]; this crate implements it for live
//! providers. Building blocks shared by all providers:
//!
//! - [`http`]: `GET`-only [`HttpTransport`](http::HttpTransport), live
//!   implementation on `ureq` + rustls (feature `http`, on by default);
//! - [`replay`]: HTTP cassettes for tests and offline tools — tests never
//!   touch the network;
//! - [`rate`]: token-bucket rate limiter and 429/5xx retry backoff;
//! - [`cache`]: `ProviderCache` with TTL, stale fallback and the six-month
//!   hard limit from TMDB's terms; in-memory and SQLite implementations;
//! - [`key`]: project key from the build plus the user's own key;
//! - [`clock`]: injectable time;
//! - [`attribution`]: what the Credits screen (UI-016) must show.
//!
//! Providers: [`tmdb`] (search, details, translations, alternative titles,
//! external ids, images, collections, seasons, episode groups, `/find`).
//!
//! ```
//! use std::sync::Arc;
//! use pa_metadata::{cache::MemoryCache, clock::ManualClock, key::Keys};
//! use pa_metadata::http::HttpResponse;
//! use pa_metadata::replay::ReplayTransport;
//! use pa_metadata::tmdb::{MediaKind, TmdbClient, TmdbConfig};
//!
//! let transport = ReplayTransport::default();
//! transport.push(
//!     "/movie/1?language=ru-RU",
//!     HttpResponse { status: 200, headers: vec![], body: r#"{"id":1,"title":"Пример"}"#.into() },
//! );
//! let client = TmdbClient::new(
//!     TmdbConfig::default(),
//!     Keys::none().with_user_key(Some("test-key")),
//!     Arc::new(transport),
//!     Arc::new(MemoryCache::default()),
//!     Arc::new(ManualClock::at_unix(1_700_000_000)),
//! );
//! let movie = client.details(MediaKind::Movie, 1).unwrap();
//! assert_eq!(movie.title.as_deref(), Some("Пример"));
//! ```

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod attribution;
pub mod cache;
pub mod clock;
pub mod http;
pub mod key;
pub mod rate;
pub mod replay;
pub mod tmdb;
