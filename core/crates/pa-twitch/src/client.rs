// SPDX-License-Identifier: MPL-2.0
//! The Helix client: configuration, request loop, endpoints.

use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex, RwLock};
use std::time::Duration;

use serde::Deserialize;
use serde::de::DeserializeOwned;

use crate::cache::{MAX_CACHE_TTL, TtlCache};
use crate::error::{Error, Result};
use crate::model::{
    BadgeSet, Category, ChannelInfo, ChannelSearchResult, ChatSettings, Clip, EmoteList,
    FollowedChannel, Schedule, Stream, User, UserChatColor, Video, VideoType,
};
use crate::pagination::{Cursor, MAX_PAGE_SIZE, Page, collect_pages};
use crate::ratelimit::{RateLimitState, retry_after_429};
use crate::transport::{Clock, HttpRequest, HttpResponse, Method, SystemClock, Transport};
use crate::util::encode_query_component;

/// Production Helix base URL.
pub const DEFAULT_BASE_URL: &str = "https://api.twitch.tv/helix";

/// Most ids/logins Helix accepts in one lookup (`users`, `channels`, …).
const MAX_IDS_PER_CALL: usize = 100;
/// `chat/emotes/set` accepts at most 25 set ids.
const MAX_EMOTE_SETS_PER_CALL: usize = 25;

/// Public Twitch application id (Device Code Flow public client: there is
/// no secret). Not a constant of the crate: builds inject it, because the
/// project has no registered application in the repository.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub struct ClientId(String);

impl ClientId {
    /// Validates the shape (non-empty, ASCII alphanumeric, as Twitch issues).
    pub fn new(value: impl Into<String>) -> Result<Self> {
        let value = value.into();
        let value = value.trim();
        if value.is_empty() || !value.bytes().all(|b| b.is_ascii_alphanumeric()) {
            return Err(Error::InvalidArgument(
                "client id must be non-empty ASCII alphanumeric",
            ));
        }
        Ok(Self(value.to_owned()))
    }

    /// The id as sent in the `Client-Id` header.
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

/// User access token (from Device Code Flow, TWI-002). `Debug` never shows
/// the value, so tokens do not leak into logs.
#[derive(Clone, PartialEq, Eq)]
pub struct AccessToken(String);

impl AccessToken {
    /// Wraps a raw token (without the `Bearer ` prefix).
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// Raw token value; use only to build the `Authorization` header or to
    /// store it in the secret store.
    pub fn secret(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for AccessToken {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("AccessToken(<redacted>)")
    }
}

/// Client configuration. Everything that differs between builds or tests is
/// here, nothing is hard-coded.
#[derive(Debug, Clone)]
pub struct HelixConfig {
    /// Application id.
    pub client_id: ClientId,
    /// Base URL without the trailing slash; [`DEFAULT_BASE_URL`] in
    /// production, a local mock in tests.
    pub base_url: String,
    /// `User-Agent` header: the product name, never "… for Twitch".
    pub user_agent: String,
    /// Retries after `429` and transient `5xx`.
    pub max_retries: u32,
    /// Longest wait for the bucket to refill; a longer one fails fast with
    /// [`Error::RateLimited`] instead of freezing the UI.
    pub max_wait: Duration,
    /// First backoff when Twitch gives no reset time (doubles per retry).
    pub retry_backoff: Duration,
    /// TTL of the user (avatar, nickname) cache; clamped to 24 h.
    pub user_cache_ttl: Duration,
    /// TTL of the category cache; clamped to 24 h.
    pub category_cache_ttl: Duration,
    /// Entries per cache.
    pub cache_capacity: usize,
}

impl HelixConfig {
    /// Defaults for production with the given client id.
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            base_url: DEFAULT_BASE_URL.to_owned(),
            user_agent: format!("Kinolune/{}", env!("CARGO_PKG_VERSION")),
            max_retries: 3,
            max_wait: Duration::from_secs(60),
            retry_backoff: Duration::from_secs(1),
            user_cache_ttl: MAX_CACHE_TTL,
            category_cache_ttl: MAX_CACHE_TTL,
            cache_capacity: 4096,
        }
    }
}

