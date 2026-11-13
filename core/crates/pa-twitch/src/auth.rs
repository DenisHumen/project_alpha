// SPDX-License-Identifier: MPL-2.0
//! Sign-in with the OAuth Device Code Flow (TWI-002).
//!
//! One flow on every device (research 06 §1.3): the app is a **public
//! client** — it has a client id and never a client secret. The device asks
//! Twitch for a `user_code`, shows it together with a QR code of
//! [`DeviceAuthorization::verification_uri`] (TV) or an "Open in browser"
//! button (phone, desktop), and polls the token endpoint until the user
//! confirms on `twitch.tv/activate`.
//!
//! Token rules (research 04 §5.1.1):
//!
//! - **Per device.** Every device signs in on its own and keeps its own
//!   tokens in the OS secret store ([`SecretKey::twitch_tokens`]). Tokens are
//!   never synchronised, never put in the sync journal, URLs or logs;
//!   `Debug` of every type here is redacted and the values are registered
//!   with [`pa_secrets::redact`].
//! - **Refresh early and one at a time.** The access token is refreshed when
//!   it has less than [`AuthConfig::refresh_margin`] (10 minutes) left. The
//!   refresh token of a public client is single-use, so two parallel
//!   refreshes would invalidate each other: refreshes are serialised and a
//!   caller that waited for a running refresh reuses its result.
//! - **30 days.** A public-client refresh token dies after 30 days of
//!   inactivity. Twitch then answers `Invalid refresh token`; the stored
//!   tokens are deleted and [`AuthError::ReauthRequired`] tells the app to
//!   ask the user to sign in again.
//! - **Validate** on start and hourly ([`TwitchAuth::validate_if_due`]), as
//!   Twitch requires from apps that use OAuth.
//! - **Minimal scopes**: [`DEFAULT_SCOPES`]; [`SCOPE_WRITE_CHAT`] only when
//!   the user enables sending chat messages.
//!
//! [`AuthorizedHelix`] ties this to [`HelixClient`]: it hands the client a
//! fresh token before each call and, when Helix answers `401`
//! ([`Error::Unauthorized`]), refreshes once and retries.

use std::future::Future;
use std::sync::{Arc, Mutex, PoisonError};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use pa_secrets::{SecretKey, SecretStore, SecretString, forget_known, redact, register_known};
use serde::{Deserialize, Serialize};

use crate::client::{AccessToken, ClientId, HelixClient};
use crate::error::Error;
use crate::transport::{Clock, HttpRequest, HttpResponse, Method, SystemClock, Transport};
use crate::util::encode_query_component;

/// Production base URL of the Twitch identity service.
pub const DEFAULT_ID_BASE_URL: &str = "https://id.twitch.tv/oauth2";
/// Read the followed channels and live streams.
pub const SCOPE_READ_FOLLOWS: &str = "user:read:follows";
/// Read chat as the user (EventSub `channel.chat.message`, TWI-010).
pub const SCOPE_READ_CHAT: &str = "user:read:chat";
/// Send chat messages; request only when the user turns sending on.
pub const SCOPE_WRITE_CHAT: &str = "user:write:chat";
/// Scopes requested by default.
pub const DEFAULT_SCOPES: &[&str] = &[SCOPE_READ_FOLLOWS, SCOPE_READ_CHAT];
/// Refresh the access token when less than this is left.
pub const REFRESH_MARGIN: Duration = Duration::from_secs(10 * 60);
/// How often Twitch wants an app to validate its token.
pub const VALIDATE_INTERVAL: Duration = Duration::from_secs(60 * 60);

const DEVICE_GRANT: &str = "urn:ietf:params:oauth:grant-type:device_code";
/// Extra wait Twitch asks for on `slow_down` (RFC 8628 §3.5).
const SLOW_DOWN_STEP: Duration = Duration::from_secs(5);
const STORED_VERSION: u32 = 1;

/// Result alias of the sign-in API.
pub type AuthResult<T> = std::result::Result<T, AuthError>;

