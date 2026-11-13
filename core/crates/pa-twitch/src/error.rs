// SPDX-License-Identifier: MPL-2.0
//! Error type of the Helix client.

use std::time::Duration;

/// Result alias of this crate.
pub type Result<T, E = Error> = std::result::Result<T, E>;

/// Everything a Helix call can fail with.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The request never got a response (DNS, TLS, connection, timeout).
    #[error("network error: {0}")]
    Transport(String),
    /// No access token is set; Helix rejects anonymous calls.
    #[error("not signed in to Twitch")]
    MissingToken,
    /// `401`: the token is invalid or expired; refresh it (TWI-002) and retry.
    #[error("Twitch rejected the token: {message}")]
    Unauthorized {
        /// Helix error message.
        message: String,
    },
    /// `429` persisted after all retries, or the bucket resets too far ahead.
    #[error("Twitch rate limit hit, retry in {retry_after:?}")]
    RateLimited {
        /// How long until the bucket refills.
        retry_after: Duration,
    },
    /// Any other non-2xx answer.
    #[error("Twitch API error {status}: {message}")]
    Api {
        /// HTTP status.
        status: u16,
        /// Helix error message (or a short body excerpt).
        message: String,
    },
    /// The body is not the JSON we expect.
    #[error("unexpected Twitch response: {0}")]
    Decode(String),
    /// The caller passed arguments Helix would reject.
    #[error("invalid request: {0}")]
    InvalidArgument(&'static str),
}