/// Filters of `GET /streams`. Empty filters mean "top live streams".
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StreamsQuery {
    /// Broadcaster ids (≤ 100 together with logins).
    pub user_ids: Vec<String>,
    /// Broadcaster logins.
    pub user_logins: Vec<String>,
    /// Category ids (≤ 100).
    pub game_ids: Vec<String>,
    /// Languages (ISO 639-1).
    pub languages: Vec<String>,
    /// Page size (≤ 100).
    pub first: Option<u32>,
    /// Cursor of the next page.
    pub after: Option<Cursor>,
}

/// Filters of `GET /videos`: exactly one of `ids`, `user_id`, `game_id`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct VideosQuery {
    /// Video ids (≤ 100).
    pub ids: Vec<String>,
    /// Owner id.
    pub user_id: Option<String>,
    /// Category id.
    pub game_id: Option<String>,
    /// Only this kind (`archive` = VODs).
    pub video_type: Option<VideoType>,
    /// `time`, `trending` or `views`.
    pub sort: Option<String>,
    /// `all`, `day`, `month` or `week`.
    pub period: Option<String>,
    /// Language filter.
    pub language: Option<String>,
    /// Page size (≤ 100).
    pub first: Option<u32>,
    /// Cursor of the next page.
    pub after: Option<Cursor>,
}

impl VideosQuery {
    /// Videos of a channel.
    pub fn by_user(user_id: impl Into<String>) -> Self {
        Self {
            user_id: Some(user_id.into()),
            ..Self::default()
        }
    }

    /// Videos of a category.
    pub fn by_game(game_id: impl Into<String>) -> Self {
        Self {
            game_id: Some(game_id.into()),
            ..Self::default()
        }
    }

    /// Specific videos.
    pub fn by_ids(ids: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            ids: ids.into_iter().map(Into::into).collect(),
            ..Self::default()
        }
    }
}

/// Filters of `GET /clips`: exactly one of `broadcaster_id`, `game_id`, `ids`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ClipsQuery {
    /// Clips of a channel.
    pub broadcaster_id: Option<String>,
    /// Clips of a category.
    pub game_id: Option<String>,
    /// Specific clips (≤ 100).
    pub ids: Vec<String>,
    /// RFC 3339 lower bound.
    pub started_at: Option<String>,
    /// RFC 3339 upper bound.
    pub ended_at: Option<String>,
    /// Only featured (or only not featured).
    pub is_featured: Option<bool>,
    /// Page size (≤ 100).
    pub first: Option<u32>,
    /// Cursor of the next page.
    pub after: Option<Cursor>,
}

/// Filters of `GET /search/channels`.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SearchChannelsQuery {
    /// Search text.
    pub query: String,
    /// Only channels that are live now.
    pub live_only: bool,
    /// Page size (≤ 100).
    pub first: Option<u32>,
    /// Cursor of the next page.
    pub after: Option<Cursor>,
}

/// A schedule page: the schedule and the cursor of more segments.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SchedulePage {
    /// Schedule with this page's segments.
    pub schedule: Schedule,
    /// Cursor of the next page.
    pub next: Option<Cursor>,
}

/// A followed streamer for the "my streamers" screen: follow record,
/// profile (avatar, nickname) and live stream, if live.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FollowedStreamer {
    /// Follow record.
    pub channel: FollowedChannel,
    /// Profile; `None` if Twitch did not return it (banned or deleted user).
    pub user: Option<User>,
    /// Current stream; `None` when offline.
    pub stream: Option<Stream>,
}

impl FollowedStreamer {
    /// Name to show: the profile's display name, else the follow record's.
    pub fn display_name(&self) -> &str {
        self.user
            .as_ref()
            .map_or(&self.channel.broadcaster_name, |u| &u.display_name)
    }

    /// Avatar URL, if known.
    pub fn avatar_url(&self) -> Option<&str> {
        self.user
            .as_ref()
            .map(|u| u.profile_image_url.as_str())
            .filter(|s| !s.is_empty())
    }

    /// Live now.
    pub fn is_live(&self) -> bool {
        self.stream.is_some()
    }
}