/// Everything sign-in, refresh and authorised calls can fail with. No
/// variant carries a token; Twitch messages pass through [`redact`].
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum AuthError {
    /// The request never got a response.
    #[error("network error: {0}")]
    Transport(String),
    /// No account is signed in on this device.
    #[error("not signed in to Twitch")]
    NotSignedIn,
    /// The user declined the authorisation on `twitch.tv/activate`.
    #[error("the sign-in was declined")]
    AccessDenied,
    /// The `user_code` expired before the user confirmed; start again.
    #[error("the sign-in code expired")]
    DeviceCodeExpired,
    /// The refresh token is no longer valid (30 days of inactivity, revoked
    /// in the Twitch settings, password change): sign in again.
    #[error("Twitch sign-in expired, please sign in again")]
    ReauthRequired,
    /// Any other answer of the identity service.
    #[error("Twitch identity error {status}: {message}")]
    Twitch {
        /// HTTP status.
        status: u16,
        /// Twitch message (redacted).
        message: String,
    },
    /// The body is not the JSON we expect.
    #[error("unexpected Twitch identity response: {0}")]
    Decode(String),
    /// The secret store failed (locked keychain, no storage).
    #[error("cannot access stored Twitch tokens: {0}")]
    Secrets(String),
    /// A Helix call failed for another reason than an expired token.
    #[error(transparent)]
    Helix(#[from] Error),
}

/// Sign-in configuration; nothing is hard-coded.
#[derive(Debug, Clone)]
pub struct AuthConfig {
    /// Public application id (no secret exists).
    pub client_id: ClientId,
    /// [`DEFAULT_ID_BASE_URL`] in production, a mock in tests.
    pub id_base_url: String,
    /// Scopes to request.
    pub scopes: Vec<String>,
    /// `User-Agent`: the product name, never "… for Twitch".
    pub user_agent: String,
    /// Refresh when less than this is left ([`REFRESH_MARGIN`]).
    pub refresh_margin: Duration,
    /// Interval of [`TwitchAuth::validate_if_due`] ([`VALIDATE_INTERVAL`]).
    pub validate_interval: Duration,
}

impl AuthConfig {
    /// Production defaults with [`DEFAULT_SCOPES`].
    pub fn new(client_id: ClientId) -> Self {
        Self {
            client_id,
            id_base_url: DEFAULT_ID_BASE_URL.to_owned(),
            scopes: DEFAULT_SCOPES.iter().map(|s| (*s).to_owned()).collect(),
            user_agent: format!("Kinolune/{}", env!("CARGO_PKG_VERSION")),
            refresh_margin: REFRESH_MARGIN,
            validate_interval: VALIDATE_INTERVAL,
        }
    }
}

/// A pending device sign-in: what to show the user.
pub struct DeviceAuthorization {
    /// Code the user types on the activation page.
    pub user_code: String,
    /// Activation page; encode it in the QR code on TVs (it already carries
    /// the code, so scanning is enough).
    pub verification_uri: String,
    /// When the code stops working.
    pub expires_at: SystemTime,
    /// Current polling interval (grows on `slow_down`).
    pub interval: Duration,
    // Whoever has it can collect the tokens: kept private and redacted.
    device_code: SecretString,
}

impl std::fmt::Debug for DeviceAuthorization {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeviceAuthorization")
            .field("user_code", &self.user_code)
            .field("verification_uri", &self.verification_uri)
            .field("expires_at", &self.expires_at)
            .field("interval", &self.interval)
            .finish_non_exhaustive()
    }
}

/// The signed-in account on this device (no secrets).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Account {
    /// Twitch user id; also the name of the stored secret.
    pub user_id: String,
    /// Login name.
    pub login: String,
    /// Granted scopes.
    pub scopes: Vec<String>,
    /// When the current access token expires.
    pub expires_at: SystemTime,
}

/// Answer of `GET /oauth2/validate`.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
pub struct Validation {
    /// Application the token belongs to.
    pub client_id: String,
    /// Login of the token owner.
    #[serde(default)]
    pub login: String,
    /// User id of the token owner.
    #[serde(default)]
    pub user_id: String,
    /// Granted scopes.
    #[serde(default)]
    pub scopes: Vec<String>,
    /// Seconds the access token has left.
    pub expires_in: u64,
}

