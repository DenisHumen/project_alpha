// SPDX-License-Identifier: MPL-2.0
//! Structured logs on stderr: `key=value` text or one JSON object per line.
//!
//! Secrets (passwords, tokens) never go into fields; request logs carry the
//! path without the query string.

use tracing_subscriber::EnvFilter;

use crate::Error;
use crate::config::{LogConfig, LogFormat};

/// Installs the global subscriber. Call once, before anything logs.
pub fn init(config: &LogConfig) -> Result<(), Error> {
    let filter = EnvFilter::try_new(&config.filter)
        .map_err(|e| Error::Config(format!("log filter {:?}: {e}", config.filter)))?;
    let builder = tracing_subscriber::fmt()
        .with_env_filter(filter)
        .with_writer(std::io::stderr)
        .with_target(true);
    let result = match config.format {
        LogFormat::Text => builder.try_init(),
        LogFormat::Json => builder.json().flatten_event(true).try_init(),
    };
    result.map_err(|e| Error::Config(format!("logging: {e}")))
}
