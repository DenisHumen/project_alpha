// SPDX-License-Identifier: MPL-2.0
//! Server configuration: an optional TOML file, then `ALPHA_MEDIA_*`
//! environment variables on top.
//!
//! ```toml
//! listen = "0.0.0.0:8487"
//! data_dir = "/var/lib/alpha-media"
//!
//! [tls]
//! enabled = true            # false: plain HTTP (old TVs), explicit opt-out
//! cert = "/etc/alpha/cert.pem"  # optional, both or neither; default: a
//! key = "/etc/alpha/key.pem"    # self-signed pair in data_dir/tls
//!
//! [discovery]
//! enabled = true            # mDNS `_projectalpha._tcp` (not on loopback)
//! name = "Living room"      # default: host name
//!
//! [log]
//! format = "json"   # or "text"
//! filter = "info"   # tracing EnvFilter syntax
//!
//! [sync]
//! playback_ttl_secs = 600   # how long the hub remembers "now playing"
//! max_connections = 256     # devices connected to the sync hub at once
//! require_pairing = true    # only paired devices may sync (SYNC-005)
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use pa_runtime::{Platform, PlatformDirs};
use serde::Deserialize;

use crate::Error;

/// Default port: 8487, unassigned by IANA (8475–8499 is free) and not used
/// by Jellyfin/Emby (8096, 8920), Plex (32400), Syncthing (8384), NAS admin
/// UIs (5000/5001, 8080) or the *arr apps, so they can run side by side.
/// ADR-NNNN.
pub const DEFAULT_PORT: u16 = 8487;

/// Default address. Loopback only: until authentication (SRV-003) exists,
/// exposing the server to the LAN is an explicit choice.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8487";

/// Name of the app directory when `data_dir` is not set (platform data,
/// config and cache locations of `pa-runtime`).
pub const APP_DIR_NAME: &str = "kinolune-server";

/// Library database file inside the data directory.
pub const LIBRARY_FILE: &str = "library.sqlite3";

/// Environment variables that override the file.
pub const ENV_LISTEN: &str = "ALPHA_MEDIA_LISTEN";
/// See [`ENV_LISTEN`].
pub const ENV_DATA_DIR: &str = "ALPHA_MEDIA_DATA_DIR";
/// See [`ENV_LISTEN`].
pub const ENV_LOG_FORMAT: &str = "ALPHA_MEDIA_LOG_FORMAT";
/// See [`ENV_LISTEN`].
pub const ENV_LOG_FILTER: &str = "ALPHA_MEDIA_LOG";
/// See [`ENV_LISTEN`]: `on`/`off` for [`TlsConfig::enabled`].
pub const ENV_TLS: &str = "ALPHA_MEDIA_TLS";
/// See [`ENV_LISTEN`]: `on`/`off` for [`DiscoveryConfig::enabled`].
pub const ENV_MDNS: &str = "ALPHA_MEDIA_MDNS";

/// Server configuration.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct Config {
    /// Address and port to listen on.
    pub listen: SocketAddr,
    /// State directory (library database, settings, cache). `None`: the
    /// platform default of `pa-runtime` for [`APP_DIR_NAME`].
    pub data_dir: Option<PathBuf>,
    /// Logging.
    pub log: LogConfig,
    /// Sync hub (`/api/v1/sync`).
    pub sync: SyncConfig,
    /// HTTPS.
    pub tls: TlsConfig,
    /// mDNS announcement.
    pub discovery: DiscoveryConfig,
}

/// TLS settings (SRV-004).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct TlsConfig {
    /// Serve HTTPS. On by default; plain HTTP only by explicit choice.
    pub enabled: bool,
    /// PEM certificate chain (leaf first). With [`key`](Self::key); when
    /// both are absent a self-signed pair is created in `data_dir/tls`.
    pub cert: Option<PathBuf>,
    /// PEM private key (PKCS#8, PKCS#1 or SEC1).
    pub key: Option<PathBuf>,
}