#[derive(Clone)]
struct Tokens {
    access: AccessToken,
    refresh: SecretString,
    expires_at: u64,
    scopes: Vec<String>,
    user_id: String,
    login: String,
}

impl Tokens {
    fn account(&self) -> Account {
        Account {
            user_id: self.user_id.clone(),
            login: self.login.clone(),
            scopes: self.scopes.clone(),
            expires_at: UNIX_EPOCH + Duration::from_secs(self.expires_at),
        }
    }

    fn register(&self) {
        register_known(&SecretString::new(self.access.secret()));
        register_known(&self.refresh);
    }

    fn forget(&self) {
        forget_known(&SecretString::new(self.access.secret()));
        forget_known(&self.refresh);
    }
}

#[derive(Serialize)]
struct StoredRef<'a> {
    v: u32,
    access_token: &'a str,
    refresh_token: &'a str,
    expires_at: u64,
    scopes: &'a [String],
    user_id: &'a str,
    login: &'a str,
}

#[derive(Deserialize)]
struct Stored {
    access_token: String,
    refresh_token: String,
    expires_at: u64,
    #[serde(default)]
    scopes: Vec<String>,
    user_id: String,
    #[serde(default)]
    login: String,
}

#[derive(Deserialize)]
struct DeviceResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    expires_in: u64,
    #[serde(default = "default_interval")]
    interval: u64,
}

fn default_interval() -> u64 {
    5
}

#[derive(Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: String,
    expires_in: u64,
    #[serde(default)]
    scope: Vec<String>,
}

#[derive(Deserialize, Default)]
struct ErrorBody {
    #[serde(default)]
    message: String,
    #[serde(default)]
    error: String,
}

#[derive(Default)]
struct State {
    tokens: Option<Tokens>,
    validated_at: Option<SystemTime>,
}

/// Twitch sign-in of this device: Device Code Flow, refresh, validation,
/// revocation and token storage. Share it behind an `Arc`.
pub struct TwitchAuth {
    config: AuthConfig,
    transport: Arc<dyn Transport>,
    clock: Arc<dyn Clock>,
    store: Arc<dyn SecretStore>,
    state: Mutex<State>,
    refresh_lock: tokio::sync::Mutex<()>,
}

impl std::fmt::Debug for TwitchAuth {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TwitchAuth")
            .field("config", &self.config)
            .field("account", &self.account())
            .field("store", &self.store.backend())
            .finish_non_exhaustive()
    }
}

impl TwitchAuth {
    /// Sign-in over an injected transport, with the system clock.
    pub fn new(
        config: AuthConfig,
        transport: Arc<dyn Transport>,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        Self::with_clock(config, transport, Arc::new(SystemClock), store)
    }

    /// Sign-in over an injected transport and clock (tests).
    pub fn with_clock(
        config: AuthConfig,
        transport: Arc<dyn Transport>,
        clock: Arc<dyn Clock>,
        store: Arc<dyn SecretStore>,
    ) -> Self {
        Self {
            config,
            transport,
            clock,
            store,
            state: Mutex::new(State::default()),
            refresh_lock: tokio::sync::Mutex::new(()),
        }
    }

    /// Sign-in over HTTPS (reqwest + rustls/ring).
    #[cfg(feature = "reqwest")]
    pub fn with_default_transport(
        config: AuthConfig,
        store: Arc<dyn SecretStore>,
    ) -> AuthResult<Self> {
        let transport = crate::ReqwestTransport::new(Duration::from_secs(30))
            .map_err(|e| AuthError::Transport(e.0))?;
        Ok(Self::new(config, Arc::new(transport), store))
    }

    /// Configuration in use.
    pub fn config(&self) -> &AuthConfig {
        &self.config
    }

    /// The signed-in account, if any.
    pub fn account(&self) -> Option<Account> {
        self.lock().tokens.as_ref().map(Tokens::account)
    }

