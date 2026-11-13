// SPDX-License-Identifier: MPL-2.0
//! Server configuration: an optional TOML file, then `ALPHA_MEDIA_*`
//! environment variables on top.
//!
//! ```toml
//! listen = "127.0.0.1:8096"
//! data_dir = "/var/lib/alpha-media"
//!
//! [log]
//! format = "json"   # or "text"
//! filter = "info"   # tracing EnvFilter syntax
//! ```

use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use pa_runtime::{Platform, PlatformDirs};
use serde::Deserialize;

use crate::Error;

/// Default address. Loopback only: until authentication (SRV-003) and TLS
/// (SRV-004) exist, exposing the server to the LAN is an explicit choice.
pub const DEFAULT_LISTEN: &str = "127.0.0.1:8096";

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
}

impl Default for Config {
    fn default() -> Self {
        Self {
            listen: DEFAULT_LISTEN.parse().expect("valid default address"),
            data_dir: None,
            log: LogConfig::default(),
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
        Ok(())
    }

    /// Directories for `pa-runtime`. With `data_dir` set, settings live in it
    /// and the cache in `data_dir/cache`.
    pub fn dirs(&self) -> Result<PlatformDirs, Error> {
        match &self.data_dir {
            Some(dir) => {
                let dir = std::path::absolute(dir)
                    .map_err(|e| Error::Config(format!("data_dir: {e}")))?;
                Ok(PlatformDirs::new(dir.clone(), dir.clone(), dir.join("cache")))
            }
            None => Ok(PlatformDirs::platform_default(
                Platform::current(),
                APP_DIR_NAME,
            )?),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_file_gives_defaults() {
        assert_eq!(Config::from_toml("").unwrap(), Config::default());
        assert!(Config::default().listen.ip().is_loopback());
    }

    #[test]
    fn file_then_env() {
        let mut config = Config::from_toml(
            "listen = \"0.0.0.0:9000\"\ndata_dir = \"/srv/media\"\n[log]\nformat = \"json\"\n",
        )
        .unwrap();
        assert_eq!(config.listen.port(), 9000);
        assert_eq!(config.log.format, LogFormat::Json);

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
