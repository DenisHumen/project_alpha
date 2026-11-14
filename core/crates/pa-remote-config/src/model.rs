// SPDX-License-Identifier: MPL-2.0
//! The typed config document: flags, kill-switches, refresh policy and the
//! Twitch section (client-id and persisted-query hashes for TWI-004).

use std::cmp::Ordering;
use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::Error;

/// Highest `schema` this client understands. A document with a newer schema
/// is rejected (the app keeps its current copy), so the owner can ship a
/// breaking format only together with an app release.
pub const SUPPORTED_SCHEMA: u32 = 1;

/// Shortest refresh interval a document may ask for (15 min): even a broken
/// config cannot make every client hammer GitHub.
pub const MIN_INTERVAL_SECS: u64 = 15 * 60;
/// Longest refresh interval (7 days): kill-switches must arrive in time.
pub const MAX_INTERVAL_SECS: u64 = 7 * 24 * 3600;
/// Shortest delay before a retry after a failed refresh (5 min).
pub const MIN_RETRY_SECS: u64 = 5 * 60;

/// Names of the flags the app reads. Unknown names in a document are kept
/// and can be read with [`RemoteConfig::is_enabled`].
pub mod flags {
    /// Native HLS playback through Twitch GQL + usher (06 §2.1). Off → embed.
    pub const TWITCH_GQL_PLAYBACK: &str = "twitch.gql_playback";
    /// VOD chapters from GQL `moments` (06 §2.3).
    pub const TWITCH_CHAPTERS_GQL: &str = "twitch.chapters_gql";
    /// Online VOD chat replay through GQL (06 §2.5).
    pub const TWITCH_CHAT_REPLAY: &str = "twitch.chat_replay";
    /// Desktop/server archive module (06 §2.7); never in store builds.
    pub const TWITCH_ARCHIVE: &str = "twitch.archive";
}

/// The remote config document (`config/remote-config.json`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct RemoteConfig {
    /// Format version, see [`SUPPORTED_SCHEMA`].
    pub schema: u32,
    /// Monotonic document number. A client never accepts a document with a
    /// smaller serial than the one it already has (anti-rollback: an old,
    /// validly signed file cannot switch a kill-switch back off).
    pub serial: u64,
    /// When the owner issued it (informational, RFC 3339).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub issued_at: Option<String>,
    /// How often clients look for a new copy.
    #[serde(default)]
    pub refresh: RefreshPolicy,
    /// Feature flags. A flag that is missing is off.
    #[serde(default)]
    pub flags: BTreeMap<String, bool>,
    /// Kill-switches: turn a feature off for some app versions/platforms
    /// even if its flag is on.
    #[serde(default)]
    pub kill_switches: Vec<KillSwitch>,
    /// Values for the Twitch module (TWI-004).
    #[serde(default)]
    pub twitch: TwitchConfig,
}

/// How often to refresh. Values are clamped, see [`Self::interval`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct RefreshPolicy {
    /// Seconds between successful checks.
    pub interval_secs: u64,
    /// Seconds before a retry after a failed check (offline, bad signature).
    pub retry_secs: u64,
}

impl Default for RefreshPolicy {
    fn default() -> Self {
        Self {
            interval_secs: 6 * 3600,
            retry_secs: 30 * 60,
        }
    }
}

impl RefreshPolicy {
    /// Interval clamped to [`MIN_INTERVAL_SECS`]..=[`MAX_INTERVAL_SECS`].
    pub fn interval(&self) -> u64 {
        self.interval_secs
            .clamp(MIN_INTERVAL_SECS, MAX_INTERVAL_SECS)
    }

    /// Retry delay clamped to [`MIN_RETRY_SECS`]..=[`Self::interval`].
    pub fn retry(&self) -> u64 {
        self.retry_secs.clamp(MIN_RETRY_SECS, self.interval())
    }
}

/// Turns `feature` off when the app matches every given condition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct KillSwitch {
    /// Flag name, or a prefix pattern such as `twitch.*`, or `*` for all.
    pub feature: String,
    /// Lowest affected app version (inclusive). None: no lower bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub min_app_version: Option<String>,
    /// Highest affected app version (inclusive). None: no upper bound.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub max_app_version: Option<String>,
    /// Affected platforms (`android`, `ios`, `linux`, …). Empty: all.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub platforms: Vec<String>,
    /// Why (shown in logs / diagnostics only).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub reason: Option<String>,
}