    /// Loads the tokens this device stored for `user_id` (the app keeps the
    /// id of the last account in its settings; it is not a secret). No
    /// network: call [`TwitchAuth::validate`] afterwards.
    pub async fn restore(&self, user_id: &str) -> AuthResult<Option<Account>> {
        let key = secret_key(user_id)?;
        let Some(value) = self.blocking(move |s| s.get(&key)).await? else {
            return Ok(None);
        };
        let stored: Stored = serde_json::from_str(value.expose())
            .map_err(|e| AuthError::Decode(format!("stored tokens: {e}")))?;
        let tokens = Tokens {
            access: AccessToken::new(stored.access_token),
            refresh: SecretString::new(stored.refresh_token),
            expires_at: stored.expires_at,
            scopes: stored.scopes,
            user_id: stored.user_id,
            login: stored.login,
        };
        let account = tokens.account();
        self.replace(Some(tokens));
        Ok(Some(account))
    }

    // ----- Device Code Flow ----------------------------------------------

    /// Starts a sign-in: `POST /device`. Show `user_code` and a QR code of
    /// `verification_uri`, then call [`TwitchAuth::complete_device_login`].
    pub async fn start_device_login(&self) -> AuthResult<DeviceAuthorization> {
        let scopes = self.config.scopes.join(" ");
        let request = self.form(
            "device",
            &[
                ("client_id", self.config.client_id.as_str()),
                ("scopes", &scopes),
            ],
        );
        let response = self.send(request).await?;
        if !is_success(&response) {
            return Err(twitch_error(&response));
        }
        let device: DeviceResponse = decode(&response, "device")?;
        Ok(DeviceAuthorization {
            user_code: device.user_code,
            verification_uri: device.verification_uri,
            expires_at: self.clock.now() + Duration::from_secs(device.expires_in),
            interval: Duration::from_secs(device.interval.max(1)),
            device_code: SecretString::new(device.device_code),
        })
    }

    /// Polls once. `Ok(None)` while the user has not confirmed yet; on
    /// `slow_down` the interval in `device` grows.
    pub async fn poll_device_login(
        &self,
        device: &mut DeviceAuthorization,
    ) -> AuthResult<Option<Account>> {
        if self.clock.now() >= device.expires_at {
            return Err(AuthError::DeviceCodeExpired);
        }
        let scopes = self.config.scopes.join(" ");
        let request = self.form(
            "token",
            &[
                ("client_id", self.config.client_id.as_str()),
                ("scopes", &scopes),
                ("device_code", device.device_code.expose()),
                ("grant_type", DEVICE_GRANT),
            ],
        );
        let response = self.send(request).await?;
        if !is_success(&response) {
            let message = error_text(&response).to_ascii_lowercase();
            return if message.contains("authorization_pending") {
                Ok(None)
            } else if message.contains("slow_down") {
                device.interval += SLOW_DOWN_STEP;
                Ok(None)
            } else if message.contains("access_denied") || message.contains("denied") {
                Err(AuthError::AccessDenied)
            } else if message.contains("expired") || message.contains("invalid device code") {
                Err(AuthError::DeviceCodeExpired)
            } else {
                Err(twitch_error(&response))
            };
        }
        let token: TokenResponse = decode(&response, "token")?;
        let access = AccessToken::new(token.access_token);
        // Who signed in: the token endpoint does not say, `validate` does.
        let validation = self.validate_token(&access).await?.ok_or_else(|| {
            AuthError::Twitch {
                status: 401,
                message: "the new token was rejected".to_owned(),
            }
        })?;
        let tokens = Tokens {
            access,
            refresh: SecretString::new(token.refresh_token),
            expires_at: self.unix_now() + token.expires_in,
            scopes: if token.scope.is_empty() {
                validation.scopes
            } else {
                token.scope
            },
            user_id: validation.user_id,
            login: validation.login,
        };
        self.persist(&tokens).await?;
        let account = tokens.account();
        self.replace(Some(tokens));
        self.lock().validated_at = Some(self.clock.now());
        Ok(Some(account))
    }