#[derive(Deserialize)]
struct Envelope<T> {
    data: T,
    #[serde(default)]
    pagination: Option<PaginationField>,
    #[serde(default)]
    total: Option<u64>,
    #[serde(default)]
    template: Option<String>,
}

#[derive(Deserialize, Default)]
struct PaginationField {
    #[serde(default)]
    cursor: Option<String>,
}

impl<T> Envelope<T> {
    fn next(&self) -> Option<Cursor> {
        self.pagination
            .as_ref()
            .and_then(|p| p.cursor.clone())
            .filter(|c| !c.is_empty())
            .map(Cursor)
    }
}

impl<T> Envelope<Vec<T>> {
    fn into_page(self) -> Page<T> {
        let next = self.next();
        Page {
            items: self.data,
            next,
            total: self.total,
        }
    }
}

#[derive(Deserialize)]
struct ErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    error: String,
}

type Query = Vec<(&'static str, String)>;

/// Async Helix client. Cheap to share behind an `Arc`; all state (token,
/// rate-limit bucket, caches) is internally synchronised.
pub struct HelixClient {
    config: HelixConfig,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    token: RwLock<Option<AccessToken>>,
    ratelimit: Mutex<RateLimitState>,
    users: Mutex<TtlCache<String, User>>,
    categories: Mutex<TtlCache<String, Category>>,
}

impl std::fmt::Debug for HelixClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelixClient")
            .field("config", &self.config)
            .field("signed_in", &self.is_signed_in())
            .finish_non_exhaustive()
    }
}

impl HelixClient {
    /// Client over an injected transport, with the system clock.
    pub fn new(config: HelixConfig, transport: Arc<dyn Transport>) -> Self {
        Self::with_clock(config, transport, Arc::new(SystemClock))
    }

    /// Client over an injected transport and clock (tests).
    pub fn with_clock(
        config: HelixConfig,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
    ) -> Self {
        let users = TtlCache::new(config.user_cache_ttl, config.cache_capacity);
        let categories = TtlCache::new(config.category_cache_ttl, config.cache_capacity);
        Self {
            config,
            transport,
            clock,
            token: RwLock::new(None),
            ratelimit: Mutex::new(RateLimitState::default()),
            users: Mutex::new(users),
            categories: Mutex::new(categories),
        }
    }

    /// Client over HTTPS (reqwest + rustls/ring).
    #[cfg(feature = "reqwest")]
    pub fn with_default_transport(config: HelixConfig) -> Result<Self> {
        let transport = crate::ReqwestTransport::new(Duration::from_secs(30))
            .map_err(|e| Error::Transport(e.0))?;
        Ok(Self::new(config, Arc::new(transport)))
    }

    /// Configuration in use.
    pub fn config(&self) -> &HelixConfig {
        &self.config
    }

    /// Sets the user token (after sign-in or refresh).
    pub fn set_access_token(&self, token: AccessToken) {
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = Some(token);
    }

    /// Signs out: forgets the token and every cached Twitch datum (the
    /// Developer Agreement requires deleting user data on revocation).
    pub fn sign_out(&self) {
        *self.token.write().unwrap_or_else(|e| e.into_inner()) = None;
        self.users.lock().unwrap_or_else(|e| e.into_inner()).clear();
        self.categories
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .clear();
    }

    /// `true` when a token is set (it may still be expired).
    pub fn is_signed_in(&self) -> bool {
        self.token
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .is_some()
    }

    /// Last known rate-limit bucket.
    pub fn rate_limit(&self) -> RateLimitState {
        *self.ratelimit.lock().unwrap_or_else(|e| e.into_inner())
    }

    // ----- request loop ---------------------------------------------------