/// Values the Twitch module needs and that Twitch may change any day.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct TwitchConfig {
    /// Public `Client-ID` header for `gql.twitch.tv`. None: GQL is unusable.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub gql_client_id: Option<String>,
    /// GQL operation name → persisted-query sha256 hash.
    pub persisted_queries: BTreeMap<String, String>,
    /// Preferred player when GQL playback is allowed.
    pub player: PlayerMode,
}

/// Twitch player choice.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlayerMode {
    /// Native HLS through GQL + usher.
    Native,
    /// Official embed player (WebView). Also any value this client does not
    /// know: the safe choice.
    #[default]
    #[serde(other)]
    Embed,
}

/// Who is asking: kill-switches match on it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ClientContext {
    /// App version, semver (`0.1.0-alpha.3`).
    pub app_version: String,
    /// Platform name, lowercase (`android`, `ios`, `tvos`, `macos`,
    /// `linux`, `windows`).
    pub platform: String,
}

/// Twitch settings resolved for one client (flags ∧ ¬kill-switches).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TwitchFlags {
    pub gql_playback: bool,
    pub chapters_gql: bool,
    pub chat_replay: bool,
    pub archive: bool,
    /// Effective player: `Embed` whenever GQL playback is off or killed.
    pub player: PlayerMode,
    /// Client-id, only when GQL playback is on.
    pub gql_client_id: Option<String>,
    /// Persisted-query hashes, empty when GQL playback is off.
    pub persisted_queries: BTreeMap<String, String>,
}

impl RemoteConfig {
    /// Parses and validates a document.
    pub fn parse(bytes: &[u8]) -> Result<Self, Error> {
        let config: Self =
            serde_json::from_slice(bytes).map_err(|e| Error::Invalid(e.to_string()))?;
        config.validate()?;
        Ok(config)
    }

    /// Checks the schema and every version bound.
    pub fn validate(&self) -> Result<(), Error> {
        if self.schema == 0 || self.schema > SUPPORTED_SCHEMA {
            return Err(Error::UnsupportedSchema(self.schema));
        }
        for ks in &self.kill_switches {
            if ks.feature.is_empty() {
                return Err(Error::Invalid("kill-switch without a feature".into()));
            }
            for v in [&ks.min_app_version, &ks.max_app_version]
                .into_iter()
                .flatten()
            {
                if Version::parse(v).is_none() {
                    return Err(Error::Invalid(format!("bad version bound {v:?}")));
                }
            }
        }
        Ok(())
    }

    /// The first kill-switch that turns `feature` off for `ctx`.
    pub fn kill_switch(&self, feature: &str, ctx: &ClientContext) -> Option<&KillSwitch> {
        self.kill_switches
            .iter()
            .find(|ks| ks.matches_feature(feature) && ks.matches_client(ctx))
    }

    /// Is `feature` killed for `ctx` (regardless of its flag)? For features
    /// that are on by default in the app and only need an emergency stop.
    pub fn is_killed(&self, feature: &str, ctx: &ClientContext) -> bool {
        self.kill_switch(feature, ctx).is_some()
    }

    /// Flag on and not killed. A missing flag is off.
    pub fn is_enabled(&self, feature: &str, ctx: &ClientContext) -> bool {
        self.flags.get(feature).copied().unwrap_or(false) && !self.is_killed(feature, ctx)
    }

    /// The Twitch module's view of this config.
    pub fn twitch_flags(&self, ctx: &ClientContext) -> TwitchFlags {
        let gql_playback = self.is_enabled(flags::TWITCH_GQL_PLAYBACK, ctx)
            && self.twitch.gql_client_id.is_some();
        TwitchFlags {
            gql_playback,
            chapters_gql: gql_playback && self.is_enabled(flags::TWITCH_CHAPTERS_GQL, ctx),
            chat_replay: gql_playback && self.is_enabled(flags::TWITCH_CHAT_REPLAY, ctx),
            archive: self.is_enabled(flags::TWITCH_ARCHIVE, ctx),
            player: if gql_playback {
                self.twitch.player
            } else {
                PlayerMode::Embed
            },
            gql_client_id: if gql_playback {
                self.twitch.gql_client_id.clone()
            } else {
                None
            },
            persisted_queries: if gql_playback {
                self.twitch.persisted_queries.clone()
            } else {
                BTreeMap::new()
            },
        }
    }
}

