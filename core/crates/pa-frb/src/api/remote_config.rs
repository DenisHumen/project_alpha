// SPDX-License-Identifier: MPL-2.0
//! Remote config (`pa-remote-config`, CORE-014) for Dart: signed flags and
//! kill-switches from the public repo, with disk cache and built-in copy.
//!
//! Dart opens one [`CoreRemoteConfig`] at start (no network), reads flags
//! synchronously and calls [`CoreRemoteConfig::refresh_if_due`] from time to
//! time (app start, resume); errors there never change the flags in use.

use std::collections::HashMap;
use std::sync::Arc;

use flutter_rust_bridge::frb;
use pa_remote_config as rc;

/// `User-Agent` of config downloads.
const USER_AGENT: &str = concat!(
    "Kinolune/",
    env!("CARGO_PKG_VERSION"),
    " (+https://github.com/DenisHumen/project_alpha)"
);

/// Error of a refresh; Dart receives it as an exception. The flags in use
/// stay as they were.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfigError {
    /// Human-readable message (no secrets; only public URLs are involved).
    pub message: String,
    /// No network: try again later.
    pub offline: bool,
    /// Remote updates are off in this build (no production key yet).
    pub disabled: bool,
}

impl From<rc::Error> for RemoteConfigError {
    fn from(e: rc::Error) -> Self {
        Self {
            offline: matches!(e, rc::Error::Offline(_)),
            disabled: matches!(e, rc::Error::NoTrustedKey),
            message: e.to_string(),
        }
    }
}

/// Where the flags in use came from.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteConfigSource {
    BuiltIn,
    Cache,
    Remote,
}

/// State of the remote config.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RemoteConfigStatus {
    pub source: RemoteConfigSource,
    /// `serial` of the document in use.
    pub serial: u64,
    /// False while the build has no production key: built-in copy only.
    pub remote_enabled: bool,
    /// Unix seconds of the last successful check.
    pub checked_at: Option<u64>,
    /// Unix seconds of the last failed check after it.
    pub failed_at: Option<u64>,
}

/// Result of a refresh.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RemoteConfigRefresh {
    /// A newer document is in use now.
    Updated,
    /// Nothing new.
    Unchanged,
    /// Not time yet (refresh policy).
    NotDue,
}

/// Twitch player to use.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TwitchPlayer {
    Native,
    Embed,
}

/// Twitch flags resolved for this app (flags minus kill-switches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CoreTwitchFlags {
    pub gql_playback: bool,
    pub chapters_gql: bool,
    pub chat_replay: bool,
    pub archive: bool,
    pub player: TwitchPlayer,
    pub gql_client_id: Option<String>,
    pub persisted_queries: HashMap<String, String>,
}

/// The app's remote config client.
#[frb(opaque)]
pub struct CoreRemoteConfig {
    inner: Arc<rc::RemoteConfigClient>,
    context: rc::ClientContext,
}

impl CoreRemoteConfig {
    /// Opens the client with its cache in `cache_dir` (e.g.
    /// `<app support>/remote-config`). `app_version` is the app's semver,
    /// `platform` the lowercase OS name (`android`, `linux`, `ios`, …): both
    /// only select kill-switches locally and are never sent anywhere.
    /// Does not touch the network.
    #[frb(sync)]
    pub fn open(cache_dir: String, app_version: String, platform: String) -> CoreRemoteConfig {
        let client = rc::RemoteConfigClient::open(
            rc::TrustedKey::production().ok(),
            rc::ClientOptions::new(cache_dir),
            Arc::new(rc::UreqFetcher::new(USER_AGENT)),
        );
        Self {
            inner: Arc::new(client),
            context: rc::ClientContext {
                app_version,
                platform: platform.to_lowercase(),
            },
        }
    }

    /// Downloads a newer document if the refresh policy says it is time.
    pub fn refresh_if_due(&self) -> Result<RemoteConfigRefresh, RemoteConfigError> {
        Ok(self.inner.refresh_if_due()?.into())
    }

    /// Downloads a newer document now (e.g. a "check now" button).
    pub fn refresh(&self) -> Result<RemoteConfigRefresh, RemoteConfigError> {
        Ok(self.inner.refresh()?.into())
    }

    /// Source, serial and check times.
    #[frb(sync)]
    pub fn status(&self) -> RemoteConfigStatus {
        let s = self.inner.snapshot();
        RemoteConfigStatus {
            source: match s.source {
                rc::ConfigSource::BuiltIn => RemoteConfigSource::BuiltIn,
                rc::ConfigSource::Cache => RemoteConfigSource::Cache,
                rc::ConfigSource::Remote => RemoteConfigSource::Remote,
            },
            serial: s.config.serial,
            remote_enabled: self.inner.remote_enabled(),
            checked_at: s.checked_at,
            failed_at: s.failed_at,
        }
    }

    /// Flag `feature` is on and not killed for this app. Missing: off.
    #[frb(sync)]
    pub fn is_enabled(&self, feature: String) -> bool {
        self.inner.config().is_enabled(&feature, &self.context)
    }

    /// A kill-switch turns `feature` off for this app (for features that
    /// are on by default and only need an emergency stop).
    #[frb(sync)]
    pub fn is_killed(&self, feature: String) -> bool {
        self.inner.config().is_killed(&feature, &self.context)
    }

    /// Twitch module settings (TWI-004).
    #[frb(sync)]
    pub fn twitch(&self) -> CoreTwitchFlags {
        let t = self.inner.config().twitch_flags(&self.context);
        CoreTwitchFlags {
            gql_playback: t.gql_playback,
            chapters_gql: t.chapters_gql,
            chat_replay: t.chat_replay,
            archive: t.archive,
            player: match t.player {
                rc::PlayerMode::Native => TwitchPlayer::Native,
                rc::PlayerMode::Embed => TwitchPlayer::Embed,
            },
            gql_client_id: t.gql_client_id,
            persisted_queries: t.persisted_queries.into_iter().collect(),
        }
    }
}

impl From<rc::RefreshOutcome> for RemoteConfigRefresh {
    fn from(o: rc::RefreshOutcome) -> Self {
        match o {
            rc::RefreshOutcome::Updated { .. } => Self::Updated,
            rc::RefreshOutcome::Unchanged => Self::Unchanged,
            rc::RefreshOutcome::NotDue => Self::NotDue,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opens_on_the_builtin_copy_without_network() {
        let dir = tempfile::tempdir().unwrap();
        let config = CoreRemoteConfig::open(
            dir.path().to_string_lossy().into(),
            "0.1.0".into(),
            "Linux".into(),
        );
        let status = config.status();
        assert_eq!(status.source, RemoteConfigSource::BuiltIn);
        if !rc::production_key_is_placeholder() {
            return;
        }
        // No production key yet: remote updates are off and refresh says so
        // without touching the network.
        assert!(!status.remote_enabled);
        assert_eq!(config.refresh_if_due(), Ok(RemoteConfigRefresh::NotDue));
        let err = config.refresh().unwrap_err();
        assert!(err.disabled && !err.offline);
        assert!(!config.is_enabled("twitch.gql_playback".into()));
        assert!(!config.is_killed("twitch.gql_playback".into()));
        let twitch = config.twitch();
        assert_eq!(twitch.player, TwitchPlayer::Embed);
        assert!(!twitch.gql_playback && twitch.gql_client_id.is_none());
    }
}