    /// Polls at the interval Twitch asks for until the user confirms, the
    /// code expires or the user declines. Drop the future to cancel.
    pub async fn complete_device_login(
        &self,
        mut device: DeviceAuthorization,
    ) -> AuthResult<Account> {
        loop {
            self.clock.sleep(device.interval).await;
            if let Some(account) = self.poll_device_login(&mut device).await? {
                return Ok(account);
            }
        }
    }

    // ----- tokens --------------------------------------------------------

    /// A usable access token, refreshed first when it has less than the
    /// refresh margin left. Parallel callers share a single refresh.
    pub async fn access_token(&self) -> AuthResult<AccessToken> {
        let tokens = self.current()?;
        if !self.needs_refresh(&tokens) {
            return Ok(tokens.access);
        }
        self.refresh_if(|t| self.needs_refresh(t))
            .await
            .map(|t| t.access)
    }

    /// Refreshes after Twitch rejected `rejected` with `401`. If another
    /// caller has already replaced that token, its result is returned
    /// without a second refresh.
    pub async fn refresh_after_unauthorized(
        &self,
        rejected: &AccessToken,
    ) -> AuthResult<AccessToken> {
        self.refresh_if(|t| &t.access == rejected)
            .await
            .map(|t| t.access)
    }

    /// Refreshes now, whatever the expiry.
    pub async fn refresh(&self) -> AuthResult<Account> {
        let before = self.current()?.access;
        self.refresh_if(|t| t.access == before)
            .await
            .map(|t| t.account())
    }

    /// `GET /validate`. On `401` refreshes once and validates the new
    /// token.
    pub async fn validate(&self) -> AuthResult<Validation> {
        let token = self.access_token().await?;
        let validation = match self.validate_token(&token).await? {
            Some(v) => v,
            None => {
                let fresh = self.refresh_after_unauthorized(&token).await?;
                self.validate_token(&fresh)
                    .await?
                    .ok_or(AuthError::ReauthRequired)?
            }
        };
        self.lock().validated_at = Some(self.clock.now());
        Ok(validation)
    }

    /// [`TwitchAuth::validate`] when the last validation is older than the
    /// validate interval; `Ok(None)` when it is not due.
    pub async fn validate_if_due(&self) -> AuthResult<Option<Validation>> {
        let last = self.lock().validated_at;
        let due = match last {
            None => true,
            // A clock that went backwards counts as due.
            Some(at) => {
                self.clock.now().duration_since(at).unwrap_or(Duration::MAX)
                    >= self.config.validate_interval
            }
        };
        if due {
            self.validate().await.map(Some)
        } else {
            Ok(None)
        }
    }

    /// Signs out: revokes the access token at Twitch (best effort: a network
    /// failure does not keep the tokens) and deletes them from the store.
    pub async fn sign_out(&self) -> AuthResult<()> {
        let _guard = self.refresh_lock.lock().await;
        let Some(tokens) = self.lock().tokens.clone() else {
            return Ok(());
        };
        let request = self.form(
            "revoke",
            &[
                ("client_id", self.config.client_id.as_str()),
                ("token", tokens.access.secret()),
            ],
        );
        // Ignored on purpose: the local copy goes away in any case.
        let _ = self.send(request).await;
        self.forget_tokens(&tokens).await
    }

    // ----- internals -----------------------------------------------------