impl KillSwitch {
    fn matches_feature(&self, feature: &str) -> bool {
        match self.feature.strip_suffix('*') {
            Some(prefix) => feature.starts_with(prefix),
            None => self.feature == feature,
        }
    }

    fn matches_client(&self, ctx: &ClientContext) -> bool {
        if !self.platforms.is_empty()
            && !self
                .platforms
                .iter()
                .any(|p| p.eq_ignore_ascii_case(&ctx.platform))
        {
            return false;
        }
        if self.min_app_version.is_none() && self.max_app_version.is_none() {
            return true;
        }
        // An app version we cannot read matches any bound: when in doubt,
        // the kill-switch wins.
        let Some(app) = Version::parse(&ctx.app_version) else {
            return true;
        };
        let above_min = self
            .min_app_version
            .as_deref()
            .and_then(Version::parse)
            .is_none_or(|min| app >= min);
        let below_max = self
            .max_app_version
            .as_deref()
            .and_then(Version::parse)
            .is_none_or(|max| app <= max);
        above_min && below_max
    }
}

/// A semver-like version: numeric core plus optional pre-release, ordered by
/// semver precedence (`0.1.0-alpha.3 < 0.1.0`). Build metadata is ignored.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct Version {
    core: Vec<u64>,
    pre: Vec<String>,
}

impl Version {
    pub(crate) fn parse(s: &str) -> Option<Self> {
        let s = s.trim().trim_start_matches('v');
        let s = s.split('+').next()?;
        let (core, pre) = match s.split_once('-') {
            Some((c, p)) => (c, Some(p)),
            None => (s, None),
        };
        let core = core
            .split('.')
            .map(|p| p.parse::<u64>().ok())
            .collect::<Option<Vec<_>>>()?;
        if core.is_empty() || core.len() > 4 {
            return None;
        }
        let pre = match pre {
            Some(p) if p.is_empty() => return None,
            Some(p) => p.split('.').map(str::to_owned).collect(),
            None => Vec::new(),
        };
        Some(Self { core, pre })
    }
}

impl PartialOrd for Version {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Version {
    fn cmp(&self, other: &Self) -> Ordering {
        let n = self.core.len().max(other.core.len());
        for i in 0..n {
            let a = self.core.get(i).copied().unwrap_or(0);
            let b = other.core.get(i).copied().unwrap_or(0);
            match a.cmp(&b) {
                Ordering::Equal => {}
                o => return o,
            }
        }
        match (self.pre.is_empty(), other.pre.is_empty()) {
            (true, true) => return Ordering::Equal,
            (true, false) => return Ordering::Greater,
            (false, true) => return Ordering::Less,
            (false, false) => {}
        }
        for (a, b) in self.pre.iter().zip(&other.pre) {
            let o = match (a.parse::<u64>(), b.parse::<u64>()) {
                (Ok(x), Ok(y)) => x.cmp(&y),
                (Ok(_), Err(_)) => Ordering::Less,
                (Err(_), Ok(_)) => Ordering::Greater,
                (Err(_), Err(_)) => a.cmp(b),
            };
            if o != Ordering::Equal {
                return o;
            }
        }
        self.pre.len().cmp(&other.pre.len())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ctx(v: &str, p: &str) -> ClientContext {
        ClientContext {
            app_version: v.into(),
            platform: p.into(),
        }
    }

    #[test]
    fn version_order_follows_semver() {
        let v = |s| Version::parse(s).unwrap();
        assert!(v("0.1.0-alpha.3") < v("0.1.0"));
        assert!(v("0.1.0-alpha.2") < v("0.1.0-alpha.10"));
        assert!(v("0.1.0-alpha") < v("0.1.0-alpha.1"));
        assert!(v("0.1.0-alpha.9") < v("0.1.0-beta"));
        assert!(v("0.9.9") < v("0.10.0"));
        assert_eq!(v("1.2"), v("1.2.0"));
        assert_eq!(v("v1.2.0+build.5"), v("1.2.0"));
        assert!(Version::parse("1.x").is_none());
        assert!(Version::parse("1.0-").is_none());
        assert!(Version::parse("").is_none());
    }

    #[test]
    fn kill_switch_matches_feature_version_and_platform() {
        let config: RemoteConfig = serde_json::from_str(
            r#"{
              "schema": 1, "serial": 3,
              "flags": { "twitch.gql_playback": true, "twitch.chat_replay": true, "x.y": true },
              "kill_switches": [
                { "feature": "twitch.chat_replay", "max_app_version": "0.2.0", "platforms": ["ios"] },
                { "feature": "x.*", "min_app_version": "1.0.0" }
              ],
              "twitch": { "gql_client_id": "public-id", "player": "native" }
            }"#,
        )
        .unwrap();
        config.validate().unwrap();
        let old_ios = ctx("0.1.0-alpha.3", "iOS");
        let new_ios = ctx("0.2.1", "ios");
        let old_android = ctx("0.1.0", "android");
        assert!(!config.is_enabled("twitch.chat_replay", &old_ios));
        assert!(config.is_enabled("twitch.chat_replay", &new_ios));
        assert!(config.is_enabled("twitch.chat_replay", &old_android));
        assert!(config.is_enabled("x.y", &old_android));
        assert!(!config.is_enabled("x.y", &ctx("1.0.0", "linux")));
        assert!(config.is_killed("x.z", &ctx("2.0.0", "linux")));
        // Unreadable app version: bounded kill-switches match.
        assert!(config.is_killed("x.y", &ctx("dev", "linux")));
        assert!(!config.is_enabled("missing.flag", &old_android));

        let t = config.twitch_flags(&old_ios);
        assert!(t.gql_playback && !t.chat_replay && !t.chapters_gql);
        assert_eq!(t.player, PlayerMode::Native);
        assert_eq!(t.gql_client_id.as_deref(), Some("public-id"));
    }