impl Default for TlsConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            cert: None,
            key: None,
        }
    }
}

/// mDNS/DNS-SD announcement settings (SRV-004, ADR-0019).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct DiscoveryConfig {
    /// Announce `_projectalpha._tcp`. Never done while listening on a
    /// loopback address: nobody else could connect.
    pub enabled: bool,
    /// Display name (TXT `name` and instance name); the host name if absent.
    pub name: Option<String>,
}

impl Default for DiscoveryConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            name: None,
        }
    }
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: DEFAULT_LISTEN.parse().expect("valid default address"),
            data_dir: None,
            log: LogConfig::default(),
            sync: SyncConfig::default(),
            tls: TlsConfig::default(),
            discovery: DiscoveryConfig::default(),
        }
    }
}

/// Sync hub settings (SYNC-008).
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct SyncConfig {
    /// How long the last `pb.update` per profile and title is kept in memory
    /// and replayed to devices that connect (instant resume).
    pub playback_ttl_secs: u64,
    /// Upper bound of remembered `pb.update`s (memory guard).
    pub playback_max_entries: usize,
    /// Devices connected to the hub at once; more get HTTP 503.
    pub max_connections: u32,
    /// Only devices paired with the server may sync; others get
    /// `error{unauthorized}`. `false` admits unpaired devices too (tests,
    /// a trusted loopback setup).
    pub require_pairing: bool,
}

impl Default for SyncConfig {
    fn default() -> Self {
        Self {
            playback_ttl_secs: 600,
            playback_max_entries: 10_000,
            max_connections: 256,
            require_pairing: true,
        }
    }
}

/// Logging settings.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(default, deny_unknown_fields)]
pub struct LogConfig {
    /// Output format.
    pub format: LogFormat,
    /// `tracing` filter directives, e.g. `info,pa_server=debug`.
    pub filter: String,
}

impl Default for LogConfig {
    fn default() -> Self {
        Self {
            format: LogFormat::Text,
            filter: "info".to_owned(),
        }
    }
}

/// Log output format.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum LogFormat {
    /// Human-readable lines with key=value fields.
    Text,
    /// One JSON object per line (journald, log collectors).
    Json,
}

impl Config {
    /// Parses a TOML document.
    pub fn from_toml(text: &str) -> Result<Self, Error> {
        toml::from_str(text).map_err(|e| Error::Config(e.to_string()))
    }

    /// Reads `path` (if given), then applies the environment.
    pub fn load(path: Option<&Path>) -> Result<Self, Error> {
        let mut config = match path {
            Some(path) => {
                let text = std::fs::read_to_string(path)
                    .map_err(|e| Error::Config(format!("{}: {e}", path.display())))?;
                Self::from_toml(&text)?
            }
            None => Self::default(),
        };
        config.apply_env(|name| std::env::var(name).ok())?;
        Ok(config)
    }

    /// Overrides fields from environment variables, read through `var`.
    pub fn apply_env(&mut self, var: impl Fn(&str) -> Option<String>) -> Result<(), Error> {
        if let Some(listen) = var(ENV_LISTEN) {
            self.listen = listen
                .parse()
                .map_err(|e| Error::Config(format!("{ENV_LISTEN}: {e}")))?;
        }
        if let Some(dir) = var(ENV_DATA_DIR) {
            self.data_dir = Some(PathBuf::from(dir));
        }
        if let Some(format) = var(ENV_LOG_FORMAT) {
            self.log.format = match format.as_str() {
                "text" => LogFormat::Text,
                "json" => LogFormat::Json,
                other => {
                    return Err(Error::Config(format!(
                        "{ENV_LOG_FORMAT}: expected text or json, got {other:?}"
                    )));
                }
            };
        }
        if let Some(filter) = var(ENV_LOG_FILTER) {
            self.log.filter = filter;
        }
        if let Some(value) = var(ENV_TLS) {
            self.tls.enabled = parse_switch(ENV_TLS, &value)?;
        }
        if let Some(value) = var(ENV_MDNS) {
            self.discovery.enabled = parse_switch(ENV_MDNS, &value)?;
        }
        Ok(())
    }

