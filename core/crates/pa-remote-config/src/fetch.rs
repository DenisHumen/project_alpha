// SPDX-License-Identifier: MPL-2.0
//! Downloading: the [`ConfigFetcher`] seam and the live HTTPS fetcher.

/// Largest config or signature file accepted.
pub const MAX_CONFIG_BYTES: u64 = 256 * 1024;

/// A response.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Fetched {
    /// HTTP status code (`304` for a matching `If-None-Match`).
    pub status: u16,
    /// `ETag` response header, verbatim.
    pub etag: Option<String>,
    /// Response body (empty for `304`).
    pub body: Vec<u8>,
}

/// The request never produced a response (DNS, TCP, TLS, timeout, body over
/// [`MAX_CONFIG_BYTES`]). For the client this means "offline".
#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
#[error("{0}")]
pub struct FetchError(pub String);

/// Fetches a static file. Implementations must be usable from several threads.
pub trait ConfigFetcher: Send + Sync {
    /// `GET url`, with `If-None-Match: <etag>` when given. Non-2xx statuses
    /// are responses, not errors.
    fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<Fetched, FetchError>;
}

#[cfg(feature = "http")]
pub use live::UreqFetcher;

#[cfg(feature = "http")]
mod live {
    use std::time::Duration;

    use super::{ConfigFetcher, FetchError, Fetched, MAX_CONFIG_BYTES};

    /// Blocking HTTPS fetcher on `ureq` with rustls (ring) and the Mozilla
    /// root store — the same stack as `pa-metadata` (ADR-0018), no OpenSSL.
    /// Sends no cookies and no user data: a plain `GET` of a public file.
    #[derive(Debug, Clone)]
    pub struct UreqFetcher {
        agent: ureq::Agent,
    }

    impl UreqFetcher {
        /// A fetcher with the given `User-Agent` and a 20 s overall timeout.
        pub fn new(user_agent: &str) -> Self {
            let config = ureq::Agent::config_builder()
                .http_status_as_error(false)
                .timeout_global(Some(Duration::from_secs(20)))
                .user_agent(user_agent)
                .build();
            Self {
                agent: ureq::Agent::new_with_config(config),
            }
        }
    }

    impl ConfigFetcher for UreqFetcher {
        fn get(&self, url: &str, if_none_match: Option<&str>) -> Result<Fetched, FetchError> {
            let mut request = self.agent.get(url);
            if let Some(etag) = if_none_match {
                request = request.header("if-none-match", etag);
            }
            let mut response = request.call().map_err(|e| FetchError(describe(&e)))?;
            let status = response.status().as_u16();
            let etag = response
                .headers()
                .get("etag")
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned);
            let body = if status == 304 {
                Vec::new()
            } else {
                response
                    .body_mut()
                    .with_config()
                    .limit(MAX_CONFIG_BYTES)
                    .read_to_vec()
                    .map_err(|e| FetchError(describe(&e)))?
            };
            Ok(Fetched { status, etag, body })
        }
    }

    fn describe(e: &ureq::Error) -> String {
        match e {
            ureq::Error::Timeout(_) => "request timed out".to_owned(),
            ureq::Error::HostNotFound => "host not found".to_owned(),
            ureq::Error::ConnectionFailed => "connection failed".to_owned(),
            ureq::Error::Io(io) => format!("I/O error: {}", io.kind()),
            ureq::Error::Tls(_) | ureq::Error::Rustls(_) => "TLS error".to_owned(),
            ureq::Error::BodyExceedsLimit(_) => "file too large".to_owned(),
            _ => "HTTP transport error".to_owned(),
        }
    }
}