    fn build_request(&self, method: Method, path: &str, query: &Query) -> Result<HttpRequest> {
        let token = self
            .token
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
            .ok_or(Error::MissingToken)?;
        let mut url = format!("{}/{}", self.config.base_url.trim_end_matches('/'), path);
        for (i, (key, value)) in query.iter().enumerate() {
            url.push(if i == 0 { '?' } else { '&' });
            url.push_str(key);
            url.push('=');
            url.push_str(&encode_query_component(value));
        }
        Ok(HttpRequest {
            method,
            url,
            headers: vec![
                (
                    "Client-Id".to_owned(),
                    self.config.client_id.as_str().to_owned(),
                ),
                (
                    "Authorization".to_owned(),
                    format!("Bearer {}", token.secret()),
                ),
                ("User-Agent".to_owned(), self.config.user_agent.clone()),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            body: None,
        })
    }

    fn backoff(&self, attempt: u32) -> Duration {
        self.config
            .retry_backoff
            .saturating_mul(1u32 << attempt.min(16))
    }

    /// Sends one request with the rate-limit bucket and retries applied.
    /// Returns the 2xx response, or `Ok(None)` for `404` when `allow_404`.
    async fn send(&self, request: HttpRequest, allow_404: bool) -> Result<Option<HttpResponse>> {
        let mut attempt = 0;
        loop {
            let wait = self
                .ratelimit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .wait_before_send(self.clock.now());
            if let Some(wait) = wait {
                if wait > self.config.max_wait {
                    return Err(Error::RateLimited { retry_after: wait });
                }
                self.clock.sleep(wait).await;
            }
            self.ratelimit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .consume(self.clock.now());

            let response = self
                .transport
                .send(request.clone())
                .await
                .map_err(|e| Error::Transport(e.0))?;
            self.ratelimit
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .update(&response);

            match response.status {
                200..=299 => return Ok(Some(response)),
                404 if allow_404 => return Ok(None),
                429 => {
                    let wait = retry_after_429(&response, self.clock.now(), self.backoff(attempt));
                    if attempt >= self.config.max_retries || wait > self.config.max_wait {
                        return Err(Error::RateLimited { retry_after: wait });
                    }
                    {
                        // Make sure the bucket gate also holds other callers.
                        let mut state = self.ratelimit.lock().unwrap_or_else(|e| e.into_inner());
                        state.remaining = Some(0);
                        state.reset_at = Some(self.clock.now() + wait);
                    }
                    self.clock.sleep(wait).await;
                }
                500 | 502 | 503 | 504 if attempt < self.config.max_retries => {
                    self.clock.sleep(self.backoff(attempt)).await;
                }
                401 => {
                    return Err(Error::Unauthorized {
                        message: error_message(&response),
                    });
                }
                status => {
                    return Err(Error::Api {
                        status,
                        message: error_message(&response),
                    });
                }
            }
            attempt += 1;
        }
    }

    async fn get<T: DeserializeOwned>(&self, path: &str, query: Query) -> Result<Envelope<T>> {
        self.get_opt(path, query, false)
            .await?
            .ok_or_else(|| Error::Decode("empty response".to_owned()))
    }

    async fn get_opt<T: DeserializeOwned>(
        &self,
        path: &str,
        query: Query,
        allow_404: bool,
    ) -> Result<Option<Envelope<T>>> {
        let request = self.build_request(Method::Get, path, &query)?;
        let Some(response) = self.send(request, allow_404).await? else {
            return Ok(None);
        };
        serde_json::from_slice(&response.body)
            .map(Some)
            .map_err(|e| Error::Decode(format!("{path}: {e}")))
    }

    // ----- users ----------------------------------------------------------

    /// The signed-in user (`GET /users` without parameters).
    pub async fn get_current_user(&self) -> Result<User> {
        let env: Envelope<Vec<User>> = self.get("users", Vec::new()).await?;
        env.data
            .into_iter()
            .next()
            .ok_or_else(|| Error::Decode("users: empty data for the token owner".to_owned()))
    }

    /// Profiles (avatar, nickname) by ids and logins. Served from the 24-hour
    /// cache when possible, the rest fetched in batches of 100. The result has
    /// no duplicates; unknown ids are simply absent.
    pub async fn get_users(&self, ids: &[&str], logins: &[&str]) -> Result<Vec<User>> {
        let now = self.clock.now();
        let mut found: Vec<User> = Vec::new();
        let mut missing: Vec<(&'static str, String)> = Vec::new();
        {
            let cache = self.users.lock().unwrap_or_else(|e| e.into_inner());
            for id in ids {
                match cache.get(&format!("id:{id}"), now) {
                    Some(user) => found.push(user),
                    None => missing.push(("id", (*id).to_owned())),
                }
            }
            for login in logins {
                let login = login.to_ascii_lowercase();
                match cache.get(&format!("login:{login}"), now) {
                    Some(user) => found.push(user),
                    None => missing.push(("login", login)),
                }
            }
        }
        dedup_pairs(&mut missing);
        for chunk in missing.chunks(MAX_IDS_PER_CALL) {
            let env: Envelope<Vec<User>> = self.get("users", chunk.to_vec()).await?;
            let mut cache = self.users.lock().unwrap_or_else(|e| e.into_inner());
            let now = self.clock.now();
            for user in env.data {
                cache.insert(format!("id:{}", user.id), user.clone(), now);
                cache.insert(format!("login:{}", user.login), user.clone(), now);
                found.push(user);
            }
        }
        let mut seen = HashSet::new();
        found.retain(|u| seen.insert(u.id.clone()));
        Ok(found)
    }

    // ----- follows --------------------------------------------------------

    /// One page of channels `user_id` follows (`user:read:follows`).
    pub async fn get_followed_channels(
        &self,
        user_id: &str,
        first: Option<u32>,
        after: Option<&Cursor>,
    ) -> Result<Page<FollowedChannel>> {
        let mut query = vec![("user_id", user_id.to_owned())];
        push_paging(&mut query, first, after);
        Ok(self
            .get::<Vec<FollowedChannel>>("channels/followed", query)
            .await?
            .into_page())
    }

    /// One page of followed channels that are live now (`user:read:follows`).
    pub async fn get_followed_streams(
        &self,
        user_id: &str,
        first: Option<u32>,
        after: Option<&Cursor>,
    ) -> Result<Page<Stream>> {
        let mut query = vec![("user_id", user_id.to_owned())];
        push_paging(&mut query, first, after);
        Ok(self
            .get::<Vec<Stream>>("streams/followed", query)
            .await?
            .into_page())
    }

    /// All followed channels (up to `limit`), de-duplicated.
    pub async fn all_followed_channels(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<FollowedChannel>> {
        collect_pages(limit, |cursor| async move {
            self.get_followed_channels(user_id, Some(MAX_PAGE_SIZE), cursor.as_ref())
                .await
        })
        .await
    }

    /// All followed live streams (up to `limit`), de-duplicated.
    pub async fn all_followed_streams(&self, user_id: &str, limit: usize) -> Result<Vec<Stream>> {
        collect_pages(limit, |cursor| async move {
            self.get_followed_streams(user_id, Some(MAX_PAGE_SIZE), cursor.as_ref())
                .await
        })
        .await
    }

    /// "My streamers": every followed channel (up to `limit`) with avatar,
    /// nickname and live stream. Live channels come first (most viewers
    /// first), then offline ones alphabetically by display name.
    pub async fn followed_overview(
        &self,
        user_id: &str,
        limit: usize,
    ) -> Result<Vec<FollowedStreamer>> {
        let channels = self.all_followed_channels(user_id, limit).await?;
        let streams = self.all_followed_streams(user_id, limit).await?;
        let ids: Vec<&str> = channels.iter().map(|c| c.broadcaster_id.as_str()).collect();
        let users = self.get_users(&ids, &[]).await?;

        let mut users: HashMap<String, User> =
            users.into_iter().map(|u| (u.id.clone(), u)).collect();
        let mut streams: HashMap<String, Stream> = streams
            .into_iter()
            .map(|s| (s.user_id.clone(), s))
            .collect();
        let mut out: Vec<FollowedStreamer> = channels
            .into_iter()
            .map(|channel| FollowedStreamer {
                user: users.remove(&channel.broadcaster_id),
                stream: streams.remove(&channel.broadcaster_id),
                channel,
            })
            .collect();
        out.sort_by(|a, b| match (&a.stream, &b.stream) {
            (Some(x), Some(y)) => y.viewer_count.cmp(&x.viewer_count),
            (Some(_), None) => std::cmp::Ordering::Less,
            (None, Some(_)) => std::cmp::Ordering::Greater,
            (None, None) => a
                .display_name()
                .to_lowercase()
                .cmp(&b.display_name().to_lowercase()),
        });
        Ok(out)
    }

    // ----- streams and channels ------------------------------------------

    /// `GET /streams`.
    pub async fn get_streams(&self, q: &StreamsQuery) -> Result<Page<Stream>> {
        if q.user_ids.len() + q.user_logins.len() > MAX_IDS_PER_CALL
            || q.game_ids.len() > MAX_IDS_PER_CALL
        {
            return Err(Error::InvalidArgument("at most 100 users and 100 games"));
        }
        let mut query = Query::new();
        query.extend(q.user_ids.iter().map(|v| ("user_id", v.clone())));
        query.extend(q.user_logins.iter().map(|v| ("user_login", v.clone())));
        query.extend(q.game_ids.iter().map(|v| ("game_id", v.clone())));
        query.extend(q.languages.iter().map(|v| ("language", v.clone())));
        push_paging(&mut query, q.first, q.after.as_ref());
        Ok(self.get::<Vec<Stream>>("streams", query).await?.into_page())
    }

    /// `GET /channels` for any number of broadcasters (batched by 100).
    pub async fn get_channels(&self, broadcaster_ids: &[&str]) -> Result<Vec<ChannelInfo>> {
        let mut out = Vec::new();
        for chunk in broadcaster_ids.chunks(MAX_IDS_PER_CALL) {
            let query = chunk
                .iter()
                .map(|id| ("broadcaster_id", (*id).to_owned()))
                .collect();
            out.extend(self.get::<Vec<ChannelInfo>>("channels", query).await?.data);
        }
        Ok(out)
    }

    /// `GET /search/channels`.
    pub async fn search_channels(
        &self,
        q: &SearchChannelsQuery,
    ) -> Result<Page<ChannelSearchResult>> {
        if q.query.trim().is_empty() {
            return Err(Error::InvalidArgument("empty search query"));
        }
        let mut query = vec![("query", q.query.clone())];
        if q.live_only {
            query.push(("live_only", "true".to_owned()));
        }
        push_paging(&mut query, q.first, q.after.as_ref());
        Ok(self
            .get::<Vec<ChannelSearchResult>>("search/channels", query)
            .await?
            .into_page())
    }

    /// `GET /schedule`; `None` when the channel has no schedule (Helix 404).
    pub async fn get_schedule(
        &self,
        broadcaster_id: &str,
        first: Option<u32>,
        after: Option<&Cursor>,
    ) -> Result<Option<SchedulePage>> {
        let mut query = vec![("broadcaster_id", broadcaster_id.to_owned())];
        push_paging(&mut query, first.map(|f| f.min(25)), after);
        Ok(self
            .get_opt::<Schedule>("schedule", query, true)
            .await?
            .map(|env| {
                let next = env.next();
                SchedulePage {
                    schedule: env.data,
                    next,
                }
            }))
    }

    // ----- videos and clips ----------------------------------------------

    /// `GET /videos` (VODs, highlights, uploads).
    pub async fn get_videos(&self, q: &VideosQuery) -> Result<Page<Video>> {
        let selectors = usize::from(!q.ids.is_empty())
            + usize::from(q.user_id.is_some())
            + usize::from(q.game_id.is_some());
        if selectors != 1 {
            return Err(Error::InvalidArgument(
                "videos: exactly one of ids, user_id, game_id",
            ));
        }
        if q.ids.len() > MAX_IDS_PER_CALL {
            return Err(Error::InvalidArgument("videos: at most 100 ids"));
        }
        let mut query = Query::new();
        query.extend(q.ids.iter().map(|v| ("id", v.clone())));
        if let Some(v) = &q.user_id {
            query.push(("user_id", v.clone()));
        }
        if let Some(v) = &q.game_id {
            query.push(("game_id", v.clone()));
        }
        if let Some(t) = q.video_type {
            let t = match t {
                VideoType::Archive => "archive",
                VideoType::Highlight => "highlight",
                VideoType::Upload => "upload",
                VideoType::Other => "all",
            };
            query.push(("type", t.to_owned()));
        }
        for (key, value) in [
            ("sort", &q.sort),
            ("period", &q.period),
            ("language", &q.language),
        ] {
            if let Some(v) = value {
                query.push((key, v.clone()));
            }
        }
        // Helix rejects paging parameters together with `id`.
        if q.ids.is_empty() {
            push_paging(&mut query, q.first, q.after.as_ref());
        }
        Ok(self.get::<Vec<Video>>("videos", query).await?.into_page())
    }

    /// `GET /clips`.
    pub async fn get_clips(&self, q: &ClipsQuery) -> Result<Page<Clip>> {
        let selectors = usize::from(!q.ids.is_empty())
            + usize::from(q.broadcaster_id.is_some())
            + usize::from(q.game_id.is_some());
        if selectors != 1 {
            return Err(Error::InvalidArgument(
                "clips: exactly one of ids, broadcaster_id, game_id",
            ));
        }
        if q.ids.len() > MAX_IDS_PER_CALL {
            return Err(Error::InvalidArgument("clips: at most 100 ids"));
        }
        let mut query = Query::new();
        query.extend(q.ids.iter().map(|v| ("id", v.clone())));
        for (key, value) in [
            ("broadcaster_id", &q.broadcaster_id),
            ("game_id", &q.game_id),
            ("started_at", &q.started_at),
            ("ended_at", &q.ended_at),
        ] {
            if let Some(v) = value {
                query.push((key, v.clone()));
            }
        }
        if let Some(featured) = q.is_featured {
            query.push(("is_featured", featured.to_string()));
        }
        if q.ids.is_empty() {
            push_paging(&mut query, q.first, q.after.as_ref());
        }
        Ok(self.get::<Vec<Clip>>("clips", query).await?.into_page())
    }

    // ----- categories -----------------------------------------------------

    /// `GET /games` by ids and names, through the 24-hour cache by id.
    pub async fn get_categories(&self, ids: &[&str], names: &[&str]) -> Result<Vec<Category>> {
        let now = self.clock.now();
        let mut found = Vec::new();
        let mut missing: Vec<(&'static str, String)> = Vec::new();
        {
            let cache = self.categories.lock().unwrap_or_else(|e| e.into_inner());
            for id in ids {
                match cache.get(&(*id).to_owned(), now) {
                    Some(c) => found.push(c),
                    None => missing.push(("id", (*id).to_owned())),
                }
            }
        }
        missing.extend(names.iter().map(|n| ("name", (*n).to_owned())));
        dedup_pairs(&mut missing);
        for chunk in missing.chunks(MAX_IDS_PER_CALL) {
            let env: Envelope<Vec<Category>> = self.get("games", chunk.to_vec()).await?;
            self.remember_categories(&env.data);
            found.extend(env.data);
        }
        let mut seen = HashSet::new();
        found.retain(|c| seen.insert(c.id.clone()));
        Ok(found)
    }

    /// `GET /games/top`: most watched categories now.
    pub async fn get_top_categories(
        &self,
        first: Option<u32>,
        after: Option<&Cursor>,
    ) -> Result<Page<Category>> {
        let mut query = Query::new();
        push_paging(&mut query, first, after);
        let page = self
            .get::<Vec<Category>>("games/top", query)
            .await?
            .into_page();
        self.remember_categories(&page.items);
        Ok(page)
    }

    /// `GET /search/categories`.
    pub async fn search_categories(
        &self,
        text: &str,
        first: Option<u32>,
        after: Option<&Cursor>,
    ) -> Result<Page<Category>> {
        if text.trim().is_empty() {
            return Err(Error::InvalidArgument("empty search query"));
        }
        let mut query = vec![("query", text.to_owned())];
        push_paging(&mut query, first, after);
        Ok(self
            .get::<Vec<Category>>("search/categories", query)
            .await?
            .into_page())
    }

    fn remember_categories(&self, categories: &[Category]) {
        let now = self.clock.now();
        let mut cache = self.categories.lock().unwrap_or_else(|e| e.into_inner());
        for c in categories {
            cache.insert(c.id.clone(), c.clone(), now);
        }
    }

    // ----- chat metadata --------------------------------------------------

    /// `GET /chat/emotes/global`.
    pub async fn get_global_emotes(&self) -> Result<EmoteList> {
        self.emotes("chat/emotes/global", Vec::new()).await
    }

    /// `GET /chat/emotes` of a channel.
    pub async fn get_channel_emotes(&self, broadcaster_id: &str) -> Result<EmoteList> {
        self.emotes(
            "chat/emotes",
            vec![("broadcaster_id", broadcaster_id.to_owned())],
        )
        .await
    }

    /// `GET /chat/emotes/set` (batched by 25 set ids).
    pub async fn get_emote_sets(&self, set_ids: &[&str]) -> Result<EmoteList> {
        let mut all = EmoteList {
            emotes: Vec::new(),
            template: String::new(),
        };
        for chunk in set_ids.chunks(MAX_EMOTE_SETS_PER_CALL) {
            let query = chunk
                .iter()
                .map(|id| ("emote_set_id", (*id).to_owned()))
                .collect();
            let part = self.emotes("chat/emotes/set", query).await?;
            all.template = part.template;
            all.emotes.extend(part.emotes);
        }
        Ok(all)
    }

    async fn emotes(&self, path: &str, query: Query) -> Result<EmoteList> {
        let env: Envelope<Vec<crate::model::Emote>> = self.get(path, query).await?;
        Ok(EmoteList {
            emotes: env.data,
            template: env.template.unwrap_or_default(),
        })
    }

    /// `GET /chat/badges/global`.
    pub async fn get_global_badges(&self) -> Result<Vec<BadgeSet>> {
        Ok(self
            .get::<Vec<BadgeSet>>("chat/badges/global", Vec::new())
            .await?
            .data)
    }

    /// `GET /chat/badges` of a channel.
    pub async fn get_channel_badges(&self, broadcaster_id: &str) -> Result<Vec<BadgeSet>> {
        Ok(self
            .get::<Vec<BadgeSet>>(
                "chat/badges",
                vec![("broadcaster_id", broadcaster_id.to_owned())],
            )
            .await?
            .data)
    }

    /// `GET /chat/color` (batched by 100 users).
    pub async fn get_user_chat_colors(&self, user_ids: &[&str]) -> Result<Vec<UserChatColor>> {
        let mut out = Vec::new();
        for chunk in user_ids.chunks(MAX_IDS_PER_CALL) {
            let query = chunk
                .iter()
                .map(|id| ("user_id", (*id).to_owned()))
                .collect();
            out.extend(
                self.get::<Vec<UserChatColor>>("chat/color", query)
                    .await?
                    .data,
            );
        }
        Ok(out)
    }

    /// `GET /chat/settings` of a channel.
    pub async fn get_chat_settings(&self, broadcaster_id: &str) -> Result<ChatSettings> {
        let env: Envelope<Vec<ChatSettings>> = self
            .get(
                "chat/settings",
                vec![("broadcaster_id", broadcaster_id.to_owned())],
            )
            .await?;
        env.data
            .into_iter()
            .next()
            .ok_or_else(|| Error::Decode("chat/settings: empty data".to_owned()))
    }
}

fn push_paging(query: &mut Query, first: Option<u32>, after: Option<&Cursor>) {
    if let Some(first) = first {
        query.push(("first", first.clamp(1, MAX_PAGE_SIZE).to_string()));
    }
    if let Some(after) = after {
        query.push(("after", after.0.clone()));
    }
}

fn dedup_pairs(pairs: &mut Vec<(&'static str, String)>) {
    let mut seen = HashSet::new();
    pairs.retain(|p| seen.insert(p.clone()));
}

fn error_message(response: &HttpResponse) -> String {
    match serde_json::from_slice::<ErrorBody>(&response.body) {
        Ok(body) if !body.message.is_empty() => body.message,
        Ok(body) if !body.error.is_empty() => body.error,
        _ => String::from_utf8_lossy(&response.body)
            .chars()
            .take(200)
            .collect(),
    }
}
