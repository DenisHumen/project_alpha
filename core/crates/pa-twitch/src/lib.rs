// SPDX-License-Identifier: MPL-2.0
//! Client for the documented Twitch Helix API (TWI-001).
//!
//! What it covers (research 06 §1.1): users (avatars and nicknames),
//! followed channels and followed live streams, streams, channel info,
//! videos (VODs, highlights, uploads), clips, categories (`games`,
//! `games/top`, `search/categories`), channel search, schedule, chat emotes,
//! chat badges, chat colours and chat settings. On top of the raw endpoints
//! [`HelixClient::followed_overview`] builds the "my streamers" list: every
//! followed channel with its avatar, display name and live stream, if any.
//!
//! # Design
//!
//! - **Injectable everything.** The project has no registered Twitch
//!   application, so the client id is configuration ([`ClientId`]), never a
//!   constant. HTTP goes through the [`Transport`] trait (a reqwest + rustls
//!   implementation is behind the default `reqwest` feature) and time through
//!   the [`Clock`] trait, so tests run on recorded fixtures without network
//!   and without real sleeps (`testing` feature).
//! - **Rate limits** (06 §1.4): a token bucket mirrors the `Ratelimit-Limit`,
//!   `Ratelimit-Remaining` and `Ratelimit-Reset` headers. When the bucket is
//!   empty the client waits for the reset instead of sending; a `429` is
//!   retried after the reset up to [`HelixConfig::max_retries`] times, then
//!   surfaces as [`Error::RateLimited`].
//! - **Pagination** is cursor based and Twitch lists "float", so
//!   [`collect_pages`] drops duplicates by id and stops on a repeated cursor.
//! - **Auth** is a bearer token from Device Code Flow (TWI-002, public client,
//!   no secret): [`TwitchAuth`] signs in, refreshes, validates, revokes and
//!   keeps the tokens of this device in `pa-secrets`; [`AuthorizedHelix`]
//!   refreshes and retries once when Helix answers `401`
//!   ([`Error::Unauthorized`]). Tokens are redacted in every `Debug`.
//! - **Chat writes** (TWI-010): [`HelixClient::send_chat_message`] and
//!   [`HelixClient::create_eventsub_subscription`] (WebSocket transport) for
//!   the live-chat sources in `pa-twitch-chat`.
//! - **Playback** (TWI-004): [`playback::TwitchPlayback`] decides between
//!   the official embed and native HLS from the signed remote config
//!   (`pa-remote-config`); the GQL/usher path exists only with the `full`
//!   feature.
//!
//! # Policy (ADR-0011 §5, docs/legal/providers.md §2.4)
//!
//! - Only documented Helix endpoints live here; that makes the crate usable
//!   in both store and full builds. Playback through GQL/usher
//!   ([`playback`]) is compiled only with the `full` feature; store builds
//!   play through the official embed (TWI-005).
//! - Twitch data may be cached for at most 24 hours: every cache in this
//!   crate clamps its TTL to [`MAX_CACHE_TTL`].
//! - The product is never named "… for Twitch"; the default User-Agent is
//!   the product name only.

mod auth;
mod cache;
mod chat_api;
mod client;
mod error;
mod model;
mod pagination;
pub mod playback;
mod ratelimit;
#[cfg(feature = "reqwest")]
mod reqwest_transport;
#[cfg(feature = "testing")]
pub mod testing;
mod transport;
mod util;

pub use auth::{
    Account, AuthConfig, AuthError, AuthResult, AuthorizedHelix, DEFAULT_ID_BASE_URL,
    DEFAULT_SCOPES, DeviceAuthorization, REFRESH_MARGIN, SCOPE_READ_CHAT, SCOPE_READ_FOLLOWS,
    SCOPE_WRITE_CHAT, TwitchAuth, VALIDATE_INTERVAL, Validation,
};
pub use cache::{MAX_CACHE_TTL, TtlCache};
pub use chat_api::{
    ChatDropReason, EventSubSubscription, EventSubSubscriptionRequest, MAX_CHAT_MESSAGE_CHARS,
    SendChatMessage, SentChatMessage,
};
pub use client::{
    AccessToken, ClientId, ClipsQuery, DEFAULT_BASE_URL, FollowedStreamer, HelixClient,
    HelixConfig, SchedulePage, SearchChannelsQuery, StreamsQuery, VideosQuery,
};
pub use error::{Error, Result};
pub use model::*;
pub use pagination::{Cursor, HasId, MAX_PAGE_SIZE, Page, collect_pages};
pub use ratelimit::RateLimitState;
#[cfg(feature = "reqwest")]
pub use reqwest_transport::ReqwestTransport;
pub use transport::{
    BoxFuture, Clock, HttpRequest, HttpResponse, Method, SystemClock, Transport, TransportError,
};
pub use util::{fill_image_template, parse_video_duration};