    #[test]
    fn killing_gql_playback_falls_back_to_embed() {
        let mut config: RemoteConfig = serde_json::from_str(
            r#"{ "schema": 1, "serial": 1,
                 "flags": { "twitch.gql_playback": true, "twitch.chapters_gql": true },
                 "twitch": { "gql_client_id": "id", "persisted_queries": { "PlaybackAccessToken": "abc" }, "player": "native" } }"#,
        )
        .unwrap();
        let c = ctx("0.1.0", "android");
        assert!(config.twitch_flags(&c).chapters_gql);
        config.kill_switches.push(KillSwitch {
            feature: "*".into(),
            min_app_version: None,
            max_app_version: None,
            platforms: vec![],
            reason: Some("integrity token required".into()),
        });
        let t = config.twitch_flags(&c);
        assert!(!t.gql_playback && !t.chapters_gql);
        assert_eq!(t.player, PlayerMode::Embed);
        assert!(t.gql_client_id.is_none() && t.persisted_queries.is_empty());
    }

    #[test]
    fn unknown_player_and_fields_are_tolerated_but_new_schema_is_not() {
        let c = RemoteConfig::parse(
            br#"{ "schema": 1, "serial": 1, "future_field": 1, "twitch": { "player": "hologram" } }"#,
        )
        .unwrap();
        assert_eq!(c.twitch.player, PlayerMode::Embed);
        assert!(matches!(
            RemoteConfig::parse(br#"{ "schema": 2, "serial": 1 }"#),
            Err(Error::UnsupportedSchema(2))
        ));
        assert!(matches!(
            RemoteConfig::parse(br#"{ "schema": 1, "serial": 1, "kill_switches": [ { "feature": "a", "max_app_version": "soon" } ] }"#),
            Err(Error::Invalid(_))
        ));
    }

    #[test]
    fn refresh_policy_is_clamped() {
        let p = RefreshPolicy {
            interval_secs: 1,
            retry_secs: 1,
        };
        assert_eq!(p.interval(), MIN_INTERVAL_SECS);
        assert_eq!(p.retry(), MIN_RETRY_SECS);
        let p = RefreshPolicy {
            interval_secs: u64::MAX,
            retry_secs: u64::MAX,
        };
        assert_eq!(p.interval(), MAX_INTERVAL_SECS);
        assert_eq!(p.retry(), MAX_INTERVAL_SECS);
    }
}
