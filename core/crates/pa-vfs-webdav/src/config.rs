// SPDX-License-Identifier: MPL-2.0
//! Source configuration.

use std::fmt;
use std::time::Duration;

use crate::tls::TlsTrust;

/// User name and password of a WebDAV source.
///
/// `Debug` never prints the password. The source manager keeps the password
/// in the platform secret store (CORE-007) and builds this value only to
/// connect.
#[derive(Clone, PartialEq, Eq)]
pub struct Credentials {
    /// User name.
    pub username: String,
    /// Password or app token (Nextcloud app passwords work as is).
    pub password: String,
}

impl Credentials {
    /// Credentials from a user name and a password.
    pub fn new(username: impl Into<String>, password: impl Into<String>) -> Self {
        Self {
            username: username.into(),
            password: password.into(),
        }
    }
}

impl fmt::Debug for Credentials {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("Credentials")
            .field("username", &self.username)
            .field("password", &"<redacted>")
            .finish()
    }
}

/// Default read-ahead block: big enough that a player reading 32–64 KiB at a
/// time makes one HTTP request per megabyte, small enough for fast seeks.
pub const DEFAULT_READ_AHEAD: usize = 1024 * 1024;

/// How to reach one WebDAV source.
#[derive(Debug, Clone)]
pub struct WebDavConfig {
    /// Root collection, e.g. `https://nas.local/remote.php/dav/files/anna/`.
    /// Must not contain a user name or password.
    pub url: String,
    /// `None` for anonymous access.
    pub credentials: Option<Credentials>,
    /// Which server certificates to accept.
    pub tls: TlsTrust,
    /// TCP and TLS connect timeout.
    pub connect_timeout: Duration,
    /// Whole-request timeout (connect, headers and body).
    pub timeout: Duration,
    /// Minimal size of one ranged GET in bytes; smaller reads are served
    /// from the fetched block. `0` disables read-ahead.
    pub read_ahead: usize,
    /// How many read-ahead blocks each opened file keeps (several threads
    /// reading different regions, e.g. the demuxer and the index at the end).
    pub cache_blocks: usize,
}

impl WebDavConfig {
    /// Anonymous source at `url` with default timeouts and read-ahead.
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            credentials: None,
            tls: TlsTrust::System,
            connect_timeout: Duration::from_secs(10),
            timeout: Duration::from_secs(60),
            read_ahead: DEFAULT_READ_AHEAD,
            cache_blocks: 4,
        }
    }

    /// Sets the credentials.
    #[must_use]
    pub fn with_credentials(mut self, credentials: Credentials) -> Self {
        self.credentials = Some(credentials);
        self
    }

    /// Sets the TLS trust mode.
    #[must_use]
    pub fn with_tls(mut self, tls: TlsTrust) -> Self {
        self.tls = tls;
        self
    }

    /// Sets the read-ahead block size (`0` disables it).
    #[must_use]
    pub fn with_read_ahead(mut self, bytes: usize) -> Self {
        self.read_ahead = bytes;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_hides_password() {
        let c = WebDavConfig::new("http://h/").with_credentials(Credentials::new("anna", "hunter2"));
        let s = format!("{c:?}");
        assert!(s.contains("anna"));
        assert!(!s.contains("hunter2"), "{s}");
    }
}