    /// Directories for `pa-runtime`. With `data_dir` set, settings live in it
    /// and the cache in `data_dir/cache`.
    pub fn dirs(&self) -> Result<PlatformDirs, Error> {
        match &self.data_dir {
            Some(dir) => {
                let dir = std::path::absolute(dir)
                    .map_err(|e| Error::Config(format!("data_dir: {e}")))?;
                Ok(PlatformDirs::new(
                    dir.clone(),
                    dir.clone(),
                    dir.join("cache"),
                ))
            }
            None => Ok(PlatformDirs::platform_default(
                Platform::current(),
                APP_DIR_NAME,
            )?),
        }
    }
}

fn parse_switch(name: &str, value: &str) -> Result<bool, Error> {
    match value.to_ascii_lowercase().as_str() {
        "1" | "on" | "true" | "yes" => Ok(true),
        "0" | "off" | "false" | "no" => Ok(false),
        other => Err(Error::Config(format!(
            "{name}: expected on or off, got {other:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
        assert!(Config::default().listen.ip().is_loopback());
        assert_eq!(Config::default().listen.port(), DEFAULT_PORT);
        assert!(Config::default().tls.enabled);
        assert!(Config::default().discovery.enabled);
    }

    #[test]
    fn tls_and_discovery_sections() {
        let config = Config::from_toml(
            "[tls]\nenabled = false\ncert = \"/c.pem\"\nkey = \"/k.pem\"\n[discovery]\nname = \"Den\"\n",
        )
        .unwrap();
        assert!(!config.tls.enabled);
        assert_eq!(config.tls.cert, Some(PathBuf::from("/c.pem")));
        assert_eq!(config.discovery.name.as_deref(), Some("Den"));
        assert!(config.discovery.enabled);

        let mut config = Config::default();
        config
            .apply_env(|n| match n {
                ENV_TLS => Some("off".to_owned()),
                ENV_MDNS => Some("0".to_owned()),
                _ => None,
            })
            .unwrap();
        assert!(!config.tls.enabled);
        assert!(!config.discovery.enabled);
        assert!(
            config
                .apply_env(|n| (n == ENV_TLS).then(|| "maybe".to_owned()))
                .is_err()
        );
    }

    #[test]
    fn file_then_env() {
        let mut config = Config::from_toml(
            "listen = \"0.0.0.0:9000\"\ndata_dir = \"/srv/media\"\n[log]\nformat = \"json\"\n",
        )
        .unwrap();
        assert_eq!(config.listen.port(), 9000);
        assert_eq!(config.log.format, LogFormat::Json);
        assert_eq!(config.sync, SyncConfig::default());
        let sync = Config::from_toml("[sync]\nmax_connections = 8\n")
            .unwrap()
            .sync;
        assert_eq!(sync.max_connections, 8);
        assert_eq!(sync.playback_ttl_secs, 600);
        assert!(sync.require_pairing);

        config
            .apply_env(|name| match name {
                ENV_LISTEN => Some("[::1]:9001".to_owned()),
                ENV_LOG_FILTER => Some("debug".to_owned()),
                _ => None,
            })
            .unwrap();
        assert_eq!(config.listen, "[::1]:9001".parse().unwrap());
        assert_eq!(config.log.filter, "debug");
        assert_eq!(config.data_dir, Some(PathBuf::from("/srv/media")));
    }

    #[test]
    fn rejects_unknown_keys_and_bad_values() {
        assert!(Config::from_toml("port = 1").is_err());
        let mut config = Config::default();
        assert!(
            config
                .apply_env(|n| (n == ENV_LOG_FORMAT).then(|| "xml".to_owned()))
                .is_err()
        );
    }
}
