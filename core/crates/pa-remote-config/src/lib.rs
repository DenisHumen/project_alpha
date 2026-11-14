// SPDX-License-Identifier: MPL-2.0
//! Remote config without project servers (CORE-014, ADR-0004).
//!
//! The owner publishes `config/remote-config.json` and its minisign
//! signature `config/remote-config.json.minisig` in the public repo; the app
//! downloads them as static files (raw.githubusercontent.com, GitHub Pages),
//! checks the signature against the key compiled into the app and only then
//! parses the document. Order of trust, best first:
//!
//! 1. a document downloaded and verified in this run ([`ConfigSource::Remote`]);
//! 2. the last valid copy on disk, re-verified on load ([`ConfigSource::Cache`]);
//! 3. the copy compiled into the app ([`ConfigSource::BuiltIn`], [`BUILTIN_JSON`]).
//!
//! Refreshes send `If-None-Match` with the cached `ETag`, never accept a
//! document with a smaller `serial` (anti-rollback) and never replace a good
//! copy with a bad one: offline or a bad signature leaves the current copy in
//! place. While [`PRODUCTION_PUBLIC_KEY`] is a placeholder there is no
//! trusted key and the app runs on the built-in copy only.
//!
//! The document carries typed flags and kill-switches (by feature, app
//! version range and platform) plus the Twitch values TWI-004 needs
//! (GQL client-id, persisted-query hashes, player mode): see [`model`].

mod cache;
mod client;
mod fetch;
pub mod model;
mod verify;

pub use client::{
    ClientOptions, ConfigSource, DEFAULT_FILE_NAME, Endpoint, RefreshOutcome, RemoteConfigClient,
    Snapshot, default_endpoints,
};
#[cfg(feature = "http")]
pub use fetch::UreqFetcher;
pub use fetch::{ConfigFetcher, FetchError, Fetched, MAX_CONFIG_BYTES};
pub use model::{
    ClientContext, KillSwitch, PlayerMode, RefreshPolicy, RemoteConfig, SUPPORTED_SCHEMA,
    TwitchConfig, TwitchFlags, flags,
};
pub use verify::{
    PLACEHOLDER_MARKER, PRODUCTION_PUBLIC_KEY, TrustedKey, production_key_is_placeholder,
};

/// The copy compiled into the app: the newest signed document at release
/// time (or the safe defaults before the first one). Must stay valid —
/// checked by the tests.
pub const BUILTIN_JSON: &str = include_str!("../builtin/remote-config.json");

/// The built-in document, parsed.
pub fn builtin() -> RemoteConfig {
    RemoteConfig::parse(BUILTIN_JSON.as_bytes())
        .expect("built-in remote config is checked by the crate's tests")
}

/// Errors of this crate. None of them carries URLs with user data or
/// secrets: the only URLs involved are public static files.
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum Error {
    /// The production key is still a placeholder: remote updates are off.
    #[error("remote config updates are disabled: no trusted minisign key")]
    NoTrustedKey,
    /// The configured public key is malformed.
    #[error("bad minisign public key: {0}")]
    BadKey(String),
    /// No response (DNS, TCP, TLS, timeout).
    #[error("offline: {0}")]
    Offline(String),
    /// A response other than 200/304.
    #[error("unexpected HTTP status {status}")]
    Http { status: u16 },
    /// The signature is missing, malformed, made by another key, does not
    /// match the bytes or names another file.
    #[error("bad signature: {0}")]
    BadSignature(String),
    /// Signed, but not a valid document.
    #[error("invalid remote config: {0}")]
    Invalid(String),
    /// Signed, but its `schema` is newer than this app understands.
    #[error("unsupported remote config schema {0}")]
    UnsupportedSchema(u32),
    /// Signed, but older than the copy the app already has.
    #[error("remote config serial {got} is older than the current {have}")]
    Rollback { got: u64, have: u64 },
    /// The disk cache could not be written.
    #[error("cache I/O error: {0}")]
    Io(String),
}

#[cfg(test)]
mod tests;