    async fn refresh_if(&self, still_needed: impl Fn(&Tokens) -> bool) -> AuthResult<Tokens> {
        let _guard = self.refresh_lock.lock().await;
        let current = self.current()?;
        if !still_needed(&current) {
            return Ok(current);
        }
        let request = self.form(
            "token",
            &[
                ("client_id", self.config.client_id.as_str()),
                ("grant_type", "refresh_token"),
                ("refresh_token", current.refresh.expose()),
            ],
        );
        let response = self.send(request).await?;
        if !is_success(&response) {
            let message = error_text(&response).to_ascii_lowercase();
            if response.status == 401 || message.contains("invalid refresh token") {
                self.forget_tokens(&current).await?;
                return Err(AuthError::ReauthRequired);
            }
            return Err(twitch_error(&response));
        }
        let token: TokenResponse = decode(&response, "refresh")?;
        let fresh = Tokens {
            access: AccessToken::new(token.access_token),
            refresh: SecretString::new(token.refresh_token),
            expires_at: self.unix_now() + token.expires_in,
            scopes: if token.scope.is_empty() {
                current.scopes.clone()
            } else {
                token.scope
            },
            user_id: current.user_id.clone(),
            login: current.login.clone(),
        };
        // The old refresh token is spent: keep the new pair in memory even
        // if the store fails, then report the store error.
        self.replace(Some(fresh.clone()));
        self.persist(&fresh).await?;
        Ok(fresh)
    }

    async fn validate_token(&self, token: &AccessToken) -> AuthResult<Option<Validation>> {
        let request = HttpRequest {
            method: Method::Get,
            url: self.url("validate"),
            headers: vec![
                (
                    "Authorization".to_owned(),
                    format!("OAuth {}", token.secret()),
                ),
                ("User-Agent".to_owned(), self.config.user_agent.clone()),
            ],
            body: None,
        };
        let response = self.send(request).await?;
        if response.status == 401 {
            return Ok(None);
        }
        if !is_success(&response) {
            return Err(twitch_error(&response));
        }
        decode(&response, "validate").map(Some)
    }

    async fn persist(&self, tokens: &Tokens) -> AuthResult<()> {
        let key = secret_key(&tokens.user_id)?;
        let doc = serde_json::to_string(&StoredRef {
            v: STORED_VERSION,
            access_token: tokens.access.secret(),
            refresh_token: tokens.refresh.expose(),
            expires_at: tokens.expires_at,
            scopes: &tokens.scopes,
            user_id: &tokens.user_id,
            login: &tokens.login,
        })
        .map_err(|e| AuthError::Decode(e.to_string()))?;
        let value = SecretString::new(doc);
        self.blocking(move |s| s.set(&key, &value)).await
    }

    async fn forget_tokens(&self, tokens: &Tokens) -> AuthResult<()> {
        self.replace(None);
        self.lock().validated_at = None;
        let key = secret_key(&tokens.user_id)?;
        self.blocking(move |s| s.delete(&key)).await.map(|_| ())
    }

    fn replace(&self, tokens: Option<Tokens>) {
        let new = tokens.clone();
        let old = std::mem::replace(&mut self.lock().tokens, tokens);
        // Forget first: a restore of the same tokens must stay registered.
        if let Some(old) = old {
            old.forget();
        }
        if let Some(new) = new {
            new.register();
        }
    }

    fn current(&self) -> AuthResult<Tokens> {
        self.lock().tokens.clone().ok_or(AuthError::NotSignedIn)
    }

    fn needs_refresh(&self, tokens: &Tokens) -> bool {
        self.unix_now() + self.config.refresh_margin.as_secs() >= tokens.expires_at
    }

    fn unix_now(&self) -> u64 {
        self.clock
            .now()
            .duration_since(UNIX_EPOCH)
            .map_or(0, |d| d.as_secs())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(PoisonError::into_inner)
    }

    fn url(&self, path: &str) -> String {
        format!(
            "{}/{}",
            self.config.id_base_url.trim_end_matches('/'),
            path
        )
    }

    /// Form POST; secrets go in the body, never in the URL.
    fn form(&self, path: &str, fields: &[(&str, &str)]) -> HttpRequest {
        let body = fields
            .iter()
            .map(|(k, v)| format!("{k}={}", encode_query_component(v)))
            .collect::<Vec<_>>()
            .join("&");
        HttpRequest {
            method: Method::Post,
            url: self.url(path),
            headers: vec![
                (
                    "Content-Type".to_owned(),
                    "application/x-www-form-urlencoded".to_owned(),
                ),
                ("User-Agent".to_owned(), self.config.user_agent.clone()),
                ("Accept".to_owned(), "application/json".to_owned()),
            ],
            body: Some(body.into_bytes()),
        }
    }

