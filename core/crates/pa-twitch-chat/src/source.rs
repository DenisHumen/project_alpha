// SPDX-License-Identifier: MPL-2.0
//! The [`ChatSource`] trait, errors, reconnect backoff and the Helix calls a
//! signed-in source needs ([`ChatApi`]).

use std::time::Duration;

use pa_twitch::{
    AuthError, AuthorizedHelix, BoxFuture, EventSubSubscription, EventSubSubscriptionRequest,
    HelixClient, SendChatMessage, SentChatMessage,
};

use crate::model::ChatEvent;

/// Chat failures.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ChatError {
    /// Could not open the WebSocket (DNS, TCP, TLS, handshake).
    #[error("cannot connect to Twitch chat: {0}")]
    Connect(String),
    /// The socket failed after it was open.
    #[error("chat connection error: {0}")]
    Socket(String),
    /// Twitch sent something we cannot decode.
    #[error("unexpected chat data: {0}")]
    Protocol(String),
    /// Anonymous IRC cannot send messages; sign in first.
    #[error("anonymous chat is read-only")]
    ReadOnly,
    /// The token is invalid or the session ended; sign in again.
    #[error("not signed in to Twitch")]
    Unauthorized,
    /// Helix refused the call.
    #[error("Twitch API error: {0}")]
    Api(String),
    /// Arguments Twitch would reject.
    #[error("invalid chat request: {0}")]
    InvalidArgument(&'static str),
    /// The source was closed.
    #[error("chat source closed")]
    Closed,
}

impl From<pa_twitch::Error> for ChatError {
    fn from(e: pa_twitch::Error) -> Self {
        match e {
            pa_twitch::Error::Unauthorized { .. } | pa_twitch::Error::MissingToken => {
                ChatError::Unauthorized
            }
            pa_twitch::Error::InvalidArgument(m) => ChatError::InvalidArgument(m),
            other => ChatError::Api(other.to_string()),
        }
    }
}

impl From<AuthError> for ChatError {
    fn from(e: AuthError) -> Self {
        match e {
            AuthError::ReauthRequired | AuthError::NotSignedIn => ChatError::Unauthorized,
            AuthError::Helix(inner) => inner.into(),
            other => ChatError::Api(other.to_string()),
        }
    }
}

/// Which implementation a source is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SourceKind {
    /// Anonymous IRC over WebSocket (`justinfan…`), read-only.
    AnonymousIrc,
    /// EventSub WebSocket + Helix send, signed in.
    EventSub,
}

/// A live chat feed (research 06 §3.1). Two implementations:
/// [`crate::IrcChatSource`] without sign-in and for many channels,
/// [`crate::EventSubChatSource`] with sign-in (reading and sending).
///
/// Sources run their connection loop in a Tokio task, reconnect with
/// backoff on their own and report it as [`crate::SourceStatus`] events.
pub trait ChatSource: Send + Sync {
    /// Implementation kind.
    fn kind(&self) -> SourceKind;

    /// `true` when [`ChatSource::send_message`] can work.
    fn can_send(&self) -> bool;

    /// Next event; `None` once the source has stopped for good.
    fn next_event(&self) -> BoxFuture<'_, Option<ChatEvent>>;

    /// Sends `text` to the channel `broadcaster_id`, optionally as a reply.
    fn send_message<'a>(
        &'a self,
        broadcaster_id: &'a str,
        text: &'a str,
        reply_to: Option<&'a str>,
    ) -> BoxFuture<'a, Result<SentChatMessage, ChatError>>;

    /// Stops the connection loop; [`ChatSource::next_event`] then drains
    /// what is buffered and returns `None`.
    fn close(&self);
}

/// Exponential reconnect backoff: `initial · 2^(attempt-1)`, capped at
/// `max`, with optional jitter (random 50–100 % of the step) so that many
/// clients do not reconnect in lock-step after a Twitch outage.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Backoff {
    /// First delay.
    pub initial: Duration,
    /// Longest delay.
    pub max: Duration,
    /// Randomise each delay.
    pub jitter: bool,
}

impl Default for Backoff {
    fn default() -> Self {
        Self {
            initial: Duration::from_secs(1),
            max: Duration::from_secs(60),
            jitter: true,
        }
    }
}

impl Backoff {
    /// Delay before attempt `attempt` (1-based).
    pub fn delay(&self, attempt: u32) -> Duration {
        let step = self
            .initial
            .saturating_mul(1u32 << attempt.saturating_sub(1).min(20))
            .min(self.max);
        if !self.jitter {
            return step;
        }
        let mut b = [0u8; 2];
        if getrandom::fill(&mut b).is_err() {
            return step;
        }
        // 50 % … 100 % of the step.
        let permille = 500 + u64::from(u16::from_le_bytes(b)) * 500 / u64::from(u16::MAX);
        Duration::from_millis(
            u64::try_from(step.as_millis())
                .unwrap_or(u64::MAX)
                .saturating_mul(permille)
                / 1000,
        )
    }
}

/// Helix calls of the signed-in source. Implemented by [`HelixClient`]
/// (token set by the caller) and by [`AuthorizedHelix`] (token refreshed on
/// `401`).
pub trait ChatApi: Send + Sync {
    /// `POST /eventsub/subscriptions` (WebSocket transport).
    fn subscribe(
        &self,
        request: EventSubSubscriptionRequest,
    ) -> BoxFuture<'_, Result<EventSubSubscription, ChatError>>;

    /// `POST /chat/messages`.
    fn send(&self, message: SendChatMessage) -> BoxFuture<'_, Result<SentChatMessage, ChatError>>;
}

impl ChatApi for HelixClient {
    fn subscribe(
        &self,
        request: EventSubSubscriptionRequest,
    ) -> BoxFuture<'_, Result<EventSubSubscription, ChatError>> {
        Box::pin(async move { Ok(self.create_eventsub_subscription(&request).await?) })
    }

    fn send(&self, message: SendChatMessage) -> BoxFuture<'_, Result<SentChatMessage, ChatError>> {
        Box::pin(async move { Ok(self.send_chat_message(&message).await?) })
    }
}

impl ChatApi for AuthorizedHelix {
    fn subscribe(
        &self,
        request: EventSubSubscriptionRequest,
    ) -> BoxFuture<'_, Result<EventSubSubscription, ChatError>> {
        Box::pin(async move {
            let request = &request;
            Ok(self
                .call(|helix| async move { helix.create_eventsub_subscription(request).await })
                .await?)
        })
    }

    fn send(&self, message: SendChatMessage) -> BoxFuture<'_, Result<SentChatMessage, ChatError>> {
        Box::pin(async move {
            let message = &message;
            Ok(self
                .call(|helix| async move { helix.send_chat_message(message).await })
                .await?)
        })
    }
}