    async fn send(&self, request: HttpRequest) -> AuthResult<HttpResponse> {
        self.transport
            .send(request)
            .await
            .map_err(|e| AuthError::Transport(redact(&e.0)))
    }

    /// Secret stores block (a keychain may prompt): run them off the
    /// async workers.
    async fn blocking<T: Send + 'static>(
        &self,
        op: impl FnOnce(&dyn SecretStore) -> pa_secrets::Result<T> + Send + 'static,
    ) -> AuthResult<T> {
        let store = self.store.clone();
        tokio::task::spawn_blocking(move || op(&*store))
            .await
            .map_err(|e| AuthError::Secrets(e.to_string()))?
            .map_err(|e| AuthError::Secrets(e.to_string()))
    }
}

fn secret_key(user_id: &str) -> AuthResult<SecretKey> {
    SecretKey::twitch_tokens(user_id).map_err(|e| AuthError::Secrets(e.to_string()))
}

fn is_success(response: &HttpResponse) -> bool {
    (200..300).contains(&response.status)
}

fn decode<T: serde::de::DeserializeOwned>(response: &HttpResponse, what: &str) -> AuthResult<T> {
    // serde messages may quote the body: never let a token through.
    serde_json::from_slice(&response.body).map_err(|e| AuthError::Decode(redact(&format!("{what}: {e}"))))
}

fn error_text(response: &HttpResponse) -> String {
    let body: ErrorBody = serde_json::from_slice(&response.body).unwrap_or_default();
    if body.message.is_empty() {
        body.error
    } else {
        body.message
    }
}

fn twitch_error(response: &HttpResponse) -> AuthError {
    AuthError::Twitch {
        status: response.status,
        message: redact(&error_text(response)),
    }
}

/// [`HelixClient`] with tokens from [`TwitchAuth`]: a fresh token before
/// every call, one refresh and retry when Helix answers `401`.
#[derive(Debug, Clone)]
pub struct AuthorizedHelix {
    auth: Arc<TwitchAuth>,
    helix: Arc<HelixClient>,
}

impl AuthorizedHelix {
    /// Binds a client to a sign-in.
    pub fn new(auth: Arc<TwitchAuth>, helix: Arc<HelixClient>) -> Self {
        Self { auth, helix }
    }

    /// The sign-in.
    pub fn auth(&self) -> &Arc<TwitchAuth> {
        &self.auth
    }

    /// The Helix client.
    pub fn helix(&self) -> &Arc<HelixClient> {
        &self.helix
    }

    /// Runs a Helix call with a valid token:
    ///
    /// ```no_run
    /// # async fn demo(api: pa_twitch::AuthorizedHelix) -> pa_twitch::AuthResult<()> {
    /// let me = api.call(|helix| async move { helix.get_current_user().await }).await?;
    /// # Ok(()) }
    /// ```
    pub async fn call<T, F, Fut>(&self, op: F) -> AuthResult<T>
    where
        F: Fn(Arc<HelixClient>) -> Fut,
        Fut: Future<Output = crate::Result<T>>,
    {
        let token = self.with_session_end(self.auth.access_token().await)?;
        self.helix.set_access_token(token.clone());
        match op(self.helix.clone()).await {
            Err(Error::Unauthorized { .. }) => {
                let fresh =
                    self.with_session_end(self.auth.refresh_after_unauthorized(&token).await)?;
                self.helix.set_access_token(fresh);
                Ok(op(self.helix.clone()).await?)
            }
            other => Ok(other?),
        }
    }

    /// Signs out of Twitch and clears the Helix token and caches.
    pub async fn sign_out(&self) -> AuthResult<()> {
        self.helix.sign_out();
        self.auth.sign_out().await
    }

    /// When the session is over, the Helix client must forget the user too.
    fn with_session_end<T>(&self, result: AuthResult<T>) -> AuthResult<T> {
        if matches!(
            result,
            Err(AuthError::ReauthRequired | AuthError::NotSignedIn)
        ) {
            self.helix.sign_out();
        }
        result
    }
}
